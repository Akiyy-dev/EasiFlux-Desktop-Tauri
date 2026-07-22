use super::super::*;
use super::support::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[tokio::test]
async fn valid_update_persists_complete_config_before_runtime_update() {
    let root = test_path("success");
    std::fs::create_dir_all(&root).unwrap();
    let store = ConfigStore::with_path(root.join("config.toml"));
    let ledger = root.join("risk_usage.toml");
    let mut initial = AppConfig::default();
    initial.active_account_id = "backup".into();
    initial.accounts = vec!["primary".into(), "backup".into()];
    initial.window_width = 1555;
    let (config, risk) = runtime(initial, &ledger);

    let (status, timezone_changed) =
        apply_risk_config_update(&store, &config, &risk, request("UTC"), || NOW_MS)
            .await
            .unwrap();

    let persisted = store.load().unwrap();
    assert_eq!(persisted.active_account_id, "backup");
    assert_eq!(persisted.accounts, vec!["primary", "backup"]);
    assert_eq!(persisted.window_width, 1555);
    assert_eq!(persisted.risk_max_order_qty, "25.5");
    assert_eq!(config.read().await.trading_day_timezone, "UTC");
    assert_eq!(status.trading_day_timezone, "UTC");
    assert!(timezone_changed);
    assert!(!ledger.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn persistence_failure_leaves_runtime_config_and_service_unchanged() {
    let blocker = test_path("blocked");
    std::fs::write(&blocker, "blocks directory creation").unwrap();
    let store = ConfigStore::with_path(blocker.join("config.toml"));
    let initial = AppConfig::default();
    let ledger = test_path("blocked-ledger");
    let (config, risk) = runtime(initial.clone(), &ledger);

    assert!(
        apply_risk_config_update(&store, &config, &risk, request("UTC"), || NOW_MS)
            .await
            .is_err()
    );
    assert_eq!(
        config.read().await.trading_day_timezone,
        initial.trading_day_timezone
    );
    assert_eq!(
        risk.read().await.status(NOW_MS).trading_day_timezone,
        initial.trading_day_timezone
    );
    assert!(!ledger.exists());
    let _ = std::fs::remove_file(blocker);
}

#[tokio::test]
async fn unchanged_timezone_does_not_request_daily_pnl_refresh() {
    let root = test_path("same-timezone");
    std::fs::create_dir_all(&root).unwrap();
    let store = ConfigStore::with_path(root.join("config.toml"));
    let ledger = root.join("risk_usage.toml");
    let (config, risk) = runtime(AppConfig::default(), &ledger);

    let (_, timezone_changed) =
        apply_risk_config_update(&store, &config, &risk, request("Asia/Shanghai"), || NOW_MS)
            .await
            .unwrap();

    assert!(!timezone_changed);
    assert!(!ledger.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn successful_timezone_persistence_triggers_daily_pnl_refresh() {
    let root = test_path("refresh-success");
    std::fs::create_dir_all(&root).unwrap();
    let store = ConfigStore::with_path(root.join("config.toml"));
    let ledger = root.join("risk_usage.toml");
    let (config, risk) = runtime(AppConfig::default(), &ledger);
    let coordinator = AccountLifecycleCoordinator::new();
    let refreshed = AtomicBool::new(false);

    execute_risk_config_update(
        RiskUpdateContext {
            coordinator: &coordinator,
            config_store: &store,
            runtime_config: &config,
            risk: &risk,
        },
        request("UTC"),
        || NOW_MS,
        || async { refreshed.store(true, Ordering::SeqCst) },
    )
    .await
    .unwrap();

    assert!(refreshed.load(Ordering::SeqCst));
    assert_eq!(store.load().unwrap().trading_day_timezone, "UTC");
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn failed_persistence_does_not_trigger_daily_pnl_refresh() {
    let blocker = test_path("refresh-failure");
    std::fs::write(&blocker, "blocks directory creation").unwrap();
    let store = ConfigStore::with_path(blocker.join("config.toml"));
    let ledger = test_path("refresh-failure-ledger");
    let (config, risk) = runtime(AppConfig::default(), &ledger);
    let coordinator = AccountLifecycleCoordinator::new();
    let refreshed = AtomicBool::new(false);

    let result = execute_risk_config_update(
        RiskUpdateContext {
            coordinator: &coordinator,
            config_store: &store,
            runtime_config: &config,
            risk: &risk,
        },
        request("UTC"),
        || NOW_MS,
        || async { refreshed.store(true, Ordering::SeqCst) },
    )
    .await;

    assert!(result.is_err());
    assert!(!refreshed.load(Ordering::SeqCst));
    let _ = std::fs::remove_file(blocker);
}
