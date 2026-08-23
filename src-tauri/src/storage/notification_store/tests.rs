use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    FailurePoint, NotificationFileV1, NotificationLoadStatus, NotificationPartition,
    NotificationPersistence, NotificationStore, RecoverySource, NOTIFICATION_SCHEMA_VERSION,
};
use crate::error::AppError;
use crate::models::notification::{
    NotificationCategory, NotificationContent, NotificationKind, NotificationRecord,
    NotificationScalar, NotificationScope, NotificationSeverity, MAX_JAVASCRIPT_SAFE_INTEGER,
};

static TEST_ROOT_ID: AtomicU64 = AtomicU64::new(0);

fn test_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "easiflux-notification-store-{}-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed),
        label,
    ))
}

fn store_path(root: &Path) -> PathBuf {
    root.join("notifications.v1.json")
}

fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

fn sample_record(id: &str, scope: NotificationScope, created_at_ms: u64) -> NotificationRecord {
    NotificationRecord {
        id: id.into(),
        scope,
        category: NotificationCategory::Trading,
        kind: NotificationKind::OrderFilled,
        severity: NotificationSeverity::Success,
        content: NotificationContent::new(
            "order.filled",
            [("orderId", NotificationScalar::String("order-42".into()))],
            "订单已成交",
            "订单已完全成交，请前往交易页查看。",
        )
        .unwrap(),
        entity: None,
        action: None,
        source_event_id: Some(format!("event-{created_at_ms}")),
        dedupe_key: format!("order-filled-{created_at_ms}"),
        occurrence_count: 1,
        created_at_ms,
        updated_at_ms: created_at_ms,
        read_at_ms: None,
    }
}

fn sample_file(revision: u64) -> NotificationFileV1 {
    NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision,
        partitions: vec![NotificationPartition {
            scope: NotificationScope::Global,
            items: vec![sample_record(
                "00000000-0000-4000-8000-000000000001",
                NotificationScope::Global,
                100,
            )],
        }],
    }
}

