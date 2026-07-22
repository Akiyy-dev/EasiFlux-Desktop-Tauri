use std::sync::Arc;

use tokio::sync::RwLock;

use crate::error::AppResult;
use crate::models::config::AppConfig;
use crate::services::AccountLifecycleCoordinator;
use crate::storage::ConfigStore;

pub(crate) async fn persist_market_config_update<F, C, Fut>(
    coordinator: &AccountLifecycleCoordinator,
    config_store: &ConfigStore,
    runtime_config: &Arc<RwLock<AppConfig>>,
    mutate: F,
    after_commit: C,
) -> AppResult<()>
where
    F: FnOnce(&mut AppConfig),
    C: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    crate::services::account_profiles::run_serialized_account_mutation(coordinator, || async {
        let mut next = runtime_config.read().await.clone();
        mutate(&mut next);
        config_store.save(&next)?;
        *runtime_config.write().await = next;
        after_commit().await;
        Ok(())
    })
    .await
}
