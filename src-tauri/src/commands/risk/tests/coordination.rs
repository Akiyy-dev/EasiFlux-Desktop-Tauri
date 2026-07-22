use super::super::*;
use super::support::*;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::commands::market::persist_market_config_update;
use crate::error::AppError;
use crate::models::trading::{Order, PlaceOrderRequest};
use crate::services::trading::execute_coordinated_reserved_order;
use crate::services::AccountLifecycleCoordinator;
use crate::storage::RiskUsageStore;

fn market_order() -> PlaceOrderRequest {
    PlaceOrderRequest {
        symbol: "BTCUSDT".into(),
        side: "Buy".into(),
        order_type: "Market".into(),
        qty: "1".into(),
        position_idx: 0,
        price: None,
        time_in_force: None,
        order_link_id: None,
        reduce_only: None,
    }
}

#[tokio::test]
async fn concurrent_market_and_risk_updates_preserve_both_fields() {
    let root = test_path("concurrent-market-risk");
    std::fs::create_dir_all(&root).unwrap();
    let store = ConfigStore::with_path(root.join("config.toml"));
    let ledger = root.join("risk_usage.toml");
    let (config, risk) = runtime(AppConfig::default(), &ledger);
    let coordinator = AccountLifecycleCoordinator::new();

    let held_guard = coordinator.mutation_guard().await;
    let risk_update = execute_risk_config_update(
        RiskUpdateContext {
            coordinator: &coordinator,
            config_store: &store,
            runtime_config: &config,
            risk: &risk,
        },
        request("UTC"),
        || NOW_MS,
        || async {},
    );
    let market_update = persist_market_config_update(
        &coordinator,
        &store,
        &config,
        |next| next.active_symbol = "ETHUSDT".into(),
        || async {},
    );

    tokio::pin!(risk_update);
    tokio::pin!(market_update);
    assert!(matches!(
        futures_util::poll!(&mut risk_update),
        std::task::Poll::Pending
    ));
    assert!(matches!(
        futures_util::poll!(&mut market_update),
        std::task::Poll::Pending
    ));
    drop(held_guard);

    let (risk_result, market_result) = tokio::join!(risk_update, market_update);
    risk_result.unwrap();
    market_result.unwrap();

    let runtime = config.read().await.clone();
    let persisted = store.load().unwrap();
    assert_eq!(runtime.active_symbol, "ETHUSDT");
    assert_eq!(runtime.trading_day_timezone, "UTC");
    assert_eq!(persisted.active_symbol, runtime.active_symbol);
    assert_eq!(persisted.trading_day_timezone, runtime.trading_day_timezone);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn returned_status_samples_time_after_entering_commit_guard() {
    let root = test_path("commit-time");
    std::fs::create_dir_all(&root).unwrap();
    let store = ConfigStore::with_path(root.join("config.toml"));
    let ledger = root.join("risk_usage.toml");
    let (config, risk) = runtime(AppConfig::default(), &ledger);
    let coordinator = AccountLifecycleCoordinator::new();
    let clock = AtomicU64::new(NOW_MS);
    let commit_now_ms = NOW_MS + 86_400_000;

    let held_guard = coordinator.mutation_guard().await;
    let update = execute_risk_config_update(
        RiskUpdateContext {
            coordinator: &coordinator,
            config_store: &store,
            runtime_config: &config,
            risk: &risk,
        },
        request("UTC"),
        || clock.load(Ordering::SeqCst),
        || async {},
    );
    tokio::pin!(update);
    assert!(matches!(
        futures_util::poll!(&mut update),
        std::task::Poll::Pending
    ));

    clock.store(commit_now_ms, Ordering::SeqCst);
    drop(held_guard);
    let status = update.await.unwrap();

    assert_eq!(
        status.trading_day,
        crate::services::time::trading_day_key(commit_now_ms, "UTC")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn disabling_risk_waits_for_failed_order_release_and_clears_disk_quota() {
    let root = test_path("disable-during-order");
    std::fs::create_dir_all(&root).unwrap();
    let store = ConfigStore::with_path(root.join("config.toml"));
    let ledger = root.join("risk_usage.toml");
    let mut initial = AppConfig::default();
    initial.risk_max_daily_orders = 1;
    let (config, risk) = runtime(initial, &ledger);
    let coordinator = AccountLifecycleCoordinator::new();
    let order_request = market_order();
    let (release_submit, wait_for_release) = tokio::sync::oneshot::channel::<()>();

    let order = execute_coordinated_reserved_order(
        &coordinator,
        &risk,
        &order_request,
        None,
        || NOW_MS,
        || async move {
            wait_for_release.await.unwrap();
            Err::<Order, AppError>(AppError::Trading("rejected".into()))
        },
    );
    tokio::pin!(order);
    assert!(matches!(
        futures_util::poll!(&mut order),
        std::task::Poll::Pending
    ));

    let mut disabled = request("Asia/Shanghai");
    disabled.enabled = false;
    let update = execute_risk_config_update(
        RiskUpdateContext {
            coordinator: &coordinator,
            config_store: &store,
            runtime_config: &config,
            risk: &risk,
        },
        disabled,
        || NOW_MS,
        || async {},
    );
    tokio::pin!(update);
    assert!(matches!(
        futures_util::poll!(&mut update),
        std::task::Poll::Pending
    ));

    release_submit.send(()).unwrap();
    let order_result = tokio::time::timeout(std::time::Duration::from_secs(1), order.as_mut())
        .await
        .expect("order lifecycle should finish after submission is released");
    assert!(matches!(order_result, Err(AppError::Trading(_))));
    let status = tokio::time::timeout(std::time::Duration::from_secs(1), update.as_mut())
        .await
        .expect("risk update should acquire the coordinator after order cleanup")
        .unwrap();
    assert!(!status.enabled);

    let usage = RiskUsageStore::with_path(ledger)
        .load()
        .unwrap()
        .expect("reservation should leave a persisted zero-usage ledger");
    assert_eq!(usage.occupied_orders, 0);
    let _ = std::fs::remove_dir_all(root);
}
