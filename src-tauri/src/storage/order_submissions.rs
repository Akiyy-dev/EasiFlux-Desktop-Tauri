use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};
use crate::models::config::APP_NAME;
use crate::models::order_submission::{
    PendingOrderSubmission, StoredSubmission, SubmissionOutcome,
};
use crate::models::trading::PlaceOrderRequest;

const SCHEMA_VERSION: u32 = 1;

/// The caller serializes account mutations while an intent is written or completed.
/// A damaged intent remains on disk and blocks a retry of the same identity.
pub struct OrderSubmissionStore {
    dir: PathBuf,
}

impl OrderSubmissionStore {
    pub fn new() -> Self {
        Self::with_dir(
            dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(APP_NAME)
                .join("order_submissions"),
        )
    }

    pub fn with_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn get(&self, scope: &str, id: &str) -> AppResult<Option<StoredSubmission>> {
        validate_identity(id)?;
        let path = self
            .scope_dir(scope)?
            .join(format!("{}.json", identity_hash(id)));
        read_record(&path, scope, Some(id))
    }

    pub fn begin(
        &self,
        scope: &str,
        request: &PlaceOrderRequest,
        now: u64,
    ) -> AppResult<StoredSubmission> {
        let id = request
            .order_link_id
            .as_deref()
            .ok_or_else(|| storage_error("订单请求缺少提交标识"))?;
        if let Some(existing) = self.get(scope, id)? {
            return Ok(existing);
        }

        let record = StoredSubmission {
            schema_version: SCHEMA_VERSION,
            scope: scope.to_owned(),
            order_link_id: id.to_owned(),
            request: request.clone(),
            created_at_ms: now,
            outcome: SubmissionOutcome::Pending,
            acknowledged: false,
        };
        let contents = serialize_record(&record)?;
        let dir = self.scope_dir(scope)?;
        fs::create_dir_all(&dir).map_err(io_error)?;
        let path = dir.join(format!("{}.json", identity_hash(id)));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                return self
                    .get(scope, id)?
                    .ok_or_else(|| storage_error("订单提交记录在读取期间发生变化"));
            }
            Err(error) => return Err(io_error(error)),
        };
        // Never remove a partial file after failure: its existence must block resubmission.
        file.write_all(&contents).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        Ok(record)
    }

    pub fn list_pending(&self, scope: &str) -> AppResult<Vec<PendingOrderSubmission>> {
        let dir = self.scope_dir(scope)?;
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(io_error(error)),
        };
        let mut names = BTreeSet::new();
        for entry in entries {
            let entry = entry.map_err(io_error)?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| storage_error("订单提交记录文件名无效"))?;
            let main_name = name
                .strip_suffix(".tmp")
                .or_else(|| name.strip_suffix(".bak"))
                .unwrap_or(name);
            let hash = main_name
                .strip_suffix(".json")
                .filter(|hash| {
                    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                .ok_or_else(|| storage_error("订单提交记录文件名无效"))?;
            names.insert(format!("{hash}.json"));
        }
        let mut pending = Vec::new();
        for name in names {
            let record = read_record(&dir.join(name), scope, None)?
                .ok_or_else(|| storage_error("订单提交记录在读取期间发生变化"))?;
            if matches!(record.outcome, SubmissionOutcome::Pending)
                || (matches!(record.outcome, SubmissionOutcome::Accepted(_))
                    && !record.acknowledged)
            {
                pending.push(PendingOrderSubmission::from(&record));
            }
        }
        pending.sort_by(|left, right| {
            (left.created_at_ms, &left.order_link_id)
                .cmp(&(right.created_at_ms, &right.order_link_id))
        });
        Ok(pending)
    }

    pub fn finish(&self, scope: &str, id: &str, outcome: SubmissionOutcome) -> AppResult<()> {
        let mut record = self
            .get(scope, id)?
            .ok_or_else(|| storage_error("找不到原订单提交记录"))?;
        record.outcome = outcome;
        let path = self
            .scope_dir(scope)?
            .join(format!("{}.json", identity_hash(id)));
        super::config_persistence::save(&path, &serialize_record(&record)?)
            .map_err(|_| storage_error("无法保存订单提交结果，请查询原订单"))
    }

    pub fn acknowledge(&self, scope: &str, id: &str) -> AppResult<()> {
        let mut record = self
            .get(scope, id)?
            .ok_or_else(|| storage_error("找不到原订单提交记录"))?;
        if !matches!(record.outcome, SubmissionOutcome::Accepted(_)) {
            return Err(storage_error("仅已确认成功的订单可确认收据"));
        }
        if record.acknowledged {
            return Ok(());
        }
        record.acknowledged = true;
        let path = self
            .scope_dir(scope)?
            .join(format!("{}.json", identity_hash(id)));
        super::config_persistence::save(&path, &serialize_record(&record)?)
            .map_err(|_| storage_error("无法保存订单收据确认，请重新查询原订单"))
    }

    fn scope_dir(&self, scope: &str) -> AppResult<PathBuf> {
        if scope.is_empty() {
            return Err(storage_error("订单提交记录缺少账户范围"));
        }
        Ok(self.dir.join(identity_hash(scope)))
    }
}

