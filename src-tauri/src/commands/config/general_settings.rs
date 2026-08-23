use std::sync::Arc;
use std::time::Duration;

use tauri::State;
use tokio::sync::RwLock;

use crate::error::{AppError, AppResult};
use crate::models::config::{
    AppConfig, GeneralSettings, UpdateGeneralSettingsRequest, MAX_TICKER_POLL_INTERVAL_SECS,
    MIN_TICKER_POLL_INTERVAL_SECS,
};
use crate::services::AccountLifecycleCoordinator;
use crate::state::AppState;
use crate::storage::ConfigStore;

fn validate_request(request: UpdateGeneralSettingsRequest) -> AppResult<GeneralSettings> {
    let seconds = request.ticker_poll_interval;
    if !seconds.is_finite()
        || !(MIN_TICKER_POLL_INTERVAL_SECS..=MAX_TICKER_POLL_INTERVAL_SECS).contains(&seconds)
    {
        return Err(AppError::Config(
            "行情轮询间隔必须是 1 到 3600 秒之间的有限数值".into(),
        ));
    }
    Ok(GeneralSettings {
        use_websocket: request.use_websocket,
        ticker_poll_interval: seconds,
    })
}

async fn apply_general_settings_update<F>(
    coordinator: &AccountLifecycleCoordinator,
    config_store: &ConfigStore,
    runtime_config: &Arc<RwLock<AppConfig>>,
    request: UpdateGeneralSettingsRequest,
    notify_interval_committed: F,
) -> AppResult<GeneralSettings>
where
    F: FnOnce(Duration),
{
    let requested = validate_request(request)?;
    crate::services::account_profiles::run_serialized_account_mutation(
        coordinator,
        move || async move {
            let mut next = runtime_config.read().await.clone();
            let interval_changed = next.ticker_poll_interval != requested.ticker_poll_interval;
            next.use_websocket = requested.use_websocket;
            next.ticker_poll_interval = requested.ticker_poll_interval;
            config_store.save(&next)?;
            *runtime_config.write().await = next.clone();
            if interval_changed {
                notify_interval_committed(Duration::from_secs_f64(requested.ticker_poll_interval));
            }
            Ok(GeneralSettings::from(&next))
        },
    )
    .await
}

#[tauri::command]
pub async fn update_general_settings(
    state: State<'_, AppState>,
    request: UpdateGeneralSettingsRequest,
) -> AppResult<GeneralSettings> {
    let scheduler = Arc::clone(&state.scheduler);
    apply_general_settings_update(
        state.account_lifecycle.as_ref(),
        &state.config_store,
        &state.config,
        request,
        move |interval| scheduler.set_market_fallback_interval(interval),
    )
    .await
}

#[cfg(test)]
mod tests;
