use std::sync::Arc;

use tokio::sync::RwLock;

use super::*;
use crate::models::config::AppConfig;
use crate::models::notification::NotificationSettings;
use crate::services::AccountLifecycleCoordinator;
use crate::storage::ConfigStore;

fn test_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "easiflux-notification-settings-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4(),
    ))
}

fn changed_settings() -> NotificationSettings {
    NotificationSettings {
        trading_toast: false,
        risk_account_toast: false,
        connection_system_toast: true,
    }
}

#[tokio::test]
async fn update_changes_only_notification_settings_after_persistence() {
    let path = test_path("narrow").with_extension("toml");
    let store = ConfigStore::with_path(path.clone());
    let coordinator = AccountLifecycleCoordinator::new();
    let mut initial = AppConfig::default();
    initial.active_symbol = "ETHUSDT".into();
    initial.risk_max_order_qty = "25.5".into();
    store.save(&initial).unwrap();
    let runtime = Arc::new(RwLock::new(initial.clone()));

    let result =
        apply_notification_settings_update(&coordinator, &store, &runtime, changed_settings())
            .await
            .unwrap();

    let memory = runtime.read().await.clone();
    let disk = store.load().unwrap();
    for config in [&memory, &disk] {
        assert_eq!(config.notification_settings, changed_settings());
        assert_eq!(config.active_symbol, initial.active_symbol);
        assert_eq!(config.risk_max_order_qty, initial.risk_max_order_qty);
    }
    assert_eq!(result, changed_settings());
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn persistence_failure_retains_runtime_and_last_committed_notification_settings() {
    let blocker = test_path("blocked-parent");
    std::fs::write(&blocker, "not a directory").unwrap();
    let store = ConfigStore::with_path(blocker.join("config.toml"));
    let initial = AppConfig::default();
    let runtime = Arc::new(RwLock::new(initial.clone()));

    let result = apply_notification_settings_update(
        &AccountLifecycleCoordinator::new(),
        &store,
        &runtime,
        changed_settings(),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        runtime.read().await.notification_settings,
        initial.notification_settings
    );
    assert_eq!(
        initial.notification_settings,
        NotificationSettings::default()
    );
    std::fs::remove_file(blocker).unwrap();
}
