use crate::models::config::{ConnectionStatus, SaveCredentialRequest, DEFAULT_BASE_URL};

use std::sync::Arc;

use super::super::{
    delete_account, run_serialized_account_mutation, save_credentials, AccountLifecycleCoordinator,
    DeleteAccountWarningCode,
};
use super::support::FakeLifecyclePort;

#[tokio::test]
async fn delete_rejects_active_and_only_accounts() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Disconnected);
    assert_eq!(
        delete_account(&AccountLifecycleCoordinator::new(), &port, "primary")
            .await
            .unwrap_err()
            .to_string(),
        "配置错误: 不能删除当前账户"
    );
    port.runtime.lock().unwrap().accounts = vec!["primary".into()];
    port.runtime.lock().unwrap().active_account_id = "primary".into();
    assert_eq!(
        delete_account(&AccountLifecycleCoordinator::new(), &port, "spare")
            .await
            .unwrap_err()
            .to_string(),
        "配置错误: 不能删除唯一账户"
    );
    assert!(port.events().is_empty());
}

#[tokio::test]
async fn delete_persist_failure_restores_credential_and_runtime_list() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Disconnected);
    port.failures.lock().unwrap().persist_for = Some("primary".into());
    assert!(
        delete_account(&AccountLifecycleCoordinator::new(), &port, "spare")
            .await
            .is_err()
    );
    assert!(port.credentials.lock().unwrap().contains_key("spare"));
    assert!(port.runtime_config().accounts.contains(&"spare".into()));
    assert_eq!(
        port.events(),
        ["delete:spare", "persist:primary", "save:spare"]
    );
}

fn draft(account_id: &str, key: &str, secret: &str) -> SaveCredentialRequest {
    SaveCredentialRequest {
        account_id: account_id.into(),
        api_key: key.into(),
        api_secret: secret.into(),
        base_url: DEFAULT_BASE_URL.into(),
        label: account_id.into(),
    }
}

#[tokio::test]
async fn save_requires_complete_pairs_and_preserves_existing_blank_pair() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Disconnected);
    let coordinator = AccountLifecycleCoordinator::new();
    assert_eq!(
        save_credentials(&coordinator, &port, draft("new", "", ""))
            .await
            .unwrap_err()
            .to_string(),
        "认证失败: 新账户必须填写 API 访问密钥和签名密钥"
    );
    assert_eq!(
        save_credentials(&coordinator, &port, draft("backup", "new", ""))
            .await
            .unwrap_err()
            .to_string(),
        "认证失败: API 访问密钥和签名密钥必须同时填写"
    );
    save_credentials(&coordinator, &port, draft("backup", "", ""))
        .await
        .unwrap();
    let saved = port.credentials.lock().unwrap()["backup"].clone();
    assert_eq!(saved.api_key, "backup-key");
    assert_eq!(saved.api_secret, "backup-secret");
    save_credentials(&coordinator, &port, draft("new", "key", "secret"))
        .await
        .unwrap();
    assert!(port.runtime_config().accounts.contains(&"new".into()));
}

#[tokio::test]
async fn orphan_keyring_entry_absent_from_config_requires_complete_credentials() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Disconnected);
    let orphan = port.credentials.lock().unwrap()["backup"].clone();
    port.credentials
        .lock()
        .unwrap()
        .insert("orphan".into(), orphan);

    let result = save_credentials(
        &AccountLifecycleCoordinator::new(),
        &port,
        draft("orphan", "", ""),
    )
    .await;

    assert!(result.is_err());
    assert!(!port.runtime_config().accounts.contains(&"orphan".into()));
}

#[tokio::test]
async fn delete_cleanup_success_is_awaited_after_commit_without_warning() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Disconnected);

    let result = delete_account(&AccountLifecycleCoordinator::new(), &port, "spare")
        .await
        .unwrap();

    assert!(!result.notification_cleanup_pending());
    assert_eq!(result.warning_code(), None);
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::json!({ "notificationCleanupPending": false })
    );
    assert!(!port.credentials.lock().unwrap().contains_key("spare"));
    assert!(!port.persisted_config().accounts.contains(&"spare".into()));
    assert!(!port.runtime_config().accounts.contains(&"spare".into()));
    assert_eq!(
        port.events(),
        [
            "delete:spare",
            "persist:primary",
            "notification-cleanup:spare"
        ]
    );
}

#[tokio::test]
async fn delete_cleanup_failure_returns_only_the_closed_pending_warning_after_commit() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Disconnected);
    port.failures.lock().unwrap().notification_cleanup = true;

    let result = delete_account(&AccountLifecycleCoordinator::new(), &port, "spare")
        .await
        .expect("notification cleanup is non-critical after account commit");

    assert!(result.notification_cleanup_pending());
    assert_eq!(
        result.warning_code(),
        Some(DeleteAccountWarningCode::NotificationCleanupPending)
    );
    assert!(!port.credentials.lock().unwrap().contains_key("spare"));
    assert!(!port.persisted_config().accounts.contains(&"spare".into()));
    assert!(!port.runtime_config().accounts.contains(&"spare".into()));
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::json!({
            "notificationCleanupPending": true,
            "warningCode": "NOTIFICATION_CLEANUP_PENDING"
        })
    );
}

#[tokio::test]
async fn delete_holds_the_mutation_guard_through_awaited_notification_cleanup() {
    let port = Arc::new(FakeLifecyclePort::new(ConnectionStatus::Disconnected));
    let coordinator = Arc::new(AccountLifecycleCoordinator::new());
    let cleanup_started = Arc::new(tokio::sync::Notify::new());
    let cleanup_release = Arc::new(tokio::sync::Notify::new());
    port.delay_notification_cleanup(Arc::clone(&cleanup_started), Arc::clone(&cleanup_release));

    let deleting = tokio::spawn({
        let port = Arc::clone(&port);
        let coordinator = Arc::clone(&coordinator);
        async move { delete_account(coordinator.as_ref(), port.as_ref(), "spare").await }
    });
    cleanup_started.notified().await;
    let concurrent = tokio::spawn({
        let port = Arc::clone(&port);
        let coordinator = Arc::clone(&coordinator);
        async move {
            run_serialized_account_mutation(coordinator.as_ref(), || async {
                port.events.lock().unwrap().push("concurrent:update".into());
            })
            .await;
        }
    });
    tokio::task::yield_now().await;
    assert!(!port.events().contains(&"concurrent:update".into()));

    cleanup_release.notify_one();
    deleting.await.unwrap().unwrap();
    concurrent.await.unwrap();
    let events = port.events();
    assert!(
        events
            .iter()
            .position(|event| event == "notification-cleanup:spare")
            < events.iter().position(|event| event == "concurrent:update")
    );
}

#[tokio::test]
async fn save_rejects_unsafe_base_urls_before_keyring_or_config_mutation() {
    let port = FakeLifecyclePort::new(ConnectionStatus::Disconnected);
    let mut request = draft("new", "key", "secret");
    request.base_url = "https://user:raw-secret@example.test/api?token=raw".into();

    let error = save_credentials(&AccountLifecycleCoordinator::new(), &port, request)
        .await
        .expect_err("credential-bearing URL must be rejected");

    assert_eq!(error.to_string(), "认证失败: API 服务地址无效");
    assert!(!port.credentials.lock().unwrap().contains_key("new"));
    assert!(!port.runtime_config().accounts.contains(&"new".into()));
    assert!(port.events().is_empty());
    assert!(!error.to_string().contains("raw-secret"));
    assert!(!error.to_string().contains("token="));
}
