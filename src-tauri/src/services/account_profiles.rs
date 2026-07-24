mod listing;
mod mutations;
mod switching;

use crate::error::{AppError, AppResult};
use crate::models::config::{ApiCredential, AppConfig, ConnectionStatus};
use crate::storage::CredentialStore;
use std::sync::atomic::{AtomicU64, Ordering};

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
    async fn activate_public_environment(&self, credential: &ApiCredential);
    async fn clear_account_data(&self);
}

pub struct AccountLifecycleCoordinator {
    // Tokio's fair, write-preferring queue prevents new reads from starving a
    // pending account mutation while allowing independent snapshots to overlap.
    lifecycle: tokio::sync::RwLock<()>,
    session_epoch: AtomicU64,
}

impl AccountLifecycleCoordinator {
    pub fn new() -> Self {
        Self {
            lifecycle: tokio::sync::RwLock::new(()),
            session_epoch: AtomicU64::new(0),
        }
    }

    pub async fn mutation_guard(&self) -> tokio::sync::RwLockWriteGuard<'_, ()> {
        self.lifecycle.write().await
    }

    pub async fn read_guard(&self) -> tokio::sync::RwLockReadGuard<'_, ()> {
        self.lifecycle.read().await
    }

    pub fn current_session_epoch(&self) -> u64 {
        self.session_epoch.load(Ordering::Acquire)
    }

    pub(crate) fn advance_session_epoch(&self) -> u64 {
        self.session_epoch.fetch_add(1, Ordering::AcqRel) + 1
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

pub(crate) async fn run_account_private_mutation<T, F, Fut>(
    coordinator: &AccountLifecycleCoordinator,
    operation: F,
) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    run_serialized_account_mutation(coordinator, operation).await
}

pub(crate) async fn run_account_private_operation<T, F, Fut>(
    coordinator: &AccountLifecycleCoordinator,
    operation: F,
) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let _guard = coordinator.read_guard().await;
    operation().await
}

pub(crate) async fn run_account_public_operation<T, F, Fut>(
    coordinator: &AccountLifecycleCoordinator,
    operation: F,
) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let _guard = coordinator.read_guard().await;
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
        .map_err(|_| AppError::Auth("账户凭据不可用".into()))
}

pub(super) fn valid_credential(value: Option<ApiCredential>) -> AppResult<ApiCredential> {
    let credential = value
        .ok_or_else(|| AppError::Auth("未找到账户凭据".into()))?
        .normalize();
    if credential.is_valid() {
        Ok(credential)
    } else {
        Err(AppError::Auth("账户凭据不可用".into()))
    }
}

#[cfg(test)]
mod tests;
