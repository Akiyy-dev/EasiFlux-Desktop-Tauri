//! A submission whose response was lost must be queried, never sent again.
use crate::error::{AppError, AppResult};
use crate::models::order_submission::SubmissionOutcome;
use crate::models::trading::{Order, PlaceOrderRequest};
use crate::storage::order_submissions::OrderSubmissionStore;

pub(crate) async fn submit_once<F, Fut>(
    store: &OrderSubmissionStore,
    scope: &str,
    request: PlaceOrderRequest,
    now_ms: u64,
    submit: F,
) -> AppResult<Order>
where
    F: FnOnce(PlaceOrderRequest) -> Fut,
    Fut: std::future::Future<Output = AppResult<Order>>,
{
    let id = request
        .order_link_id
        .as_deref()
        .filter(|id| !id.is_empty() && id.len() <= 36)
        .ok_or_else(|| AppError::OrderSubmissionRejected("订单提交标识无效".into()))?
        .to_owned();
    let unknown = || AppError::OrderSubmissionUnknown {
        order_link_id: id.clone(),
    };
    let before_submission =
        |error: AppError| AppError::OrderSubmissionRejected(error.user_message());
    if let Some(record) = store.get(scope, &id).map_err(before_submission)? {
        if serde_json::to_value(&record.request).ok() != serde_json::to_value(&request).ok() {
            return Err(AppError::OrderSubmissionRejected(
                "同一提交标识不能用于不同订单".into(),
            ));
        }
        return match record.outcome {
            SubmissionOutcome::Accepted(order) => Ok(*order),
            SubmissionOutcome::Rejected => Err(AppError::OrderSubmissionRejected(
                "原订单已确认未受理，请重新填写后提交".into(),
            )),
            SubmissionOutcome::Pending => Err(unknown()),
        };
    }
    let pending = store.list_pending(scope).map_err(before_submission)?;
    if let Some(blocker) = pending
        .iter()
        .find(|pending| request.reduce_only != Some(true) || pending.reduce_only == Some(true))
    {
        return Err(AppError::OrderSubmissionBlocked {
            order_link_id: blocker.order_link_id.clone(),
        });
    }
    let original = store
        .begin(scope, &request, now_ms)
        .map_err(before_submission)?;
    match submit(request).await {
        Ok(mut order) => {
            if order.order_id.trim().is_empty()
                || order.order_link_id.as_ref().is_some_and(|link| link != &id)
            {
                return Err(unknown());
            }
            order.order_link_id = Some(id.clone());
            // Some successful responses contain only the exchange order ID.
            // Restore request metadata from the durable intent, without inventing
            // an execution status, fill quantity, or average fill price.
            if order.symbol.is_empty() {
                order.symbol = original.request.symbol.clone();
            }
            if order.side.is_empty() {
                order.side = original.request.side.clone();
            }
            if order.order_type.is_empty() {
                order.order_type = original.request.order_type.clone();
            }
            if order.qty.is_empty()
                || order.qty.parse::<rust_decimal::Decimal>().ok()
                    == Some(rust_decimal::Decimal::ZERO)
            {
                order.qty = original.request.qty.clone();
            }
            if original.request.order_type.eq_ignore_ascii_case("limit")
                && (order.price.is_empty()
                    || order.price.parse::<rust_decimal::Decimal>().ok()
                        == Some(rust_decimal::Decimal::ZERO))
            {
                if let Some(price) = &original.request.price {
                    order.price = price.clone();
                }
            }
            if store
                .finish(
                    scope,
                    &id,
                    SubmissionOutcome::Accepted(Box::new(order.clone())),
                )
                .is_err()
            {
                // Acceptance is known; preserve the pending intent for a later query.
                tracing::warn!(
                    "confirmed order receipt could not be persisted; original intent retained"
                );
            }
            Ok(order)
        }
        Err(error) if definitely_not_accepted(&error) => {
            store
                .finish(scope, &id, SubmissionOutcome::Rejected)
                .map_err(|_| unknown())?;
            if matches!(error, AppError::Notified { .. }) {
                Err(error)
            } else {
                Err(AppError::OrderSubmissionRejected(error.user_message()))
            }
        }
        Err(_) => Err(unknown()),
    }
}

fn definitely_not_accepted(error: &AppError) -> bool {
    super::trading::is_certain_submission_failure(error)
        || matches!(
            error,
            AppError::Risk(_)
                | AppError::Auth(_)
                | AppError::NotConnected
                | AppError::Config(_)
                | AppError::OrderSubmissionRejected(_)
        )
        || matches!(
            error,
            AppError::Notified {
                code: "RISK_ORDER_BLOCKED" | "ORDER_REJECTED",
                ..
            }
        )
}

