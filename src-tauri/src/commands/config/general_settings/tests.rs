use super::*;
use crate::commands::market::persist_market_config_update;
use crate::error::AppError;
use crate::models::config::AppConfig;
use crate::services::AccountLifecycleCoordinator;
use crate::storage::ConfigStore;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

fn request(seconds: f64) -> UpdateGeneralSettingsRequest {
    UpdateGeneralSettingsRequest {
        use_websocket: false,
        ticker_poll_interval: seconds,
    }
}

fn test_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "easiflux-general-settings-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4(),
    ))
}

fn without_general_fields(config: &AppConfig) -> serde_json::Value {
    let mut value = serde_json::to_value(config).unwrap();
    let object = value.as_object_mut().unwrap();
    object.remove("useWebsocket");
    object.remove("tickerPollInterval");
    value
}

fn cleanup_config(path: &std::path::Path) {
    for candidate in [
        path.to_path_buf(),
        std::path::PathBuf::from(format!("{}.bak", path.display())),
        std::path::PathBuf::from(format!("{}.tmp", path.display())),
    ] {
        let _ = std::fs::remove_file(candidate);
    }
}

#[test]
fn rejects_non_finite_and_out_of_range_intervals() {
    for value in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        0.0,
        3600.000_001,
    ] {
        assert!(validate_request(request(value)).is_err(), "{value}");
    }
    assert!(validate_request(request(1.0)).is_ok());
    assert!(validate_request(request(3600.0)).is_ok());
}

#[tokio::test]
async fn update_changes_only_general_fields_after_persistence() {
    let path = test_path("narrow").with_extension("toml");
    let store = ConfigStore::with_path(path.clone());
    let coordinator = AccountLifecycleCoordinator::new();
    let mut initial = AppConfig::default();
    initial.active_symbol = "ETHUSDT".into();
    initial.active_account_id = "backup".into();
    initial.accounts = vec!["primary".into(), "backup".into()];
    initial.window_width = 1555;
    initial.watchlist_symbols = vec!["ETHUSDT".into(), "SOLUSDT".into()];
    initial.risk_max_order_qty = "25.5".into();
    store.save(&initial).unwrap();
    let runtime = Arc::new(RwLock::new(initial.clone()));
    let notifications = Arc::new(std::sync::Mutex::new(Vec::new()));

    let result = apply_general_settings_update(
        &coordinator,
        &store,
        &runtime,
        UpdateGeneralSettingsRequest {
            use_websocket: false,
            ticker_poll_interval: 15.0,
        },
        {
            let notifications = Arc::clone(&notifications);
            move |interval| notifications.lock().unwrap().push(interval)
        },
    )
    .await
    .unwrap();

    let memory = runtime.read().await.clone();
    let disk = store.load().unwrap();
    assert_eq!(
        without_general_fields(&memory),
        without_general_fields(&initial)
    );
    assert_eq!(
        without_general_fields(&disk),
        without_general_fields(&initial)
    );
    for config in [&memory, &disk] {
        assert!(!config.use_websocket);
        assert_eq!(config.ticker_poll_interval, 15.0);
    }
    assert_eq!(result.ticker_poll_interval, 15.0);
    assert!(!result.use_websocket);
    assert_eq!(*notifications.lock().unwrap(), [Duration::from_secs(15)]);
    cleanup_config(&path);
}

#[tokio::test]
async fn persistence_failure_changes_neither_runtime_nor_schedule() {
    let blocker = test_path("blocked-parent");
    std::fs::write(&blocker, "not a directory").unwrap();
    let store = ConfigStore::with_path(blocker.join("config.toml"));
    let initial = AppConfig::default();
    let runtime = Arc::new(RwLock::new(initial.clone()));
    let notifications = Arc::new(AtomicUsize::new(0));

    let result = apply_general_settings_update(
        &AccountLifecycleCoordinator::new(),
        &store,
        &runtime,
        request(30.0),
        {
            let notifications = Arc::clone(&notifications);
            move |_| {
                notifications.fetch_add(1, Ordering::SeqCst);
            }
        },
    )
    .await;

    assert!(result.is_err());
    let memory = runtime.read().await.clone();
    assert_eq!(
        without_general_fields(&memory),
        without_general_fields(&initial)
    );
    assert_eq!(memory.use_websocket, initial.use_websocket);
    assert_eq!(memory.ticker_poll_interval, initial.ticker_poll_interval);
    assert_eq!(notifications.load(Ordering::SeqCst), 0);
    std::fs::remove_file(blocker).unwrap();
}

