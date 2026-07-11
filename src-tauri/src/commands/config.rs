use tauri::State;

use crate::error::AppResult;
use crate::models::config::{normalize_account_id, AppConfig, RiskConfig, SaveCredentialRequest};
use crate::state::AppState;
use crate::storage::CredentialStore;

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> AppResult<AppConfig> {
    Ok(state.config.read().await.clone())
}

#[tauri::command]
pub async fn save_config(state: State<'_, AppState>, config: AppConfig) -> AppResult<()> {
    let timezone_changed = {
        let current = state.config.read().await;
        current.trading_day_timezone != config.trading_day_timezone
    };
    state.config_store.save(&config)?;
    *state.config.write().await = config.clone();
    state
        .risk
        .write()
        .await
        .update_config(RiskConfig::from(&config));
    if timezone_changed {
        let _ = state
            .scheduler
            .run_now(crate::services::scheduler::TaskId::DailyPnl, true)
            .await;
    }
    Ok(())
}

#[tauri::command]
pub async fn save_credentials(
    state: State<'_, AppState>,
    request: SaveCredentialRequest,
) -> AppResult<()> {
    let account_id = normalize_account_id(&request.account_id);
    let mut credential = crate::models::config::ApiCredential {
        api_key: request.api_key.trim().to_string(),
        api_secret: request.api_secret.trim().to_string(),
        base_url: request.base_url.trim().to_string(),
        label: request.label.trim().to_string(),
    }
    .normalize();

    if !credential.has_secret() {
        if let Some(existing) = CredentialStore::load(&account_id)? {
            credential.api_secret = existing.api_secret;
            if credential.api_key.is_empty() {
                credential.api_key = existing.api_key;
            }
        } else {
            return Err(crate::error::AppError::Auth(
                "首次保存凭据时必须填写 API Secret".into(),
            ));
        }
    }
    if credential.api_key.is_empty() {
        return Err(crate::error::AppError::Auth("API Key 不能为空".into()));
    }

    CredentialStore::save(&account_id, &credential)?;
    let mut config = state.config.write().await;
    if !config.accounts.contains(&account_id) {
        config.accounts.push(account_id.clone());
    }
    state.config_store.save(&config)?;
    Ok(())
}

#[tauri::command]
pub async fn has_credentials(account_id: String) -> AppResult<bool> {
    Ok(CredentialStore::has(&normalize_account_id(&account_id)))
}

#[tauri::command]
pub async fn save_window_size(
    state: State<'_, AppState>,
    width: u32,
    height: u32,
) -> AppResult<()> {
    let mut config = state.config.write().await;
    config.window_width = width;
    config.window_height = height;
    state.config_store.save(&config)?;
    Ok(())
}
