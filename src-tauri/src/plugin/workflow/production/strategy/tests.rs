use super::*;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

const SCOPE: &str = "fixture-scope";
const ID: &str = "a986c8d7-2a0c-4c31-a52b-379c508d8130";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/strategy-production-tests");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> OrderSubmissionStore {
        OrderSubmissionStore::with_dir(self.0.clone())
    }
    fn record_path(&self) -> PathBuf {
        let scope = std::fs::read_dir(&self.0)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::read_dir(scope)
            .unwrap()
            .find_map(|entry| {
                let path = entry.unwrap().path();
                (path.extension().and_then(|s| s.to_str()) == Some("json")).then_some(path)
            })
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/strategy-production-tests");
        if self.0.parent() == Some(root.as_path())
            && uuid::Uuid::parse_str(self.0.file_name().unwrap().to_str().unwrap()).is_ok()
        {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
fn proposal() -> PlaceProposal {
    PlaceProposal {
        symbol: "BTCUSDT".into(),
        side: "Buy".into(),
        order_type: "Limit".into(),
        qty: "0.01".into(),
        price: Some("50000".into()),
        time_in_force: "GTC".into(),
        position_idx: 1,
        reduce_only: false,
    }
}
fn request() -> PlaceOrderRequest {
    PlaceOrderRequest {
        symbol: "BTCUSDT".into(),
        side: "Buy".into(),
        order_type: "Limit".into(),
        qty: "0.01".into(),
        price: Some("50000".into()),
        time_in_force: Some("GoodTillCancel".into()),
        position_idx: 1,
        reduce_only: Some(false),
        order_link_id: Some(ID.into()),
    }
}
fn order() -> Order {
    Order {
        order_id: "exchange-original".into(),
        symbol: "BTCUSDT".into(),
        side: "Buy".into(),
        order_type: "Limit".into(),
        price: "50000".into(),
        qty: "0.01".into(),
        status: OrderStatus::New,
        order_link_id: Some(ID.into()),
        filled_qty: "0".into(),
        avg_price: "0".into(),
    }
}
fn payload() -> Value {
    json!({"data":[{"order_id":"exchange-original","order_link_id":ID,"symbol":"BTCUSDT",
        "side":"Buy","order_type":"Limit","qty":"0.010","price":"50000.0","order_status":"New",
        "cum_exec_qty":"0","avg_price":"0","time_in_force":"GoodTillCancel","position_idx":1,"reduce_only":false}]})
}
fn no_query(_: &'static str, _: Vec<(String, String)>) -> std::future::Ready<AppResult<Value>> {
    panic!("persisted acceptance must not contact exchange")
}
fn pending(f: &Fixture) {
    f.store().begin(SCOPE, &request(), 1).unwrap();
}
fn accepted(f: &Fixture) {
    pending(f);
    f.store()
        .finish(SCOPE, ID, SubmissionOutcome::Accepted(Box::new(order())))
        .unwrap();
}

// Removing durable acknowledgement or unnecessarily querying a known receipt breaks this test.
#[tokio::test]
async fn durable_accepted_journal_acknowledges_without_query_and_is_idempotent() {
    let f = Fixture::new();
    accepted(&f);
    for _ in 0..2 {
        acknowledge(
            &f.store(),
            SCOPE,
            SCOPE,
            ID,
            &order(),
            &proposal(),
            no_query,
        )
        .await
        .unwrap();
    }
    assert!(f.store().get(SCOPE, ID).unwrap().unwrap().acknowledged);
    assert!(f.store().list_pending(SCOPE).unwrap().is_empty());
}

// Accepting the HTTP receipt without first recovering its original pending journal breaks this.
#[tokio::test]
async fn accepted_http_pending_journal_queries_original_then_persists_before_ack() {
    let f = Fixture::new();
    pending(&f);
    let calls = AtomicUsize::new(0);
    acknowledge(
        &f.store(),
        SCOPE,
        SCOPE,
        ID,
        &order(),
        &proposal(),
        |endpoint, params| {
            assert_eq!(endpoint, endpoints::OPEN_ORDERS);
            assert!(params.contains(&("order_link_id".into(), ID.into())));
            assert!(params.contains(&("symbol".into(), "BTCUSDT".into())));
            assert!(!f.store().get(SCOPE, ID).unwrap().unwrap().acknowledged);
            calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(payload()))
        },
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let record = f.store().get(SCOPE, ID).unwrap().unwrap();
    assert!(record.acknowledged);
    assert!(matches!(record.outcome, SubmissionOutcome::Accepted(_)));
}

// A wrong scope, original request, or exchange receipt must never release the pending blocker.
#[tokio::test]
async fn mismatched_scope_request_or_expected_identity_never_acknowledges() {
    for field in [
        "scope", "id", "symbol", "qty", "price", "side", "type", "tif", "idx", "reduce",
        "exchange", "link",
    ] {
        let f = Fixture::new();
        accepted(&f);
        let mut p = proposal();
        let mut o = order();
        let current = if field == "scope" {
            "different-scope"
        } else {
            SCOPE
        };
        let id = if field == "id" { "other" } else { ID };
        match field {
            "symbol" => p.symbol = "ETHUSDT".into(),
            "qty" => p.qty = "1".into(),
            "price" => p.price = Some("1".into()),
            "side" => p.side = "Sell".into(),
            "type" => p.order_type = "Market".into(),
            "tif" => p.time_in_force = "IOC".into(),
            "idx" => p.position_idx = 2,
            "reduce" => p.reduce_only = true,
            "exchange" => o.order_id = "wrong".into(),
            "link" => o.order_link_id = None,
            _ => {}
        }
        assert!(
            acknowledge(&f.store(), current, SCOPE, id, &o, &p, no_query)
                .await
                .is_err(),
            "{field}"
        );
        assert!(!f.store().get(SCOPE, ID).unwrap().unwrap().acknowledged);
    }
}

// Wrong and ambiguous exact-query results must not be converted into an accepted journal.
#[tokio::test]
async fn missing_malformed_wrong_and_duplicate_records_preserve_pending_intent() {
    for field in [
        "empty",
        "missing-list",
        "duplicate",
        "symbol",
        "link",
        "qty",
        "price",
        "side",
        "type",
        "tif",
        "idx",
        "reduce",
        "filled",
        "status",
    ] {
        let f = Fixture::new();
        pending(&f);
        let mut data = payload();
        match field {
            "empty" => data = json!({"data":[]}),
            "missing-list" => data = json!({}),
            "duplicate" => {
                let copy = data["data"][0].clone();
                data["data"].as_array_mut().unwrap().push(copy);
            }
            "symbol" => data["data"][0]["symbol"] = "ETHUSDT".into(),
            "link" => data["data"][0]["order_link_id"] = "wrong".into(),
            "qty" => data["data"][0]["qty"] = "1".into(),
            "price" => data["data"][0]["price"] = "1".into(),
            "side" => data["data"][0]["side"] = "Sell".into(),
            "type" => data["data"][0]["order_type"] = "Market".into(),
            "tif" => data["data"][0]["time_in_force"] = "ImmediateOrCancel".into(),
            "idx" => data["data"][0]["position_idx"] = 2.into(),
            "reduce" => data["data"][0]["reduce_only"] = true.into(),
            "filled" => {
                data["data"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("cum_exec_qty");
            }
            "status" => {
                data["data"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("order_status");
            }
            _ => unreachable!(),
        }
        let result = reconcile(
            &f.store(),
            SCOPE,
            SCOPE,
            "BTCUSDT",
            Some(ID),
            None,
            |_, _| std::future::ready(Ok(data.clone())),
        )
        .await;
        if field == "empty" {
            assert!(result.unwrap().is_none());
        } else {
            assert!(result.is_err(), "{field}");
        }
        assert!(matches!(
            f.store().get(SCOPE, ID).unwrap().unwrap().outcome,
            SubmissionOutcome::Pending
        ));
    }
}

// Skipping history after an empty open-order result loses terminal recovery.
#[tokio::test]
async fn cancel_recovery_queries_exact_exchange_identity_and_preserves_unknown_status() {
    for status in ["Filled", "Cancelled", "Rejected", "FutureUnknownStatus"] {
        let f = Fixture::new();
        let calls = AtomicUsize::new(0);
        let got = reconcile(
            &f.store(),
            SCOPE,
            SCOPE,
            "BTCUSDT",
            None,
            Some("exchange-original"),
            |endpoint, params| {
                assert!(params.contains(&("order_id".into(), "exchange-original".into())));
                assert!(!params.iter().any(|(key, _)| key == "order_link_id"));
                let call = calls.fetch_add(1, Ordering::SeqCst);
                let mut data = payload();
                data["data"][0]["order_status"] = status.into();
                if call == 0 {
                    assert_eq!(endpoint, endpoints::OPEN_ORDERS);
                    std::future::ready(Ok(json!({"data":[]})))
                } else {
                    assert_eq!(endpoint, endpoints::ORDERS);
                    std::future::ready(Ok(data))
                }
            },
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(got.order_id, "exchange-original");
        assert_eq!(
            got.status,
            match status {
                "Filled" => OrderStatus::Filled,
                "Cancelled" => OrderStatus::Cancelled,
                "Rejected" => OrderStatus::Rejected,
                _ => OrderStatus::Unknown,
            }
        );
    }
}

// Swallowing an acknowledgement write failure would silently release the journal barrier.
#[tokio::test]
async fn failed_acknowledgement_leaves_accepted_journal_recoverable() {
    let f = Fixture::new();
    accepted(&f);
    std::fs::create_dir(PathBuf::from(format!("{}.tmp", f.record_path().display()))).unwrap();
    assert!(acknowledge(
        &f.store(),
        SCOPE,
        SCOPE,
        ID,
        &order(),
        &proposal(),
        no_query
    )
    .await
    .is_err());
    assert!(!f.store().get(SCOPE, ID).unwrap().unwrap().acknowledged);
    assert_eq!(f.store().list_pending(SCOPE).unwrap().len(), 1);
}

// First-alias-wins parsing must not conceal a contradictory exchange identity/status.
#[tokio::test]
async fn contradictory_aliases_do_not_establish_authoritative_identity_or_terminal_state() {
    for (key, value) in [
        ("orderId", json!("wrong")),
        ("orderLinkId", json!("wrong")),
        ("status", json!("Filled")),
        ("qty", json!("1")),
        ("reduceOnly", json!(true)),
    ] {
        let f = Fixture::new();
        pending(&f);
        let mut data = payload();
        if key == "qty" {
            data["data"][0]["quantity"] = value;
        } else {
            data["data"][0][key] = value;
        }
        assert!(
            reconcile(
                &f.store(),
                SCOPE,
                SCOPE,
                "BTCUSDT",
                Some(ID),
                None,
                |_, _| std::future::ready(Ok(data.clone()))
            )
            .await
            .is_err(),
            "{key}"
        );
        assert!(matches!(
            f.store().get(SCOPE, ID).unwrap().unwrap().outcome,
            SubmissionOutcome::Pending
        ));
    }
}

// The actual submit_once success-with-failed-finish path must remain recoverable.
#[tokio::test]
async fn failed_submit_finish_requires_query_and_retryable_local_ack_without_resubmitting() {
    let f = Fixture::new();
    let store = f.store();
    let accepted =
        crate::services::order_submission::submit_once(&store, SCOPE, request(), 1, |_| async {
            std::fs::create_dir(PathBuf::from(format!("{}.tmp", f.record_path().display())))
                .unwrap();
            Ok(order())
        })
        .await
        .unwrap();
    assert!(matches!(
        store.get(SCOPE, ID).unwrap().unwrap().outcome,
        SubmissionOutcome::Pending
    ));
    assert!(
        acknowledge(&store, SCOPE, SCOPE, ID, &accepted, &proposal(), |_, _| {
            std::future::ready(Ok(payload()))
        })
        .await
        .is_err()
    );
    assert!(!store.get(SCOPE, ID).unwrap().unwrap().acknowledged);
    let obstruction = PathBuf::from(format!("{}.tmp", f.record_path().display()));
    assert!(obstruction.starts_with(&f.0));
    std::fs::remove_dir(obstruction).unwrap();
    acknowledge(&store, SCOPE, SCOPE, ID, &accepted, &proposal(), |_, _| {
        std::future::ready(Ok(payload()))
    })
    .await
    .unwrap();
    assert!(f.store().list_pending(SCOPE).unwrap().is_empty());
}

#[tokio::test]
async fn pending_ack_wrong_exchange_id_does_not_poison_journal() {
    let f = Fixture::new();
    pending(&f);
    let mut data = payload();
    data["data"][0]["order_id"] = "different-exchange".into();
    assert!(acknowledge(
        &f.store(),
        SCOPE,
        SCOPE,
        ID,
        &order(),
        &proposal(),
        |_, _| std::future::ready(Ok(data.clone()))
    )
    .await
    .is_err());
    assert!(matches!(
        f.store().get(SCOPE, ID).unwrap().unwrap().outcome,
        SubmissionOutcome::Pending
    ));
}

#[tokio::test]
async fn exact_authoritative_rejection_is_durable_and_never_acknowledged() {
    let f = Fixture::new();
    pending(&f);
    let mut data = payload();
    data["data"][0]["order_status"] = "Rejected".into();
    for _ in 0..2 {
        assert!(matches!(
            reconcile(
                &f.store(),
                SCOPE,
                SCOPE,
                "BTCUSDT",
                Some(ID),
                None,
                |_, _| std::future::ready(Ok(data.clone()))
            )
            .await,
            Err(crate::error::AppError::OrderSubmissionRejected(_))
        ));
    }
    let record = f.store().get(SCOPE, ID).unwrap().unwrap();
    assert!(matches!(record.outcome, SubmissionOutcome::Rejected));
    assert!(!record.acknowledged);
}
