use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::models::config::EnvironmentStatus;
use crate::models::time::TimeSnapshot;
use crate::state::AppState;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResponse {
    pub message: String,
    pub version: String,
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
pub async fn get_server_time(state: State<'_, AppState>) -> AppResult<u64> {
    let snapshot = state.time.sync().await?;
    Ok(snapshot.server_time_ms)
}

#[tauri::command]
pub async fn get_time_snapshot(state: State<'_, AppState>) -> AppResult<TimeSnapshot> {
    Ok(state.time.snapshot().await)
}

#[tauri::command]
pub async fn sync_time_now(state: State<'_, AppState>) -> AppResult<TimeSnapshot> {
    state.time.sync().await
}

#[tauri::command]
pub async fn get_environment_status(state: State<'_, AppState>) -> AppResult<EnvironmentStatus> {
    Ok(state.environment_status.read().await.clone())
}

#[tauri::command]
pub async fn scheduler_run_task(
    state: State<'_, AppState>,
    task: String,
    force: Option<bool>,
) -> AppResult<()> {
    let task_id = crate::services::scheduler::TaskId::from_name(&task)
        .ok_or_else(|| crate::error::AppError::Internal(format!("未知调度任务: {task}")))?;
    state
        .scheduler
        .run_now(task_id, force.unwrap_or(false))
        .await
}
