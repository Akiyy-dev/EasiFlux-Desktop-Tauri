use std::sync::Arc;

use tauri::State;
use tokio::sync::RwLock;

use crate::error::AppResult;
use crate::models::config::AppConfig;
use crate::models::notification::NotificationSettings;
use crate::services::AccountLifecycleCoordinator;
use crate::state::AppState;
use crate::storage::ConfigStore;

async fn apply_notification_settings_update(
    coordinator: &AccountLifecycleCoordinator,
    config_store: &ConfigStore,
    runtime_config: &Arc<RwLock<AppConfig>>,
    settings: NotificationSettings,
) -> AppResult<NotificationSettings> {
    crate::services::account_profiles::run_serialized_account_mutation(coordinator, || async {
        let mut next = runtime_config.read().await.clone();
        next.notification_settings = settings;
        config_store.save(&next)?;
        *runtime_config.write().await = next;
        Ok(settings)
    })
    .await
}

#[tauri::command]
pub async fn get_notification_settings(
    state: State<'_, AppState>,
) -> AppResult<NotificationSettings> {
    Ok(state.config.read().await.notification_settings)
}

#[tauri::command]
pub async fn update_notification_settings(
    state: State<'_, AppState>,
    settings: NotificationSettings,
) -> AppResult<NotificationSettings> {
    apply_notification_settings_update(
        state.account_lifecycle.as_ref(),
        &state.config_store,
        &state.config,
        settings,
    )
    .await
}

#[cfg(test)]
mod tests;
