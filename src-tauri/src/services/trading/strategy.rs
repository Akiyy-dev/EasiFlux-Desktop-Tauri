//! Strategy-only admission at the mutation API hand-off, never a wire-send promise.
use super::*;
use std::future::Future;

pub(crate) type StrategyAdmission<'a> = dyn Fn() -> AppResult<()> + Send + Sync + 'a;

pub(super) async fn dispatch<T, F, Fut>(
    admission: Option<&StrategyAdmission<'_>>,
    submit: F,
) -> AppResult<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = AppResult<T>>,
{
    if let Some(admission) = admission {
        admission()?;
    }
    // No intervening await: successful check hands ownership to this mutation
    // future. Revocation afterward does not cancel or recheck an admitted call.
    submit().await
}

pub(super) async fn dispatch_reserved<F, Fut>(
    risk: &Arc<tokio::sync::RwLock<RiskService>>,
    reservation: &RiskReservation,
    now_ms: u64,
    admission: Option<&StrategyAdmission<'_>>,
    submit: F,
) -> AppResult<Order>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = AppResult<Order>>,
{
    if let Some(admission) = admission {
        if let Err(rejection) = admission() {
            if risk
                .read()
                .await
                .release_reservation(reservation, now_ms)
                .is_err()
            {
                // No mutation occurred even if local quota repair fails. The
                // remaining reservation stays conservative and visibly blocks.
                tracing::warn!(
                    code = "STRATEGY_UNSENT_RISK_RELEASE_FAILED",
                    "unsubmitted strategy risk reservation could not be released"
                );
            }
            return Err(rejection);
        }
    }
    submit().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{models::config::RiskConfig, storage::RiskUsageStore};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/strategy-tests")
                .join(uuid::Uuid::new_v4().to_string());
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn denied() -> AppError {
        AppError::OrderSubmissionRejected("strategy admission revoked".into())
    }
    fn request() -> PlaceOrderRequest {
        PlaceOrderRequest {
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Market".into(),
            qty: "1".into(),
            position_idx: 1,
            price: None,
            time_in_force: Some("ImmediateOrCancel".into()),
            order_link_id: Some(uuid::Uuid::new_v4().to_string()),
            reduce_only: Some(false),
        }
    }
    fn order() -> Order {
        Order {
            order_id: "synthetic".into(),
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Market".into(),
            price: "0".into(),
            qty: "1".into(),
            status: OrderStatus::New,
            order_link_id: None,
            filled_qty: "0".into(),
            avg_price: "0".into(),
        }
    }
    #[tokio::test]
    async fn blocked_real_risk_preparation_checks_strategy_guard_but_preserves_legacy_path() {
        for guarded in [true, false] {
            let temp = Temp::new();
            let path = temp.0.join("risk.toml");
            let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
                RiskConfig {
                    max_daily_orders: 1,
                    ..Default::default()
                },
                RiskUsageStore::with_path(path.clone()),
            )));
            let observer =
                OrderNotificationObserver::new(Arc::new(NotificationRuntime::Unavailable(
                    crate::services::notification::NotificationAvailability::new(
                        "TEST_ONLY",
                        "synthetic observer disabled",
                    ),
                )));
            let context = SubmissionContext {
                submission_id: uuid::Uuid::new_v4().to_string(),
                account_id: "synthetic-account".into(),
                session_epoch: 1,
            };
            let allowed = AtomicBool::new(true);
            let calls = AtomicUsize::new(0);
            let effects = AtomicUsize::new(0);
            let admission = || {
                if allowed.load(Ordering::SeqCst) {
                    Ok(())
                } else {
                    Err(denied())
                }
            };
            let blocked_risk = risk.write().await;
            let mut pending = Box::pin(execute_place_order_with_submit(
                &risk,
                &observer,
                &context,
                request(),
                None,
                1000,
                guarded.then_some(&admission as &StrategyAdmission<'_>),
                |_| async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(order())
                },
                |_| async {
                    effects.fetch_add(1, Ordering::SeqCst);
                },
            ));
            // Prove the actual preparation pipeline has started and is blocked
            // on risk configuration, before revoking its native admission.
            std::future::poll_fn(|cx| {
                assert!(pending.as_mut().poll(cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            allowed.store(false, Ordering::SeqCst);
            drop(blocked_risk);
            let result = pending.await;
            assert_eq!(result.is_err(), guarded);
            if guarded {
                assert!(matches!(result, Err(AppError::OrderSubmissionRejected(_))));
            }
            assert_eq!(calls.load(Ordering::SeqCst), usize::from(!guarded));
            assert_eq!(effects.load(Ordering::SeqCst), usize::from(!guarded));
            assert_eq!(
                RiskUsageStore::with_path(path)
                    .load()
                    .unwrap()
                    .unwrap()
                    .occupied_orders,
                u32::from(!guarded),
            );
        }
    }
    #[tokio::test]
    async fn revoked_handoff_releases_real_unsubmitted_risk_reservation() {
        let temp = Temp::new();
        let path = temp.0.join("risk.toml");
        let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
            RiskConfig {
                max_daily_orders: 1,
                ..Default::default()
            },
            RiskUsageStore::with_path(path.clone()),
        )));
        let request = request();
        let reservation = risk
            .read()
            .await
            .reserve_order(&request, None, 1000)
            .unwrap();
        let calls = AtomicUsize::new(0);
        let result = dispatch_reserved(
            &risk,
            &reservation,
            1000,
            Some(&|| Err(denied())),
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(order())
            },
        )
        .await;
        let usage = RiskUsageStore::with_path(path)
            .load()
            .unwrap()
            .unwrap()
            .occupied_orders;
        assert_eq!(
            (calls.load(Ordering::SeqCst), result.is_err(), usage),
            (0, true, 0)
        );
        assert!(risk
            .read()
            .await
            .reserve_order(&request, None, 1000)
            .is_ok());
    }
    #[tokio::test]
    async fn cancellation_handoff_checks_revocation_after_preparation() {
        let allowed = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(AtomicUsize::new(0));
        let (release, wait) = tokio::sync::oneshot::channel();
        let task = {
            let allowed = allowed.clone();
            let calls = calls.clone();
            tokio::spawn(async move {
                wait.await.unwrap();
                dispatch(
                    Some(&|| {
                        if allowed.load(Ordering::SeqCst) {
                            Ok(())
                        } else {
                            Err(denied())
                        }
                    }),
                    || async {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(order())
                    },
                )
                .await
            })
        };
        allowed.store(false, Ordering::SeqCst);
        release.send(()).unwrap();
        assert!(task.await.unwrap().is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn admitted_future_drains_after_revocation_without_rechecking_or_aborting() {
        let allowed = Arc::new(AtomicBool::new(true));
        let checks = Arc::new(AtomicUsize::new(0));
        let (entered, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = tokio::sync::oneshot::channel();
        let task = {
            let allowed = allowed.clone();
            let checks = checks.clone();
            tokio::spawn(async move {
                dispatch(
                    Some(&|| {
                        checks.fetch_add(1, Ordering::SeqCst);
                        if allowed.load(Ordering::SeqCst) {
                            Ok(())
                        } else {
                            Err(denied())
                        }
                    }),
                    || async {
                        entered.send(()).unwrap();
                        wait.await.unwrap();
                        Ok(order())
                    },
                )
                .await
            })
        };
        ready.await.unwrap();
        allowed.store(false, Ordering::SeqCst);
        release.send(()).unwrap();
        assert_eq!(task.await.unwrap().unwrap().order_id, "synthetic");
        assert_eq!(checks.load(Ordering::SeqCst), 1);
    }
}