impl Default for OrderSubmissionStore {
    fn default() -> Self {
        Self::new()
    }
}

fn identity_hash(id: &str) -> String {
    hex::encode(Sha256::digest(id.as_bytes()))
}

fn validate_identity(id: &str) -> AppResult<()> {
    if id.is_empty() || id.len() > 36 {
        return Err(storage_error("订单提交标识长度必须为 1 至 36 字节"));
    }
    Ok(())
}

fn read_record(path: &Path, scope: &str, id: Option<&str>) -> AppResult<Option<StoredSubmission>> {
    let mut temp = path.as_os_str().to_os_string();
    temp.push(".tmp");
    let mut backup = path.as_os_str().to_os_string();
    backup.push(".bak");
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(temp),
        PathBuf::from(backup),
    ] {
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => return Err(storage_error("订单提交记录不是普通文件")),
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(io_error(error)),
        }
        let contents = fs::read(&candidate).map_err(io_error)?;
        let record: StoredSubmission = serde_json::from_slice(&contents)
            .map_err(|_| storage_error("订单提交记录已损坏，请先恢复原记录"))?;
        if record.schema_version != SCHEMA_VERSION {
            return Err(storage_error("不支持的订单提交记录版本"));
        }
        validate_identity(&record.order_link_id)?;
        if record.scope != scope
            || id.is_some_and(|id| id != record.order_link_id)
            || record.request.order_link_id.as_deref() != Some(record.order_link_id.as_str())
            || path.file_stem().and_then(|name| name.to_str())
                != Some(identity_hash(&record.order_link_id).as_str())
        {
            return Err(storage_error("订单提交记录与原请求标识不一致"));
        }
        return Ok(Some(record));
    }
    Ok(None)
}

fn serialize_record(record: &StoredSubmission) -> AppResult<Vec<u8>> {
    serde_json::to_vec(record).map_err(|_| storage_error("无法编码订单提交记录"))
}

fn storage_error(message: &str) -> AppError {
    AppError::Storage(message.to_owned())
}

