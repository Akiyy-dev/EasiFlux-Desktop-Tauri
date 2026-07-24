use crate::models::config::{ConnectionStatus, SaveCredentialRequest, DEFAULT_BASE_URL};

use super::super::{delete_account, save_credentials, AccountLifecycleCoordinator};
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
