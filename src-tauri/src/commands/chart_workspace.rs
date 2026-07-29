use tauri::State;

use crate::error::{AppError, AppResult};
use crate::models::chart_workspace::{
    ChartWorkspaceKey, ChartWorkspaceSaveResult, ChartWorkspaceSnapshot, SaveChartWorkspaceRequest,
};
use crate::state::AppState;

#[tauri::command]
pub async fn load_chart_workspace(
    state: State<'_, AppState>,
    symbol: String,
    interval: String,
    from: Option<i64>,
    to: Option<i64>,
    limit: Option<u32>,
) -> AppResult<ChartWorkspaceSnapshot> {
    let key = ChartWorkspaceKey::parse(&symbol, &interval).map_err(AppError::Storage)?;
    let service = state.chart_workspace.clone();
    tauri::async_runtime::spawn_blocking(move || service.load(&key, from, to, limit))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
}

#[tauri::command]
pub async fn save_chart_workspace(
    state: State<'_, AppState>,
    request: SaveChartWorkspaceRequest,
) -> AppResult<ChartWorkspaceSaveResult> {
    let service = state.chart_workspace.clone();
    tauri::async_runtime::spawn_blocking(move || service.save(request))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
}
