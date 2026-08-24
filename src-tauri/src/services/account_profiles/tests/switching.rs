use std::path::PathBuf;
use std::sync::Arc;

use crate::error::AppError;
use crate::models::config::{ConnectionStatus, RiskConfig};
use crate::models::trading::{Order, PlaceOrderRequest};
use crate::services::trading::execute_coordinated_reserved_order;
use crate::services::RiskService;
use crate::storage::RiskUsageStore;

use super::super::{
    delete_account, run_serialized_account_mutation, switch_account, AccountLifecycleCoordinator,
};
use super::support::FakeLifecyclePort;

#[tokio::test]
async fn switching_to_active_account_is_idempotent() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Connected);
    let coordinator = AccountLifecycleCoordinator::new();
    let result = switch_account(&coordinator, &port, " primary ", None)
        .await
        .unwrap();
    assert_eq!(result.active_account_id, "primary");
    assert!(result.connected);
    assert_eq!(result.session_epoch, 0);
    assert_eq!(coordinator.current_session_epoch(), 0);
    assert_eq!(port.analytics_clear_count(), 0);
    assert_eq!(port.public_base_url(), "https://primary.example.test");
    assert!(port.public_has_credential());
    assert_eq!(port.public_environment_activation_count(), 0);
    assert!(port.events().is_empty());
}

#[tokio::test]
async fn target_validation_and_preflight_leave_source_untouched() {
    for target in ["missing", "broken", "backup"] {
        let port = FakeLifecyclePort::new(ConnectionStatus::Connected);
        if target == "broken" {
            port.failures.lock().unwrap().credential_load_for = Some(target.into());
            port.runtime.lock().unwrap().accounts.push(target.into());
        } else if target == "missing" {
            port.runtime.lock().unwrap().accounts.push(target.into());
        } else {
            port.failures.lock().unwrap().preflight = true;
        }
        assert!(
            switch_account(&AccountLifecycleCoordinator::new(), &port, target, None)
                .await
                .is_err()
        );
        assert_eq!(port.runtime_config().active_account_id, "primary");
        assert_eq!(port.persisted_config().active_account_id, "primary");
        assert_eq!(*port.status.lock().unwrap(), ConnectionStatus::Connected);
        assert_eq!(port.analytics_clear_count(), 0);
        assert_eq!(port.public_base_url(), "https://primary.example.test");
        assert!(port.public_has_credential());
        assert_eq!(port.public_environment_activation_count(), 0);
        assert!(!port.events().iter().any(|event| event == "disconnect"));
    }
}

#[tokio::test]
async fn disconnected_switch_persists_without_reconnecting() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Disconnected);
    let coordinator = AccountLifecycleCoordinator::new();
    let result = switch_account(&coordinator, &port, "backup", None)
        .await
        .unwrap();
    assert!(!result.connected);
    assert_eq!(result.session_epoch, 1);
    assert_eq!(coordinator.current_session_epoch(), 1);
    assert_eq!(port.analytics_clear_count(), 1);
    assert_eq!(port.public_base_url(), "https://backup.example.test");
    assert!(!port.public_has_credential());
    assert_eq!(port.public_environment_activation_count(), 1);
    assert_eq!(
        port.events(),
        [
            "preflight:backup",
            "disconnect",
            "persist:backup",
            "activate:backup:1",
        ]
    );
}

#[tokio::test]
async fn non_connected_transitional_states_activate_only_the_target_public_environment() {
    for status in [ConnectionStatus::Error, ConnectionStatus::Connecting] {
        let port = FakeLifecyclePort::new(status);
        let coordinator = AccountLifecycleCoordinator::new();

        let result = switch_account(&coordinator, &port, "backup", None)
            .await
            .unwrap();

        assert!(!result.connected);
        assert_eq!(result.session_epoch, 1);
        assert_eq!(coordinator.current_session_epoch(), 1);
        assert_eq!(port.analytics_clear_count(), 1);
        assert_eq!(port.public_base_url(), "https://backup.example.test");
        assert!(!port.public_has_credential());
        assert_eq!(port.public_environment_activation_count(), 1);
    }
}