fn write_json(path: &Path, value: &impl serde::Serialize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn read_file(path: &Path) -> NotificationFileV1 {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn assert_storage_unavailable(error: AppError) {
    assert!(
        matches!(error, AppError::Storage(ref message) if message == "NOTIFICATION_STORAGE_UNAVAILABLE")
    );
}

fn assert_persistence_is_send_sync<T: NotificationPersistence>() {}

#[test]
fn round_trip_preserves_revision_partitions_and_records() {
    let root = test_root("round-trip");
    let store = NotificationStore::with_path(store_path(&root));
    assert_persistence_is_send_sync::<NotificationStore>();
    let file = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 7,
        partitions: vec![
            NotificationPartition {
                scope: NotificationScope::Global,
                items: vec![sample_record(
                    "00000000-0000-4000-8000-000000000001",
                    NotificationScope::Global,
                    100,
                )],
            },
            NotificationPartition {
                scope: NotificationScope::Account {
                    account_id: "account_1".into(),
                },
                items: vec![sample_record(
                    "00000000-0000-4000-8000-000000000002",
                    NotificationScope::Account {
                        account_id: "account_1".into(),
                    },
                    200,
                )],
            },
        ],
    };

    store.save(&file).unwrap();
    let outcome = store.load().unwrap();

    assert_eq!(outcome.status, NotificationLoadStatus::Clean);
    assert_eq!(outcome.file, file);
    cleanup(&root);
}

#[test]
fn load_prefers_main_then_tmp_then_backup() {
    for (label, main, temp, backup, expected_revision, expected_status) in [
        (
            "main",
            Some(sample_file(1)),
            Some(sample_file(2)),
            Some(sample_file(3)),
            1,
            NotificationLoadStatus::Clean,
        ),
        (
            "temp",
            None,
            Some(sample_file(2)),
            Some(sample_file(3)),
            2,
            NotificationLoadStatus::Recovered {
                source: RecoverySource::Temp,
            },
        ),
        (
            "backup",
            None,
            None,
            Some(sample_file(3)),
            3,
            NotificationLoadStatus::Recovered {
                source: RecoverySource::Backup,
            },
        ),
    ] {
        let root = test_root(label);
        let path = store_path(&root);
        if let Some(file) = main {
            write_json(&path, &file);
        }
        if let Some(file) = temp {
            write_json(&NotificationStore::temp_path_for_test(&path), &file);
        }
        if let Some(file) = backup {
            write_json(&NotificationStore::backup_path_for_test(&path), &file);
        }

        let outcome = NotificationStore::with_path(path).load().unwrap();

        assert_eq!(outcome.file.revision, expected_revision);
        assert_eq!(outcome.status, expected_status);
        cleanup(&root);
    }
}

#[test]
fn future_main_schema_is_not_downgraded_to_an_older_backup() {
    let root = test_root("future-main");
    let path = store_path(&root);
    let future_bytes = br#"{"schemaVersion":2,"futureData":{"kept":true}}"#;
    fs::create_dir_all(&root).unwrap();
    fs::write(&path, future_bytes).unwrap();
    write_json(
        &NotificationStore::backup_path_for_test(&path),
        &sample_file(4),
    );

    let outcome = NotificationStore::with_path(path.clone()).load().unwrap();

    assert_eq!(
        outcome.status,
        NotificationLoadStatus::UnsupportedSchema { found: 2 }
    );
    assert_eq!(outcome.file, NotificationFileV1::empty());
    assert_eq!(fs::read(&path).unwrap(), future_bytes);
    assert_eq!(
        read_file(&NotificationStore::backup_path_for_test(&path)).revision,
        4
    );
    cleanup(&root);
}

#[test]
fn save_does_not_overwrite_or_rename_a_future_main_schema() {
    let root = test_root("future-main-save");
    let path = store_path(&root);
    let future_bytes = br#"{"schemaVersion":3,"futureData":{"kept":true}}"#;
    fs::create_dir_all(&root).unwrap();
    fs::write(&path, future_bytes).unwrap();

    assert_storage_unavailable(
        NotificationStore::with_path(path.clone())
            .save(&sample_file(8))
            .unwrap_err(),
    );

    assert_eq!(fs::read(&path).unwrap(), future_bytes);
    assert!(!NotificationStore::backup_path_for_test(&path).exists());
    assert!(!NotificationStore::temp_path_for_test(&path).exists());
    cleanup(&root);
}

#[test]
fn all_corrupt_v1_candidates_are_preserved_and_empty_state_is_returned() {
    let root = test_root("all-corrupt");
    let path = store_path(&root);
    let temp = NotificationStore::temp_path_for_test(&path);
    let backup = NotificationStore::backup_path_for_test(&path);
    fs::create_dir_all(&root).unwrap();
    fs::write(&path, b"not-json-main").unwrap();
    fs::write(&temp, br#"{"schemaVersion":1,"partitions":[]}"#).unwrap();
    fs::write(&backup, br#"{"schemaVersion":0}"#).unwrap();

    let outcome = NotificationStore::with_path(path.clone()).load().unwrap();

    assert_eq!(outcome.status, NotificationLoadStatus::ResetFromCorruption);
    assert_eq!(outcome.file, NotificationFileV1::empty());
    assert!(!path.exists());
    assert!(!temp.exists());
    assert!(!backup.exists());
    let evidence: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".corrupt-"))
        .collect();
    assert_eq!(evidence.len(), 3, "evidence files: {evidence:?}");
    cleanup(&root);
}

#[test]
fn duplicate_partition_scope_is_rejected() {
    let root = test_root("duplicate-scope");
    let store = NotificationStore::with_path(store_path(&root));
    let mut file = sample_file(1);
    file.partitions.push(NotificationPartition {
        scope: NotificationScope::Global,
        items: Vec::new(),
    });

    assert!(matches!(store.save(&file), Err(AppError::Storage(_))));
    assert!(!store.path_for_test().exists());
    cleanup(&root);
}

#[test]
fn duplicate_record_id_across_partitions_is_rejected() {
    let root = test_root("duplicate-id");
    let store = NotificationStore::with_path(store_path(&root));
    let mut file = sample_file(1);
    let duplicate_id = file.partitions[0].items[0].id.clone();
    let account_scope = NotificationScope::Account {
        account_id: "account_1".into(),
    };
    file.partitions.push(NotificationPartition {
        scope: account_scope.clone(),
        items: vec![sample_record(&duplicate_id, account_scope, 200)],
    });

    assert!(matches!(store.save(&file), Err(AppError::Storage(_))));
    assert!(!store.path_for_test().exists());
    cleanup(&root);
}

#[test]
fn invalid_timestamps_scalars_and_enums_are_rejected() {
    let timestamp_root = test_root("invalid-timestamp");
    let timestamp_store = NotificationStore::with_path(store_path(&timestamp_root));
    let mut invalid_timestamp = sample_file(1);
    invalid_timestamp.partitions[0].items[0].updated_at_ms = MAX_JAVASCRIPT_SAFE_INTEGER + 1;
    assert!(matches!(
        timestamp_store.save(&invalid_timestamp),
        Err(AppError::Storage(_))
    ));

    let scalar_root = test_root("invalid-scalar");
    let scalar_store = NotificationStore::with_path(store_path(&scalar_root));
    let mut invalid_scalar = sample_file(1);
    invalid_scalar.partitions[0].items[0].category = NotificationCategory::RiskAccount;
    invalid_scalar.partitions[0].items[0].kind = NotificationKind::RiskOrderBlocked;
    invalid_scalar.partitions[0].items[0].content = NotificationContent::new(
        "risk.orderBlocked",
        [("limit", NotificationScalar::Number(1.0))],
        "订单被风控拦截",
        "请检查风控设置",
    )
    .unwrap();
    invalid_scalar.partitions[0].items[0]
        .content
        .params
        .insert("limit".into(), NotificationScalar::Number(f64::NAN));
    assert!(matches!(
        scalar_store.save(&invalid_scalar),
        Err(AppError::Storage(_))
    ));

    let enum_root = test_root("invalid-enum");
    let enum_path = store_path(&enum_root);
    let mut invalid_enum = serde_json::to_value(sample_file(1)).unwrap();
    invalid_enum["partitions"][0]["items"][0]["severity"] =
        serde_json::Value::String("catastrophic".into());
    write_json(&enum_path, &invalid_enum);
    let outcome = NotificationStore::with_path(enum_path).load().unwrap();
    assert_eq!(outcome.status, NotificationLoadStatus::ResetFromCorruption);

    cleanup(&timestamp_root);
    cleanup(&scalar_root);
    cleanup(&enum_root);
}

#[test]
fn record_scope_must_match_its_partition() {
    let root = test_root("scope-mismatch");
    let store = NotificationStore::with_path(store_path(&root));
    let mut file = sample_file(1);
    file.partitions[0].items[0].scope = NotificationScope::Account {
        account_id: "account_1".into(),
    };

    assert!(matches!(store.save(&file), Err(AppError::Storage(_))));
    cleanup(&root);
}

#[test]
fn successful_save_rotates_main_to_backup() {
    let root = test_root("main-rotation");
    let path = store_path(&root);
    let store = NotificationStore::with_path(path.clone());

    store.save(&sample_file(1)).unwrap();
    store.save(&sample_file(2)).unwrap();

    assert_eq!(read_file(&path).revision, 2);
    assert_eq!(
        read_file(&NotificationStore::backup_path_for_test(&path)).revision,
        1
    );
    assert!(!NotificationStore::temp_path_for_test(&path).exists());
    cleanup(&root);
}

#[test]
fn tmp_promotion_failure_restores_the_backup() {
    let root = test_root("promotion-failure");
    let path = store_path(&root);
    NotificationStore::with_path(path.clone())
        .save(&sample_file(1))
        .unwrap();
    let failing =
        NotificationStore::with_path_and_failures(path.clone(), vec![FailurePoint::PromoteTemp]);

    assert_storage_unavailable(failing.save(&sample_file(2)).unwrap_err());

    assert_eq!(read_file(&path).revision, 1);
    let outcome = NotificationStore::with_path(path).load().unwrap();
    assert_eq!(outcome.status, NotificationLoadStatus::Clean);
    assert_eq!(outcome.file.revision, 1);
    cleanup(&root);
}

#[test]
fn backup_restore_failure_keeps_recovery_candidates() {
    let root = test_root("restore-failure");
    let path = store_path(&root);
    NotificationStore::with_path(path.clone())
        .save(&sample_file(1))
        .unwrap();
    let failing = NotificationStore::with_path_and_failures(
        path.clone(),
        vec![FailurePoint::PromoteTemp, FailurePoint::RestoreBackup],
    );

    assert_storage_unavailable(failing.save(&sample_file(2)).unwrap_err());

    assert!(!path.exists());
    assert_eq!(
        read_file(&NotificationStore::backup_path_for_test(&path)).revision,
        1
    );
    assert_eq!(
        read_file(&NotificationStore::temp_path_for_test(&path)).revision,
        2
    );
    let recovered = NotificationStore::with_path(path).load().unwrap();
    assert_eq!(recovered.file.revision, 2);
    assert_eq!(
        recovered.status,
        NotificationLoadStatus::Recovered {
            source: RecoverySource::Temp
        }
    );
    cleanup(&root);
}

#[test]
fn no_file_startup_returns_clean_empty_state_without_writing() {
    let root = test_root("no-files");
    let path = store_path(&root);

    let outcome = NotificationStore::with_path(path.clone()).load().unwrap();

    assert_eq!(outcome.status, NotificationLoadStatus::Clean);
    assert_eq!(outcome.file, NotificationFileV1::empty());
    assert!(!path.exists());
    assert!(!root.exists());
}

#[test]
fn recovered_main_is_normalized_without_changing_revision() {
    for (label, source) in [
        ("temp", RecoverySource::Temp),
        ("backup", RecoverySource::Backup),
    ] {
        let root = test_root(label);
        let path = store_path(&root);
        let candidate_path = match source {
            RecoverySource::Temp => NotificationStore::temp_path_for_test(&path),
            RecoverySource::Backup => NotificationStore::backup_path_for_test(&path),
        };
        write_json(&candidate_path, &sample_file(9));

        let outcome = NotificationStore::with_path(path.clone()).load().unwrap();

        assert_eq!(outcome.status, NotificationLoadStatus::Recovered { source });
        assert_eq!(outcome.file.revision, 9);
        assert_eq!(read_file(&path).revision, 9);
        cleanup(&root);
    }
}

#[test]
fn normalization_failure_still_returns_the_recovered_candidate() {
    let root = test_root("normalization-failure");
    let path = store_path(&root);
    write_json(
        &NotificationStore::backup_path_for_test(&path),
        &sample_file(11),
    );
    let failing =
        NotificationStore::with_path_and_failures(path.clone(), vec![FailurePoint::PromoteTemp]);

    let outcome = failing.load().unwrap();

    assert_eq!(outcome.file.revision, 11);
    assert_eq!(
        outcome.status,
        NotificationLoadStatus::Recovered {
            source: RecoverySource::Backup
        }
    );
    assert_eq!(read_file(&path).revision, 11);
    cleanup(&root);
}

#[test]
fn content_map_round_trip_is_not_reordered_or_dropped() {
    let root = test_root("content-map");
    let store = NotificationStore::with_path(store_path(&root));
    let mut file = sample_file(5);
    file.partitions[0].items[0].content.params = BTreeMap::from([(
        "orderId".into(),
        NotificationScalar::String("order-42".into()),
    )]);

    store.save(&file).unwrap();

    assert_eq!(store.load().unwrap().file, file);
    cleanup(&root);
}
