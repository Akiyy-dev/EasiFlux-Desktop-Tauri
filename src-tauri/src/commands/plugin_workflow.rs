use super::plugin::PluginCommandState;
use crate::{error::AppResult, plugin::workflow::*};
use tauri::State;

#[tauri::command]
pub(crate) async fn get_plugin_workflow_access(
    state: State<'_, PluginCommandState>,
    host: State<'_, WorkflowHostState>,
    request: AuthorityRequest,
) -> AppResult<Access> {
    state
        .runtime
        .get_workflow_access(host.0.clone(), request)
        .await
}
#[tauri::command]
pub(crate) async fn set_plugin_workflow_grants(
    state: State<'_, PluginCommandState>,
    host: State<'_, WorkflowHostState>,
    request: GrantRequest,
) -> AppResult<Access> {
    state
        .runtime
        .set_workflow_grants(host.0.clone(), request)
        .await
}
#[tauri::command]
pub(crate) async fn run_plugin_workflow(
    state: State<'_, PluginCommandState>,
    host: State<'_, WorkflowHostState>,
    request: RunRequest,
) -> AppResult<WorkflowResult> {
    state.runtime.run_workflow(host.0.clone(), request).await
}
#[tauri::command]
pub(crate) async fn confirm_plugin_workflow(
    state: State<'_, PluginCommandState>,
    host: State<'_, WorkflowHostState>,
    token: String,
) -> AppResult<TradeReceipt> {
    state.runtime.confirm_workflow(host.0.clone(), token).await
}