#[tokio::test]
async fn successful_switch_persists_the_repaired_account_list() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Disconnected);
    port.runtime.lock().unwrap().accounts = vec![" backup ".into(), "backup".into()];
    port.persisted.lock().unwrap().accounts = vec![" backup ".into(), "backup".into()];

    switch_account(&AccountLifecycleCoordinator::new(), &port, "backup", None)
        .await
        .unwrap();

    assert_eq!(
        port.persisted_config().accounts,
        vec!["primary".to_string(), "backup".to_string()]
    );
    assert_eq!(
        port.runtime_config().accounts,
        vec!["primary".to_string(), "backup".to_string()]
    );
}

#[tokio::test]
async fn connected_switch_disconnects_persists_and_reconnects() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Connected);
    let result = switch_account(
        &AccountLifecycleCoordinator::new(),
        &port,
        "backup",
        Some(false),
    )
    .await
    .unwrap();
    assert!(result.connected);
    assert_eq!(port.analytics_clear_count(), 1);
    assert_eq!(port.public_base_url(), "https://backup.example.test");
    assert!(port.public_has_credential());
    assert_eq!(port.public_environment_activation_count(), 0);
    assert_eq!(
        port.events(),
        [
            "preflight:backup",
            "disconnect",
            "persist:backup",
            "connect:backup",
            "activate:backup:1",
        ]
    );
}

#[tokio::test]
async fn target_connect_failure_rolls_back_config_and_connection() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Connected);
    let coordinator = AccountLifecycleCoordinator::new();
    port.failures.lock().unwrap().target_connect = true;
    let error = switch_account(&coordinator, &port, "backup", None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("target connection failed"));
    assert_eq!(port.runtime_config().active_account_id, "primary");
    assert_eq!(port.persisted_config().active_account_id, "primary");
    assert_eq!(coordinator.current_session_epoch(), 0);
    assert_eq!(port.analytics_clear_count(), 0);
    assert_eq!(port.public_base_url(), "https://primary.example.test");
    assert!(port.public_has_credential());
    assert_eq!(port.public_environment_activation_count(), 0);
    assert_eq!(
        port.events(),
        [
            "preflight:backup",
            "disconnect",
            "persist:backup",
            "connect:backup",
            "persist:primary",
            "connect:primary"
        ]
    );
}

#[tokio::test]
async fn target_persist_failure_rolls_back_without_advancing_the_epoch() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Connected);
    let coordinator = AccountLifecycleCoordinator::new();
    port.failures.lock().unwrap().persist_for = Some("backup".into());

    let error = switch_account(&coordinator, &port, "backup", None)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("persist backup failed"));
    assert_eq!(coordinator.current_session_epoch(), 0);
    assert_eq!(port.analytics_clear_count(), 0);
    assert_eq!(port.public_base_url(), "https://primary.example.test");
    assert!(port.public_has_credential());
    assert_eq!(port.public_environment_activation_count(), 0);
    assert_eq!(port.runtime_config().active_account_id, "primary");
    assert_eq!(port.persisted_config().active_account_id, "primary");
    assert_eq!(*port.status.lock().unwrap(), ConnectionStatus::Connected);
}

#[tokio::test]
async fn disconnected_persist_failure_keeps_the_source_public_environment() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Disconnected);
    let coordinator = AccountLifecycleCoordinator::new();
    port.failures.lock().unwrap().persist_for = Some("backup".into());

    assert!(switch_account(&coordinator, &port, "backup", None)
        .await
        .is_err());

    assert_eq!(coordinator.current_session_epoch(), 0);
    assert_eq!(port.analytics_clear_count(), 0);
    assert_eq!(port.public_base_url(), "https://primary.example.test");
    assert!(!port.public_has_credential());
    assert_eq!(port.public_environment_activation_count(), 0);
}

#[tokio::test]
async fn epoch_advances_once_only_after_target_connection_commits() {
    let mut raw = FakeLifecyclePort::new(ConnectionStatus::Connected);
    raw.delay_connect = true;
    let port = raw;
    let coordinator = AccountLifecycleCoordinator::new();
    let switching = switch_account(&coordinator, &port, "backup", None);
    tokio::pin!(switching);

    assert!(matches!(
        futures_util::poll!(&mut switching),
        std::task::Poll::Pending
    ));
    assert!(port.events().iter().any(|event| event == "connect:backup"));
    assert_eq!(coordinator.current_session_epoch(), 0);

    let result = switching.await.unwrap();
    assert_eq!(result.session_epoch, 1);
    assert_eq!(coordinator.current_session_epoch(), 1);
}