#[tokio::test]
async fn scheduler_notification_observes_committed_disk_and_runtime_state() {
    let path = test_path("committed-observation").with_extension("toml");
    let store = ConfigStore::with_path(path.clone());
    let initial = AppConfig::default();
    store.save(&initial).unwrap();
    let runtime = Arc::new(RwLock::new(initial));
    let observed = Arc::new(std::sync::Mutex::new(None));

    apply_general_settings_update(
        &AccountLifecycleCoordinator::new(),
        &store,
        &runtime,
        request(45.0),
        {
            let observed = Arc::clone(&observed);
            let path = path.clone();
            let runtime = Arc::clone(&runtime);
            move |_| {
                let disk = ConfigStore::with_path(path).load().unwrap();
                let memory = runtime.try_read().unwrap().clone();
                *observed.lock().unwrap() =
                    Some((disk.ticker_poll_interval, memory.ticker_poll_interval));
            }
        },
    )
    .await
    .unwrap();

    assert_eq!(*observed.lock().unwrap(), Some((45.0, 45.0)));
    cleanup_config(&path);
}

#[tokio::test]
async fn unchanged_interval_does_not_rearm_market_fallback() {
    let path = test_path("unchanged-interval").with_extension("toml");
    let store = ConfigStore::with_path(path.clone());
    let initial = AppConfig::default();
    store.save(&initial).unwrap();
    let runtime = Arc::new(RwLock::new(initial));
    let notifications = Arc::new(AtomicUsize::new(0));

    let result = apply_general_settings_update(
        &AccountLifecycleCoordinator::new(),
        &store,
        &runtime,
        UpdateGeneralSettingsRequest {
            use_websocket: false,
            ticker_poll_interval: 1.0,
        },
        {
            let notifications = Arc::clone(&notifications);
            move |_| {
                notifications.fetch_add(1, Ordering::SeqCst);
            }
        },
    )
    .await
    .unwrap();

    let memory = runtime.read().await.clone();
    let disk = store.load().unwrap();
    assert!(!result.use_websocket);
    assert_eq!(result.ticker_poll_interval, 1.0);
    for config in [&memory, &disk] {
        assert!(!config.use_websocket);
        assert_eq!(config.ticker_poll_interval, 1.0);
    }
    assert_eq!(notifications.load(Ordering::SeqCst), 0);
    cleanup_config(&path);
}

#[tokio::test]
async fn concurrent_general_and_market_updates_preserve_both_modules() {
    let path = test_path("general-market").with_extension("toml");
    let store = ConfigStore::with_path(path.clone());
    let initial = AppConfig::default();
    store.save(&initial).unwrap();
    let runtime = Arc::new(RwLock::new(initial));
    let coordinator = AccountLifecycleCoordinator::new();
    let held = coordinator.mutation_guard().await;

    let general =
        apply_general_settings_update(&coordinator, &store, &runtime, request(20.0), |_| {});
    let market = persist_market_config_update(
        &coordinator,
        &store,
        &runtime,
        |next| {
            next.active_symbol = "ETHUSDT".into();
            next.kline_interval = "15".into();
        },
        || async {},
    );
    tokio::pin!(general);
    tokio::pin!(market);
    assert!(matches!(
        futures_util::poll!(&mut general),
        std::task::Poll::Pending,
    ));
    assert!(matches!(
        futures_util::poll!(&mut market),
        std::task::Poll::Pending,
    ));
    drop(held);

    let (general_result, market_result) = tokio::join!(general, market);
    let general_settings = general_result.unwrap();
    market_result.unwrap();
    assert!(!general_settings.use_websocket);
    assert_eq!(general_settings.ticker_poll_interval, 20.0);
    let memory = runtime.read().await.clone();
    let disk = store.load().unwrap();
    for config in [&memory, &disk] {
        assert!(!config.use_websocket);
        assert_eq!(config.ticker_poll_interval, 20.0);
        assert_eq!(config.active_symbol, "ETHUSDT");
        assert_eq!(config.kline_interval, "15");
    }
    cleanup_config(&path);
}

