mod listing;
mod mutations;
mod switching;

use crate::error::{AppError, AppResult};
use crate::models::config::{ApiCredential, AppConfig, ConnectionStatus};
use crate::storage::CredentialStore;

#[cfg(test)]
pub(crate) use listing::build_account_profiles;
pub(crate) use listing::{list_account_profiles_transaction, normalize_account_ids};
pub(crate) use mutations::{delete_account, save_credentials};
pub(crate) use switching::switch_account;

#[allow(async_fn_in_trait)]
pub(crate) trait AccountLifecyclePort: Send + Sync {
    async fn read_config(&self) -> AppConfig;
    async fn replace_runtime_config(&self, config: AppConfig);
    fn persist_config(&self, config: &AppConfig) -> AppResult<()>;
    fn load_credential(&self, account_id: &str) -> AppResult<Option<ApiCredential>>;
    fn save_credential(&self, account_id: &str, credential: &ApiCredential) -> AppResult<()>;
    fn delete_credential(&self, account_id: &str) -> AppResult<()>;
    async fn connection_status(&self) -> ConnectionStatus;
    async fn preflight(&self, credential: &ApiCredential) -> AppResult<()>;
    async fn disconnect(&self);
    async fn connect(
        &self,
        account_id: &str,
        realtime: bool,
        credential: ApiCredential,
    ) -> AppResult<()>;
}

pub struct AccountLifecycleCoordinator {
    mutation: tokio::sync::Mutex<()>,
}

impl AccountLifecycleCoordinator {
    pub fn new() -> Self {
        Self {
            mutation: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn mutation_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.mutation.lock().await
    }
}

impl Default for AccountLifecycleCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) async fn run_serialized_account_mutation<T, F, Fut>(
    coordinator: &AccountLifecycleCoordinator,
    operation: F,
) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let _guard = coordinator.mutation_guard().await;
    operation().await
}

pub(crate) trait CredentialRepository: Send + Sync {
    fn load(&self, account_id: &str) -> AppResult<Option<ApiCredential>>;
    fn save(&self, account_id: &str, credential: &ApiCredential) -> AppResult<()>;
    fn delete(&self, account_id: &str) -> AppResult<()>;
}

#[allow(async_fn_in_trait)]
pub(crate) trait AccountProfileListPort: CredentialRepository {
    async fn read_profile_config(&self) -> AppConfig;
}

pub(crate) struct KeyringCredentialRepository;

impl CredentialRepository for KeyringCredentialRepository {
    fn load(&self, account_id: &str) -> AppResult<Option<ApiCredential>> {
        CredentialStore::load(account_id)
    }

    fn save(&self, account_id: &str, credential: &ApiCredential) -> AppResult<()> {
        CredentialStore::save(account_id, credential)
    }

    fn delete(&self, account_id: &str) -> AppResult<()> {
        CredentialStore::delete(account_id)
    }
}

pub(super) fn safe_load<P: AccountLifecyclePort>(
    port: &P,
    account_id: &str,
) -> AppResult<Option<ApiCredential>> {
    port.load_credential(account_id)
        .map_err(|_| AppError::Auth("Account credentials are unavailable".into()))
}

pub(super) fn valid_credential(value: Option<ApiCredential>) -> AppResult<ApiCredential> {
    let credential = value
        .ok_or_else(|| AppError::Auth("Account credentials are missing".into()))?
        .normalize();
    if credential.is_valid() {
        Ok(credential)
    } else {
        Err(AppError::Auth("Account credentials are unavailable".into()))
    }
}

#[cfg(test)]
mod tests;
