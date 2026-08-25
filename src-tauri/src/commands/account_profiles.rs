use tauri::State;

use crate::error::AppResult;
use crate::models::account::{AccountProfile, AccountSwitchResult};
use crate::models::config::{ApiCredential, AppConfig, ConnectionStatus};
use crate::models::trading::SessionContext;
use crate::services::account_profiles::{
    self, AccountLifecyclePort, AccountProfileListPort, CredentialRepository, DeleteAccountResult,
    KeyringCredentialRepository, NotificationPartitionCleanupError,
};
use crate::services::notification::NotificationRuntime;
use crate::state::AppState;

struct StateLifecyclePort<'a> {
    state: &'a AppState,
}

async fn activate_public_base_url(api: &crate::api::ApiClient, credential: &ApiCredential) {
    api.set_base_url(&credential.base_url).await;
}

async fn delete_notification_partition(
    runtime: &NotificationRuntime,
    account_id: &str,
    now_ms: u64,
) -> Result<(), NotificationPartitionCleanupError> {
    let service = runtime
        .service()
        .map_err(|_| NotificationPartitionCleanupError)?;
    service
        .delete_account_partition(account_id, now_ms)
        .await
        .map_err(|_| NotificationPartitionCleanupError)
}

impl CredentialRepository for StateLifecyclePort<'_> {
    fn load(&self, account_id: &str) -> AppResult<Option<ApiCredential>> {
        KeyringCredentialRepository.load(account_id)
    }

    fn save(&self, account_id: &str, credential: &ApiCredential) -> AppResult<()> {
        KeyringCredentialRepository.save(account_id, credential)
    }

    fn delete(&self, account_id: &str) -> AppResult<()> {
        KeyringCredentialRepository.delete(account_id)
    }
}

impl AccountProfileListPort for StateLifecyclePort<'_> {
    async fn read_profile_config(&self) -> AppConfig {
        self.state.config.read().await.clone()
    }
}

impl AccountLifecyclePort for StateLifecyclePort<'_> {
    async fn read_config(&self) -> AppConfig {
        self.state.config.read().await.clone()
    }

    async fn replace_runtime_config(&self, config: AppConfig) {
        *self.state.config.write().await = config;
    }

    fn persist_config(&self, config: &AppConfig) -> AppResult<()> {
        self.state.config_store.save(config)
    }

    fn load_credential(&self, account_id: &str) -> AppResult<Option<ApiCredential>> {
        self.load(account_id)
    }

    fn save_credential(&self, account_id: &str, credential: &ApiCredential) -> AppResult<()> {
        self.save(account_id, credential)
    }

    fn delete_credential(&self, account_id: &str) -> AppResult<()> {
        self.delete(account_id)
    }

    async fn connection_status(&self) -> ConnectionStatus {
        self.state.connection.status().await
    }

    async fn preflight(&self, credential: &ApiCredential) -> AppResult<()> {
        crate::commands::connection::preflight_credential(credential).await
    }

    async fn disconnect(&self) {
        self.state.connection.disconnect().await;
    }

    async fn connect(
        &self,
        account_id: &str,
        realtime: bool,
        credential: ApiCredential,
        session_epoch: u64,
    ) -> AppResult<()> {
        let symbol = self.state.config.read().await.active_symbol.clone();
        self.state
            .connection
            .connect_for_session(
                SessionContext {
                    account_id: crate::models::config::normalize_account_id(account_id),
                    session_epoch,
                },
                realtime,
                &symbol,
                Some(credential),
            )
            .await
    }

    async fn activate_public_environment(&self, credential: &ApiCredential) {
        activate_public_base_url(self.state.api.as_ref(), credential).await;
    }

    async fn activate_session(&self, context: &SessionContext) {
        self.state
            .connection
            .activate_committed_session(context)
            .await;
        self.state.ws.activate_committed_session(context).await;
    }

    async fn clear_account_data(&self) {
        self.state.analytics.clear_account_data().await;
    }

    async fn delete_notification_partition(
        &self,
        account_id: &str,
    ) -> Result<(), NotificationPartitionCleanupError> {
        delete_notification_partition(
            self.state.notification.as_ref(),
            account_id,
            self.state.time.local_now_ms(),
        )
        .await
    }
}

#[tauri::command]
pub async fn list_account_profiles(state: State<'_, AppState>) -> AppResult<Vec<AccountProfile>> {
    let port = StateLifecyclePort { state: &state };
    Ok(
        account_profiles::list_account_profiles_transaction(
            state.account_lifecycle.as_ref(),
            &port,
        )
        .await,
    )
}

#[tauri::command]
pub async fn switch_account(
    state: State<'_, AppState>,
    account_id: String,
    start_realtime: Option<bool>,
) -> AppResult<AccountSwitchResult> {
    let port = StateLifecyclePort { state: &state };
    account_profiles::switch_account(
        state.account_lifecycle.as_ref(),
        &port,
        &account_id,
        start_realtime,
    )
    .await
}

#[tauri::command]
pub async fn delete_account(
    state: State<'_, AppState>,
    account_id: String,
) -> AppResult<DeleteAccountResult> {
    let port = StateLifecyclePort { state: &state };
    account_profiles::delete_account(state.account_lifecycle.as_ref(), &port, &account_id).await
}

