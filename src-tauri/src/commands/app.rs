use tauri::{AppHandle, State};

use crate::api::{ApiClient, PublicApi};
use crate::error::AppResult;
use crate::models::config::{normalize_account_id, DEFAULT_BASE_URL};
use crate::state::AppState;
use crate::storage::CredentialStore;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResponse {
    pub message: String,
    pub version: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentStatus {
    pub base_url: String,
    pub label: String,
    pub reachable: bool,
    pub checked_at: u64,
    pub error: Option<String>,
}

fn environment_label(base_url: &str) -> &'static str {
    match url::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_string()))
        .as_deref()
    {
        Some("api.easicoin.io") => "正式",
        Some(_) => "测试",
        None if base_url.contains("api.easicoin.io") => "正式",
        None => "未知",
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn app_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
pub fn ping(app: AppHandle, _state: State<'_, AppState>) -> AppResult<PingResponse> {
    Ok(PingResponse {
        message: "pong".into(),
        version: app_version(&app),
    })
}

#[tauri::command]
pub fn get_version(app: AppHandle) -> String {
    app_version(&app)
}

#[tauri::command]
pub async fn get_environment_status(state: State<'_, AppState>) -> AppResult<EnvironmentStatus> {
    let active_account_id = {
        let config = state.config.read().await;
        normalize_account_id(&config.active_account_id)
    };
    let credential = CredentialStore::load(&active_account_id)?;
    let base_url = if let Some(credential) = credential.as_ref() {
        credential.base_url.clone()
    } else {
        state.api.base_url().await
    };
    let base_url = if base_url.trim().is_empty() {
        DEFAULT_BASE_URL.to_string()
    } else {
        base_url
    };
    let probe_client;
    let client: &ApiClient = if let Some(credential) = credential {
        probe_client = ApiClient::new();
        probe_client.set_credential(credential).await;
        &probe_client
    } else {
        state.api.as_ref()
    };
    match PublicApi::server_time(client).await {
        Ok(_) => Ok(EnvironmentStatus {
            label: environment_label(&base_url).to_string(),
            base_url,
            reachable: true,
            checked_at: now_ms(),
            error: None,
        }),
        Err(error) => Ok(EnvironmentStatus {
            label: environment_label(&base_url).to_string(),
            base_url,
            reachable: false,
            checked_at: now_ms(),
            error: Some(error.user_message()),
        }),
    }
}