#[tokio::test]
async fn failed_switch_restores_the_former_account_list_verbatim() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Connected);
    let former_accounts = vec![" backup ".to_string(), "backup".to_string()];
    port.runtime.lock().unwrap().accounts = former_accounts.clone();
    port.persisted.lock().unwrap().accounts = former_accounts.clone();
    port.failures.lock().unwrap().target_connect = true;

    assert!(
        switch_account(&AccountLifecycleCoordinator::new(), &port, "backup", None)
            .await
            .is_err()
    );

    assert_eq!(port.persisted_config().accounts, former_accounts);
    assert_eq!(port.runtime_config().accounts, former_accounts);
}

#[tokio::test]
async fn rollback_failure_requires_recovery_and_finishes_disconnected() {
    for (restore_persist, former_connect) in [(true, false), (false, true)] {
        let port = FakeLifecyclePort::new(ConnectionStatus::Connected);
        let coordinator = AccountLifecycleCoordinator::new();
        {
            let mut failures = port.failures.lock().unwrap();
            failures.target_connect = true;
            failures.restore_persist = restore_persist;
            failures.former_connect = former_connect;
        }

        let error = switch_account(&coordinator, &port, "backup", None)
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("ACCOUNT_SWITCH_RECOVERY_REQUIRED:"));
        assert_eq!(
            error,
            "内部错误: ACCOUNT_SWITCH_RECOVERY_REQUIRED: 账户切换失败且恢复原账户失败"
        );
        assert!(!error.contains("target connection failed"));
        assert!(!error.contains("persist primary failed"));
        assert!(!error.contains("former connection failed"));
        assert_eq!(coordinator.current_session_epoch(), 0);
        assert_eq!(port.analytics_clear_count(), 0);
        assert_eq!(port.public_base_url(), "https://primary.example.test");
        assert!(!port.public_has_credential());
        assert_eq!(port.public_environment_activation_count(), 0);
        assert_eq!(*port.status.lock().unwrap(), ConnectionStatus::Disconnected);
        assert_eq!(port.events().last().map(String::as_str), Some("disconnect"));
    }
}

#[tokio::test]
async fn rollback_notification_backed_failure_preserves_the_exact_structured_id() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Connected);
    let coordinator = AccountLifecycleCoordinator::new();
    let notification_id = uuid::Uuid::new_v4().to_string();
    {
        let mut failures = port.failures.lock().unwrap();
        failures.target_connect = true;
        failures.former_connect_notified = Some(notification_id.clone());
    }

    let error = switch_account(&coordinator, &port, "backup", None)
        .await
        .expect_err("rollback connection incident must fail the switch");

    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        serde_json::json!({
            "code": "CONNECTION_UNAVAILABLE",
            "message": "交易连接暂时不可用",
            "notificationId": notification_id,
        })
    );
    assert!(matches!(error, AppError::Notified { cause: None, .. }));
    assert_eq!(coordinator.current_session_epoch(), 0);
    assert_eq!(*port.status.lock().unwrap(), ConnectionStatus::Disconnected);
}

#[tokio::test]
async fn simultaneous_switch_and_delete_are_serialized() {
    let mut raw = FakeLifecyclePort::new(ConnectionStatus::Connected);
    raw.delay_effects = true;
    let port = Arc::new(raw);
    let coordinator = Arc::new(AccountLifecycleCoordinator::new());
    let (switch, delete) = tokio::join!(
        switch_account(coordinator.as_ref(), port.as_ref(), "backup", None),
        delete_account(coordinator.as_ref(), port.as_ref(), "spare"),
    );
    switch.unwrap();
    delete.unwrap();
    assert_eq!(
        port.events(),
        [
            "preflight:backup",
            "disconnect",
            "persist:backup",
            "connect:backup",
            "activate:backup:1",
            "delete:spare",
            "persist:backup",
            "notification-cleanup:spare"
        ]
    );
}

