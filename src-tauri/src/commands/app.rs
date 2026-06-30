use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResponse {
    pub message: String,
    pub version: String,
}

#[tauri::command]
pub fn ping(_state: State<'_, AppState>) -> AppResult<PingResponse> {
    Ok(PingResponse {
        message: "pong".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

#[tauri::command]
pub fn get_version() -> String {
    env!("CARGO_PKG_VERSION").into()
}
