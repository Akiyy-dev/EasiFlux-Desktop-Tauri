use tauri::State;

use crate::error::AppResult;
use crate::models::account::{AccountProfile, AccountSwitchResult};
use crate::models::config::{ApiCredential, AppConfig, ConnectionStatus};
use crate::services::account_profiles::{
    self, AccountLifecyclePort, AccountProfileListPort, CredentialRepository,
    KeyringCredentialRepository,
};
use crate::state::AppState;

struct StateLifecyclePort<'a> {
    state: &'a AppState,
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
    ) -> AppResult<()> {
        let symbol = self.state.config.read().await.active_symbol.clone();
        self.state
            .connection
            .connect(account_id, realtime, &symbol, Some(credential))
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
pub async fn delete_account(state: State<'_, AppState>, account_id: String) -> AppResult<()> {
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