#[tokio::test]
async fn queued_lifecycle_fields_then_general_preserve_account_window_and_risk() {
    let path = test_path("general-lifecycle").with_extension("toml");
    let store = ConfigStore::with_path(path.clone());
    let initial = AppConfig::default();
    store.save(&initial).unwrap();
    let runtime = Arc::new(RwLock::new(initial));
    let coordinator = AccountLifecycleCoordinator::new();
    let held = coordinator.mutation_guard().await;

    let lifecycle_store = &store;
    let lifecycle_runtime = &runtime;
    let lifecycle = crate::services::account_profiles::run_serialized_account_mutation(
        &coordinator,
        move || async move {
            let mut next = lifecycle_runtime.read().await.clone();
            next.active_account_id = "backup".into();
            next.accounts = vec!["primary".into(), "backup".into()];
            next.window_width = 1555;
            next.window_height = 955;
            next.risk_enabled = false;
            next.risk_max_order_qty = "25.5".into();
            next.risk_max_daily_orders = 20;
            next.trading_day_timezone = "UTC".into();
            lifecycle_store.save(&next)?;
            *lifecycle_runtime.write().await = next;
            Ok::<(), AppError>(())
        },
    );
    let general =
        apply_general_settings_update(&coordinator, &store, &runtime, request(30.0), |_| {});
    tokio::pin!(lifecycle);
    tokio::pin!(general);
    assert!(matches!(
        futures_util::poll!(&mut lifecycle),
        std::task::Poll::Pending,
    ));
    assert!(matches!(
        futures_util::poll!(&mut general),
        std::task::Poll::Pending,
    ));
    drop(held);

    let (lifecycle_result, general_result) = tokio::join!(lifecycle, general);
    lifecycle_result.unwrap();
    let general_settings = general_result.unwrap();
    assert!(!general_settings.use_websocket);
    assert_eq!(general_settings.ticker_poll_interval, 30.0);
    let memory = runtime.read().await.clone();
    let disk = store.load().unwrap();
    for config in [&memory, &disk] {
        assert!(!config.use_websocket);
        assert_eq!(config.ticker_poll_interval, 30.0);
        assert_eq!(config.active_account_id, "backup");
        assert_eq!(
            config.accounts,
            vec!["primary".to_string(), "backup".to_string()],
        );
        assert_eq!((config.window_width, config.window_height), (1555, 955));
        assert!(!config.risk_enabled);
        assert_eq!(config.risk_max_order_qty, "25.5");
        assert_eq!(config.risk_max_daily_orders, 20);
        assert_eq!(config.trading_day_timezone, "UTC");
    }
    cleanup_config(&path);
}

#[tokio::test]
async fn queued_general_updates_notify_in_commit_order() {
    let path = test_path("general-order").with_extension("toml");
    let store = ConfigStore::with_path(path.clone());
    let initial = AppConfig::default();
    store.save(&initial).unwrap();
    let runtime = Arc::new(RwLock::new(initial));
    let coordinator = AccountLifecycleCoordinator::new();
    let held = coordinator.mutation_guard().await;
    let notifications = Arc::new(std::sync::Mutex::new(Vec::new()));

    let first = apply_general_settings_update(
        &coordinator,
        &store,
        &runtime,
        UpdateGeneralSettingsRequest {
            use_websocket: true,
            ticker_poll_interval: 10.0,
        },
        {
            let notifications = Arc::clone(&notifications);
            move |interval| notifications.lock().unwrap().push(interval)
        },
    );
    let second = apply_general_settings_update(
        &coordinator,
        &store,
        &runtime,
        UpdateGeneralSettingsRequest {
            use_websocket: false,
            ticker_poll_interval: 20.0,
        },
        {
            let notifications = Arc::clone(&notifications);
            move |interval| notifications.lock().unwrap().push(interval)
        },
    );
    tokio::pin!(first);
    tokio::pin!(second);
    assert!(matches!(
        futures_util::poll!(&mut first),
        std::task::Poll::Pending,
    ));
    assert!(matches!(
        futures_util::poll!(&mut second),
        std::task::Poll::Pending,
    ));
    drop(held);

    let (first_result, second_result) = tokio::join!(first, second);
    let first_settings = first_result.unwrap();
    let second_settings = second_result.unwrap();
    assert!(first_settings.use_websocket);
    assert_eq!(first_settings.ticker_poll_interval, 10.0);
    assert!(!second_settings.use_websocket);
    assert_eq!(second_settings.ticker_poll_interval, 20.0);
    assert_eq!(
        *notifications.lock().unwrap(),
        [Duration::from_secs(10), Duration::from_secs(20)],
    );
    let memory = runtime.read().await.clone();
    let disk = store.load().unwrap();
    assert!(!memory.use_websocket);
    assert_eq!(memory.ticker_poll_interval, 20.0);
    assert!(!disk.use_websocket);
    assert_eq!(disk.ticker_poll_interval, 20.0);
    cleanup_config(&path);
}