pub(crate) async fn save_credentials_transaction(
    state: &AppState,
    request: crate::models::config::SaveCredentialRequest,
) -> AppResult<()> {
    let port = StateLifecyclePort { state };
    account_profiles::save_credentials(state.account_lifecycle.as_ref(), &port, request).await
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::{activate_public_base_url, delete_notification_partition};
    use crate::api::ApiClient;
    use crate::error::AppResult;
    use crate::models::config::{ApiCredential, AppConfig, ConnectionStatus};
    use crate::models::trading::SessionContext;
    use crate::services::account_profiles::{
        self, AccountLifecycleCoordinator, AccountLifecyclePort, NotificationPartitionCleanupError,
    };
    use crate::services::notification::{NotificationAvailability, NotificationRuntime};

    struct UnavailableRuntimeDeletePort {
        runtime_config: Mutex<AppConfig>,
        persisted_config: Mutex<AppConfig>,
        credential_present: Mutex<bool>,
        notification: NotificationRuntime,
    }

    impl UnavailableRuntimeDeletePort {
        fn new() -> Self {
            let config = AppConfig {
                active_account_id: "primary".into(),
                accounts: vec!["primary".into(), "spare".into()],
                ..Default::default()
            };
            Self {
                runtime_config: Mutex::new(config.clone()),
                persisted_config: Mutex::new(config),
                credential_present: Mutex::new(true),
                notification: NotificationRuntime::Unavailable(NotificationAvailability::new(
                    "NOTIFICATION_STORAGE_UNAVAILABLE",
                    "通知中心暂时不可用",
                )),
            }
        }
    }

    impl AccountLifecyclePort for UnavailableRuntimeDeletePort {
        async fn read_config(&self) -> AppConfig {
            self.runtime_config.lock().unwrap().clone()
        }

        async fn replace_runtime_config(&self, config: AppConfig) {
            *self.runtime_config.lock().unwrap() = config;
        }

        fn persist_config(&self, config: &AppConfig) -> AppResult<()> {
            *self.persisted_config.lock().unwrap() = config.clone();
            Ok(())
        }

        fn load_credential(&self, _account_id: &str) -> AppResult<Option<ApiCredential>> {
            Ok(self
                .credential_present
                .lock()
                .unwrap()
                .then(ApiCredential::default))
        }

        fn save_credential(&self, _account_id: &str, _credential: &ApiCredential) -> AppResult<()> {
            *self.credential_present.lock().unwrap() = true;
            Ok(())
        }

        fn delete_credential(&self, _account_id: &str) -> AppResult<()> {
            *self.credential_present.lock().unwrap() = false;
            Ok(())
        }

        async fn connection_status(&self) -> ConnectionStatus {
            ConnectionStatus::Disconnected
        }

        async fn preflight(&self, _credential: &ApiCredential) -> AppResult<()> {
            unreachable!("delete does not preflight")
        }

        async fn disconnect(&self) {
            unreachable!("delete does not disconnect")
        }

        async fn connect(
            &self,
            _account_id: &str,
            _realtime: bool,
            _credential: ApiCredential,
            _session_epoch: u64,
        ) -> AppResult<()> {
            unreachable!("delete does not connect")
        }

        async fn activate_public_environment(&self, _credential: &ApiCredential) {
            unreachable!("delete does not activate an environment")
        }

        async fn activate_session(&self, _context: &SessionContext) {
            unreachable!("delete does not activate a session")
        }

        async fn clear_account_data(&self) {
            unreachable!("delete does not clear active-account data")
        }

        async fn delete_notification_partition(
            &self,
            account_id: &str,
        ) -> Result<(), NotificationPartitionCleanupError> {
            delete_notification_partition(&self.notification, account_id, 123).await
        }
    }

    #[tokio::test]
    async fn public_environment_activation_changes_url_without_installing_credentials() {
        let api = ApiClient::new();
        api.set_base_url("https://source.example.test").await;
        let target = ApiCredential {
            api_key: "target-key".into(),
            api_secret: "target-secret".into(),
            base_url: " https://target.example.test/ ".into(),
            label: "target".into(),
        };

        activate_public_base_url(&api, &target).await;

        assert_eq!(api.base_url().await, "https://target.example.test");
        assert!(!api.has_credential().await);
    }

    #[tokio::test]
    async fn unavailable_notification_runtime_maps_to_closed_cleanup_failure() {
        let runtime = NotificationRuntime::Unavailable(NotificationAvailability::new(
            "NOTIFICATION_STORAGE_UNAVAILABLE",
            "通知中心暂时不可用",
        ));

        let result = delete_notification_partition(&runtime, "deleted-account", 123).await;

        assert_eq!(result, Err(super::NotificationPartitionCleanupError));
    }

    #[tokio::test]
    async fn unavailable_notification_runtime_keeps_delete_committed_with_exact_pending_result() {
        let port = UnavailableRuntimeDeletePort::new();

        let result =
            account_profiles::delete_account(&AccountLifecycleCoordinator::new(), &port, "spare")
                .await
                .expect("notification unavailability is post-commit and nonblocking");

        assert_eq!(
            serde_json::to_value(result).unwrap(),
            serde_json::json!({
                "notificationCleanupPending": true,
                "warningCode": "NOTIFICATION_CLEANUP_PENDING"
            })
        );
        assert!(!*port.credential_present.lock().unwrap());
        assert!(!port
            .persisted_config
            .lock()
            .unwrap()
            .accounts
            .contains(&"spare".into()));
        assert!(!port
            .runtime_config
            .lock()
            .unwrap()
            .accounts
            .contains(&"spare".into()));
    }
}