fn io_error(_error: std::io::Error) -> AppError {
    storage_error("订单提交记录存储不可用，请检查存储后再查询原订单")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trading::{Order, OrderStatus};
    use std::fs;
    use std::path::Path;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "easiflux-order-submissions-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn store(&self) -> OrderSubmissionStore {
            OrderSubmissionStore::with_dir(self.0.clone())
        }

        fn record_path(&self) -> PathBuf {
            let scope = fs::read_dir(&self.0)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            fs::read_dir(scope).unwrap().next().unwrap().unwrap().path()
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            // Only the uniquely created test directory is removed.
            let temp = std::env::temp_dir();
            if self.0.parent() == Some(temp.as_path())
                && self
                    .0
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("easiflux-order-submissions-")
            {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    fn request(id: &str) -> PlaceOrderRequest {
        PlaceOrderRequest {
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Limit".into(),
            qty: "0.01".into(),
            position_idx: 0,
            price: Some("50000".into()),
            time_in_force: Some("GTC".into()),
            order_link_id: Some(id.into()),
            reduce_only: Some(false),
        }
    }

    fn accepted_order(id: &str) -> Order {
        Order {
            order_id: "exchange-order-1".into(),
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Limit".into(),
            price: "50000".into(),
            qty: "0.01".into(),
            status: OrderStatus::New,
            order_link_id: Some(id.into()),
            filled_qty: "0".into(),
            avg_price: "0".into(),
        }
    }

    fn rewrite_record(path: &Path, update: impl FnOnce(&mut serde_json::Value)) {
        let mut record = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        update(&mut record);
        fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
    }

    #[test]
    fn pending_record_survives_restart_with_flat_camel_case_details() {
        let dir = TestDirectory::new();
        dir.store()
            .begin("account-a", &request("original-id"), 123)
            .unwrap();

        let restarted = dir.store();
        let record = restarted.get("account-a", "original-id").unwrap().unwrap();
        assert!(matches!(record.outcome, SubmissionOutcome::Pending));
        assert_eq!(record.created_at_ms, 123);
        let pending = restarted.list_pending("account-a").unwrap();
        assert_eq!(
            serde_json::to_value(pending).unwrap(),
            serde_json::json!([{
                "orderLinkId": "original-id", "symbol": "BTCUSDT", "side": "Buy",
                "orderType": "Limit", "qty": "0.01", "price": "50000",
                "reduceOnly": false, "createdAtMs": 123
            }])
        );
    }

    #[test]
    fn accepted_result_survives_restart_and_stays_recoverable_until_acknowledged() {
        let dir = TestDirectory::new();
        let store = dir.store();
        store
            .begin("account-a", &request("original-id"), 123)
            .unwrap();
        store
            .finish(
                "account-a",
                "original-id",
                SubmissionOutcome::Accepted(Box::new(accepted_order("original-id"))),
            )
            .unwrap();

        let restarted = dir.store();
        let record = restarted.get("account-a", "original-id").unwrap().unwrap();
        let SubmissionOutcome::Accepted(order) = record.outcome else {
            panic!("accepted outcome lost")
        };
        assert_eq!(order.order_id, "exchange-order-1");
        assert!(!record.acknowledged);
        assert_eq!(restarted.list_pending("account-a").unwrap().len(), 1);

        restarted.acknowledge("account-a", "original-id").unwrap();

        let acknowledged = dir.store();
        assert!(acknowledged.list_pending("account-a").unwrap().is_empty());
        let replay = acknowledged
            .begin("account-a", &request("original-id"), 999)
            .unwrap();
        assert!(replay.acknowledged);
        assert_eq!(replay.created_at_ms, 123);
        let SubmissionOutcome::Accepted(order) = replay.outcome else {
            panic!("acknowledgement removed the original accepted receipt")
        };
        assert_eq!(order.order_id, "exchange-order-1");
    }

    #[test]
    fn pending_and_rejected_records_cannot_be_acknowledged() {
        let dir = TestDirectory::new();
        let store = dir.store();
        store
            .begin("account-a", &request("original-id"), 123)
            .unwrap();

        assert!(store.acknowledge("account-a", "original-id").is_err());
        assert_eq!(store.list_pending("account-a").unwrap().len(), 1);
        assert!(
            !store
                .get("account-a", "original-id")
                .unwrap()
                .unwrap()
                .acknowledged
        );

        store
            .finish("account-a", "original-id", SubmissionOutcome::Rejected)
            .unwrap();
        assert!(store.acknowledge("account-a", "original-id").is_err());
        assert!(
            !store
                .get("account-a", "original-id")
                .unwrap()
                .unwrap()
                .acknowledged
        );
        assert!(store.list_pending("account-a").unwrap().is_empty());
    }

    #[test]
    fn legacy_accepted_receipt_defaults_to_unacknowledged_and_remains_recoverable() {
        let dir = TestDirectory::new();
        let store = dir.store();
        store
            .begin("account-a", &request("original-id"), 123)
            .unwrap();
        let path = dir.record_path();
        store
            .finish(
                "account-a",
                "original-id",
                SubmissionOutcome::Accepted(Box::new(accepted_order("original-id"))),
            )
            .unwrap();
        rewrite_record(&path, |record| {
            record.as_object_mut().unwrap().remove("acknowledged");
        });

        let restarted = dir.store();
        assert!(
            !restarted
                .get("account-a", "original-id")
                .unwrap()
                .unwrap()
                .acknowledged
        );
        assert_eq!(restarted.list_pending("account-a").unwrap().len(), 1);
    }

    #[test]
    fn failed_acknowledgement_keeps_accepted_receipt_recoverable_after_restart() {
        let dir = TestDirectory::new();
        let store = dir.store();
        store
            .begin("account-a", &request("original-id"), 123)
            .unwrap();
        let path = dir.record_path();
        store
            .finish(
                "account-a",
                "original-id",
                SubmissionOutcome::Accepted(Box::new(accepted_order("original-id"))),
            )
            .unwrap();
        fs::create_dir(PathBuf::from(format!("{}.tmp", path.display()))).unwrap();

        assert!(store.acknowledge("account-a", "original-id").is_err());

        let restarted = dir.store();
        assert!(
            !restarted
                .get("account-a", "original-id")
                .unwrap()
                .unwrap()
                .acknowledged
        );
        assert_eq!(restarted.list_pending("account-a").unwrap().len(), 1);
    }

    #[test]
    fn beginning_existing_identity_never_changes_original_request_or_outcome() {
        let dir = TestDirectory::new();
        let store = dir.store();
        store
            .begin("account-a", &request("original-id"), 123)
            .unwrap();
        store
            .finish("account-a", "original-id", SubmissionOutcome::Rejected)
            .unwrap();
        let path = dir.record_path();
        let before = fs::read(&path).unwrap();
        let mut modified = request("original-id");
        modified.qty = "100".into();

        let record = store.begin("account-a", &modified, 999).unwrap();

        assert_eq!(record.request.qty, "0.01");
        assert_eq!(record.created_at_ms, 123);
        assert!(matches!(record.outcome, SubmissionOutcome::Rejected));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(store.list_pending("account-a").unwrap().is_empty());
    }

    #[test]
    fn corrupt_record_blocks_read_listing_and_duplicate_begin() {
        let dir = TestDirectory::new();
        let store = dir.store();
        store
            .begin("account-a", &request("original-id"), 123)
            .unwrap();
        let path = dir.record_path();
        fs::write(&path, b"{partial").unwrap();

        assert!(store.get("account-a", "original-id").is_err());
        assert!(store.list_pending("account-a").is_err());
        assert!(store
            .begin("account-a", &request("original-id"), 999)
            .is_err());
        assert_eq!(fs::read(path).unwrap(), b"{partial");
    }

    #[test]
    fn identical_ids_are_isolated_by_account_scope() {
        let dir = TestDirectory::new();
        let store = dir.store();
        store.begin("account-a", &request("same-id"), 123).unwrap();
        let mut other = request("same-id");
        other.symbol = "ETHUSDT".into();
        store.begin("account-b", &other, 456).unwrap();
        store
            .finish("account-a", "same-id", SubmissionOutcome::Rejected)
            .unwrap();

        assert!(store.list_pending("account-a").unwrap().is_empty());
        let pending = store.list_pending("account-b").unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].symbol, "ETHUSDT");
        assert_eq!(pending[0].created_at_ms, 456);
    }

    #[test]
    fn rejects_future_schema_even_when_older_backup_exists() {
        let dir = TestDirectory::new();
        let store = dir.store();
        store
            .begin("account-a", &request("original-id"), 123)
            .unwrap();
        let path = dir.record_path();
        fs::copy(&path, PathBuf::from(format!("{}.bak", path.display()))).unwrap();
        rewrite_record(&path, |record| record["schemaVersion"] = 2.into());

        assert!(store.get("account-a", "original-id").is_err());
        assert!(store.list_pending("account-a").is_err());
        assert!(store
            .begin("account-a", &request("original-id"), 999)
            .is_err());
    }

    #[test]
    fn mismatched_scope_identity_and_request_are_rejected() {
        for field in ["scope", "orderLinkId", "request"] {
            let dir = TestDirectory::new();
            let store = dir.store();
            store
                .begin("account-a", &request("original-id"), 123)
                .unwrap();
            let path = dir.record_path();
            rewrite_record(&path, |record| match field {
                "request" => record["request"]["orderLinkId"] = "different-id".into(),
                _ => record[field] = "different-value".into(),
            });

            assert!(store.get("account-a", "original-id").is_err(), "{field}");
            assert!(store.list_pending("account-a").is_err(), "{field}");
        }
    }

    #[test]
    fn storage_failure_blocks_begin_without_deleting_existing_file() {
        let dir = TestDirectory::new();
        let blocked = dir.0.join("blocked");
        fs::write(&blocked, b"keep").unwrap();
        let store = OrderSubmissionStore::with_dir(blocked.clone());

        assert!(store
            .begin("account-a", &request("original-id"), 123)
            .is_err());
        assert_eq!(fs::read(blocked).unwrap(), b"keep");
    }

    #[test]
    fn identifiers_cannot_escape_the_store_directory() {
        let dir = TestDirectory::new();
        let store = dir.store();
        store
            .begin("../../scope", &request("../../id"), 123)
            .unwrap();

        let pending = store.list_pending("../../scope").unwrap();
        assert_eq!(pending[0].order_link_id, "../../id");
        let scope = fs::read_dir(&dir.0).unwrap().next().unwrap().unwrap();
        assert!(scope
            .file_name()
            .to_string_lossy()
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
        let record = fs::read_dir(scope.path()).unwrap().next().unwrap().unwrap();
        assert_eq!(record.path().extension().unwrap(), "json");
        assert_eq!(record.path().file_stem().unwrap().len(), 64);
    }

    #[test]
    fn production_scope_has_a_fixed_size_directory_component() {
        let dir = TestDirectory::new();
        let scope = "a".repeat(64);
        dir.store()
            .begin(&scope, &request("original-id"), 123)
            .unwrap();

        let stored_scope = fs::read_dir(&dir.0).unwrap().next().unwrap().unwrap();
        assert_eq!(stored_scope.file_name().len(), 64);
        assert_eq!(dir.store().list_pending(&scope).unwrap().len(), 1);
    }

    #[test]
    fn missing_or_empty_order_identity_is_rejected_before_writing() {
        let dir = TestDirectory::new();
        let store = dir.store();
        let mut missing = request("original-id");
        missing.order_link_id = None;

        assert!(store.begin("account-a", &missing, 123).is_err());
        assert!(store.begin("account-a", &request(""), 123).is_err());
        assert!(store
            .begin("account-a", &request(&"x".repeat(37)), 123)
            .is_err());
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 0);
    }

    #[test]
    fn missing_main_recovers_pending_backup_and_blocks_duplicate_begin() {
        let dir = TestDirectory::new();
        let store = dir.store();
        store
            .begin("account-a", &request("original-id"), 123)
            .unwrap();
        let path = dir.record_path();
        fs::rename(&path, PathBuf::from(format!("{}.bak", path.display()))).unwrap();

        let record = store
            .begin("account-a", &request("original-id"), 999)
            .unwrap();
        assert_eq!(record.created_at_ms, 123);
        assert!(matches!(record.outcome, SubmissionOutcome::Pending));
        assert_eq!(store.list_pending("account-a").unwrap().len(), 1);
        assert!(!path.exists());
    }

    #[test]
    fn failed_finish_retains_pending_record_for_recovery() {
        let dir = TestDirectory::new();
        let store = dir.store();
        store
            .begin("account-a", &request("original-id"), 123)
            .unwrap();
        let path = dir.record_path();
        fs::create_dir(PathBuf::from(format!("{}.tmp", path.display()))).unwrap();

        assert!(store
            .finish("account-a", "original-id", SubmissionOutcome::Rejected)
            .is_err());
        assert!(matches!(
            store
                .get("account-a", "original-id")
                .unwrap()
                .unwrap()
                .outcome,
            SubmissionOutcome::Pending
        ));
        assert_eq!(store.list_pending("account-a").unwrap().len(), 1);
    }
}