pub(crate) async fn reconcile<F, Fut>(
    store: &OrderSubmissionStore,
    scope: &str,
    order_link_id: &str,
    query: F,
) -> AppResult<Option<Order>>
where
    F: FnOnce(PlaceOrderRequest) -> Fut,
    Fut: std::future::Future<Output = AppResult<Vec<Order>>>,
{
    let Some(record) = store.get(scope, order_link_id)? else {
        return Ok(None);
    };
    match record.outcome {
        SubmissionOutcome::Accepted(order) => return Ok(Some(*order)),
        SubmissionOutcome::Rejected => {
            return Err(AppError::OrderSubmissionRejected(
                "原订单已确认未受理，可重新提交".into(),
            ))
        }
        SubmissionOutcome::Pending => {}
    }
    let symbol = record.request.symbol.clone();
    let orders = query(record.request).await?;
    let found = orders.into_iter().find(|order| {
        order.order_link_id.as_deref() == Some(order_link_id)
            && order.symbol == symbol
            && !order.order_id.trim().is_empty()
    });
    if let Some(order) = &found {
        store.finish(
            scope,
            order_link_id,
            SubmissionOutcome::Accepted(Box::new(order.clone())),
        )?;
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trading::OrderStatus;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SCOPE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(
                std::env::temp_dir().join(format!("easiflux-submit-once-{}", uuid::Uuid::new_v4())),
            )
        }
        fn store(&self) -> OrderSubmissionStore {
            OrderSubmissionStore::with_dir(self.0.clone())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn request(id: &str, reduce_only: bool) -> PlaceOrderRequest {
        PlaceOrderRequest {
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Market".into(),
            qty: "1".into(),
            position_idx: 0,
            price: None,
            time_in_force: None,
            order_link_id: Some(id.into()),
            reduce_only: Some(reduce_only),
        }
    }
    fn accepted(id: &str) -> Order {
        Order {
            order_id: "order-1".into(),
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Market".into(),
            price: "0".into(),
            qty: "1".into(),
            status: OrderStatus::New,
            order_link_id: Some(id.into()),
            filled_qty: "0".into(),
            avg_price: "0".into(),
        }
    }

    #[tokio::test]
    async fn lost_response_is_durable_and_retry_never_sends_a_second_order() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let calls = AtomicUsize::new(0);
        let first = submit_once(&store, SCOPE, request("original", false), 10, |_| async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(AppError::Connection("lost response".into()))
        })
        .await;
        assert!(first.is_err());
        let restarted = fixture.store();
        let retry = submit_once(
            &restarted,
            SCOPE,
            request("original", false),
            20,
            |_| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(accepted("original"))
            },
        )
        .await;
        assert!(
            retry.is_err(),
            "unknown original must be queried instead of resubmitted"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(restarted.list_pending(SCOPE).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn pending_open_blocks_new_id_but_permits_one_emergency_reduction() {
        let fixture = Fixture::new();
        let store = fixture.store();
        store
            .begin(SCOPE, &request("unknown-open", false), 10)
            .unwrap();
        let calls = AtomicUsize::new(0);
        let result = submit_once(&store, SCOPE, request("new-open", false), 20, |_| async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(accepted("new-open"))
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(submit_once(
            &store,
            SCOPE,
            request("emergency-close", true),
            20,
            |_| async { Ok(accepted("emergency-close")) }
        )
        .await
        .is_ok());
        store
            .begin(SCOPE, &request("unknown-close", true), 30)
            .unwrap();
        assert!(submit_once(
            &store,
            SCOPE,
            request("another-close", true),
            40,
            |_| async { panic!("must not duplicate a reduction with unknown outcome") }
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn accepted_receipt_survives_restart_and_is_returned_without_resending() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let result = submit_once(&store, SCOPE, request("accepted", false), 10, |_| async {
            Ok(accepted("accepted"))
        })
        .await
        .unwrap();
        let replay = submit_once(
            &fixture.store(),
            SCOPE,
            request("accepted", false),
            20,
            |_| async { panic!("confirmed request must not be sent twice") },
        )
        .await
        .unwrap();
        assert_eq!(result.order_id, replay.order_id);
        assert_eq!(fixture.store().list_pending(SCOPE).unwrap().len(), 1);
        fixture.store().acknowledge(SCOPE, "accepted").unwrap();
        assert!(store.list_pending(SCOPE).unwrap().is_empty());
    }

    #[tokio::test]
    async fn known_risk_rejection_does_not_leave_unknown_submission() {
        let fixture = Fixture::new();
        let store = fixture.store();
        assert!(submit_once(
            &store,
            SCOPE,
            request("risk-blocked", false),
            10,
            |_| async { Err(AppError::Risk("quantity too large".into())) }
        )
        .await
        .is_err());
        assert!(store.list_pending(SCOPE).unwrap().is_empty());
        assert!(matches!(
            store.get(SCOPE, "risk-blocked").unwrap().unwrap().outcome,
            SubmissionOutcome::Rejected
        ));
    }

    #[tokio::test]
    async fn unusable_journal_never_calls_the_trading_api() {
        let fixture = Fixture::new();
        std::fs::write(&fixture.0, b"not a directory").unwrap();
        let result = submit_once(
            &fixture.store(),
            SCOPE,
            request("blocked", false),
            10,
            |_| async { panic!("must persist intent before contacting exchange") },
        )
        .await;
        assert!(result.is_err());
        std::fs::remove_file(&fixture.0).unwrap();
    }

    #[tokio::test]
    async fn reconciliation_requires_exact_original_identity_and_survives_restart() {
        let fixture = Fixture::new();
        let store = fixture.store();
        store.begin(SCOPE, &request("original", false), 10).unwrap();
        let wrong = reconcile(&store, SCOPE, "original", |_| async {
            Ok(vec![accepted("unrelated")])
        })
        .await
        .unwrap();
        assert!(wrong.is_none());
        assert_eq!(store.list_pending(SCOPE).unwrap().len(), 1);
        let found = reconcile(&store, SCOPE, "original", |original| async move {
            assert_eq!(original.order_link_id.as_deref(), Some("original"));
            Ok(vec![accepted("original")])
        })
        .await
        .unwrap();
        assert_eq!(found.unwrap().order_id, "order-1");
        assert_eq!(fixture.store().list_pending(SCOPE).unwrap().len(), 1);
        let cached = reconcile(&fixture.store(), SCOPE, "original", |_| async {
            panic!("known receipt requires no network request")
        })
        .await
        .unwrap();
        assert_eq!(cached.unwrap().order_id, "order-1");
        fixture.store().acknowledge(SCOPE, "original").unwrap();
        assert!(fixture.store().list_pending(SCOPE).unwrap().is_empty());
    }

    #[tokio::test]
    async fn lost_success_reply_stays_blocked_after_restart_until_receipt_acknowledged() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let _lost_reply = submit_once(
            &store,
            SCOPE,
            request("accepted-original", false),
            10,
            |_| async { Ok(accepted("accepted-original")) },
        )
        .await
        .unwrap();
        let restarted = fixture.store();
        let restored = restarted.list_pending(SCOPE).unwrap();
        assert_eq!(restored[0].order_link_id, "accepted-original");
        assert!(matches!(
            submit_once(
                &restarted,
                SCOPE,
                request("new-intent", false),
                20,
                |_| async {
                    panic!("unacknowledged success must block duplicate intent after restart")
                }
            )
            .await,
            Err(AppError::OrderSubmissionBlocked { .. })
        ));
        let receipt = reconcile(&restarted, SCOPE, "accepted-original", |_| async {
            panic!("accepted receipt is already durable")
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(receipt.order_link_id.as_deref(), Some("accepted-original"));
        assert_eq!(restarted.list_pending(SCOPE).unwrap().len(), 1);
        restarted.acknowledge(SCOPE, "accepted-original").unwrap();
        assert!(fixture.store().list_pending(SCOPE).unwrap().is_empty());
        assert!(submit_once(
            &fixture.store(),
            SCOPE,
            request("new-intent", false),
            20,
            |_| async { Ok(accepted("new-intent")) }
        )
        .await
        .is_ok());
    }

    #[tokio::test]
    async fn minimal_exchange_receipt_is_completed_from_the_durable_original_request() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let mut original = request("minimal-receipt", false);
        original.order_type = "Limit".into();
        original.price = Some("100".into());
        let receipt = submit_once(&store, SCOPE, original, 10, |_| async {
            Ok(crate::api::mapper::parse_order(
                &serde_json::json!({"order_id": "order-minimal"}),
            ))
        })
        .await
        .unwrap();
        assert_eq!(receipt.symbol, "BTCUSDT");
        assert_eq!(receipt.side, "Buy");
        assert_eq!(receipt.order_type, "Limit");
        assert_eq!(receipt.qty, "1");
        assert_eq!(receipt.price, "100");
        assert_eq!(receipt.order_link_id.as_deref(), Some("minimal-receipt"));
        let cached = reconcile(&fixture.store(), SCOPE, "minimal-receipt", |_| async {
            panic!("accepted minimal receipt must be recoverable without sending again")
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(cached.symbol, receipt.symbol);
        assert_eq!(cached.qty, receipt.qty);
        fixture
            .store()
            .acknowledge(SCOPE, "minimal-receipt")
            .unwrap();
        assert!(fixture.store().list_pending(SCOPE).unwrap().is_empty());
    }

    #[tokio::test]
    async fn empty_or_failed_lookup_keeps_original_pending() {
        let fixture = Fixture::new();
        let store = fixture.store();
        store.begin(SCOPE, &request("original", false), 10).unwrap();
        assert!(
            reconcile(&store, SCOPE, "original", |_| async { Ok(vec![]) })
                .await
                .unwrap()
                .is_none()
        );
        assert!(reconcile(&store, SCOPE, "original", |_| async {
            Err(AppError::Connection("offline".into()))
        })
        .await
        .is_err());
        assert_eq!(store.list_pending(SCOPE).unwrap().len(), 1);
    }
}
