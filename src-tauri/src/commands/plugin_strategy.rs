use std::sync::Arc;

use crate::{error::AppResult, plugin::strategy::*, plugin::workflow::AuthorityRequest};
use tauri::State;

#[tauri::command]
pub(crate) async fn get_plugin_strategy_access(
    supervisor: State<'_, Arc<StrategySupervisor>>,
    request: AuthorityRequest,
) -> AppResult<StrategyAccess> {
    supervisor.access(request).await
}

#[tauri::command]
pub(crate) async fn start_plugin_strategy(
    supervisor: State<'_, Arc<StrategySupervisor>>,
    request: StrategyStartRequest,
) -> AppResult<StrategyRunView> {
    supervisor.start(request).await
}

#[tauri::command]
pub(crate) async fn list_plugin_strategies(
    supervisor: State<'_, Arc<StrategySupervisor>>,
) -> AppResult<StrategyList> {
    supervisor.list()
}

#[tauri::command]
pub(crate) async fn control_plugin_strategy(
    supervisor: State<'_, Arc<StrategySupervisor>>,
    request: StrategyControlRequest,
) -> AppResult<StrategyRunView> {
    supervisor.control(request).await
}

#[tauri::command]
pub(crate) async fn stop_all_plugin_strategies(
    supervisor: State<'_, Arc<StrategySupervisor>>,
) -> AppResult<StrategyList> {
    supervisor.stop_all().await
}

#[tauri::command]
pub(crate) async fn reconcile_plugin_strategy(
    supervisor: State<'_, Arc<StrategySupervisor>>,
    run_id: String,
) -> AppResult<StrategyRunView> {
    supervisor.reconcile(run_id).await
}