#[tokio::test]
async fn shared_guard_serializes_public_connection_and_config_mutations() {
    let mut raw = FakeLifecyclePort::new(ConnectionStatus::Connected);
    raw.delay_effects = true;
    let port = Arc::new(raw);
    let coordinator = Arc::new(AccountLifecycleCoordinator::new());
    let external = async {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        for event in ["public:connect", "public:disconnect", "public:config"] {
            run_serialized_account_mutation(coordinator.as_ref(), || async {
                port.events.lock().unwrap().push(event.into());
            })
            .await;
        }
    };
    let (result, ()) = tokio::join!(
        switch_account(coordinator.as_ref(), port.as_ref(), "backup", None),
        external,
    );
    result.unwrap();
    assert_eq!(
        port.events(),
        [
            "preflight:backup",
            "disconnect",
            "persist:backup",
            "connect:backup",
            "activate:backup:1",
            "public:connect",
            "public:disconnect",
            "public:config"
        ]
    );
}

#[tokio::test]
async fn successful_switch_does_not_reset_global_risk_quota() {
    let path = PathBuf::from(std::env::temp_dir()).join(format!(
        "easiflux-account-switch-risk-{}.toml",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let risk = RiskService::with_store(
        RiskConfig {
            max_daily_orders: 1,
            ..RiskConfig::default()
        },
        RiskUsageStore::with_path(path.clone()),
    );
    let order = PlaceOrderRequest {
        symbol: "BTCUSDT".into(),
        side: "Buy".into(),
        order_type: "Market".into(),
        qty: "1".into(),
        position_idx: 0,
        price: None,
        time_in_force: None,
        order_link_id: None,
        reduce_only: None,
    };
    risk.reserve_order(&order, None, 1_700_000_000_000).unwrap();
    let port = FakeLifecyclePort::new(ConnectionStatus::Disconnected);
    switch_account(&AccountLifecycleCoordinator::new(), &port, "backup", None)
        .await
        .unwrap();
    assert!(risk.reserve_order(&order, None, 1_700_000_000_000).is_err());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn account_switch_waits_for_in_flight_order_lifecycle() {
    let root = PathBuf::from(std::env::temp_dir()).join(format!(
        "easiflux-account-switch-order-risk-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
        RiskConfig {
            max_daily_orders: 1,
            ..RiskConfig::default()
        },
        RiskUsageStore::with_path(root.join("risk_usage.toml")),
    )));
    let coordinator = AccountLifecycleCoordinator::new();
    let port = FakeLifecyclePort::new(ConnectionStatus::Connected);
    let order = PlaceOrderRequest {
        symbol: "BTCUSDT".into(),
        side: "Buy".into(),
        order_type: "Market".into(),
        qty: "1".into(),
        position_idx: 0,
        price: None,
        time_in_force: None,
        order_link_id: None,
        reduce_only: None,
    };
    let (release_submit, wait_for_release) = tokio::sync::oneshot::channel::<()>();
    let order_events = &port.events;

    let order_lifecycle = execute_coordinated_reserved_order(
        &coordinator,
        &risk,
        &order,
        || None,
        || 1_700_000_000_000,
        || async move {
            order_events.lock().unwrap().push("order:submit".into());
            wait_for_release.await.unwrap();
            order_events.lock().unwrap().push("order:failed".into());
            Err::<Order, AppError>(AppError::Trading("rejected".into()))
        },
        |_| async {},
    );
    let switch = switch_account(&coordinator, &port, "backup", None);
    tokio::pin!(order_lifecycle);
    tokio::pin!(switch);

    assert!(matches!(
        futures_util::poll!(&mut order_lifecycle),
        std::task::Poll::Pending
    ));
    assert_eq!(port.events(), ["order:submit"]);
    assert!(matches!(
        futures_util::poll!(&mut switch),
        std::task::Poll::Pending
    ));
    assert_eq!(port.events(), ["order:submit"]);

    release_submit.send(()).unwrap();
    let order_result =
        tokio::time::timeout(std::time::Duration::from_secs(1), order_lifecycle.as_mut())
            .await
            .expect("order lifecycle should finish after submission is released");
    assert!(matches!(order_result, Err(AppError::Trading(_))));
    tokio::time::timeout(std::time::Duration::from_secs(1), switch.as_mut())
        .await
        .expect("account switch should acquire the coordinator after order cleanup")
        .unwrap();
    assert_eq!(
        port.events(),
        [
            "order:submit",
            "order:failed",
            "preflight:backup",
            "disconnect",
            "persist:backup",
            "connect:backup",
            "activate:backup:1",
        ]
    );
    let _ = std::fs::remove_dir_all(root);
}
