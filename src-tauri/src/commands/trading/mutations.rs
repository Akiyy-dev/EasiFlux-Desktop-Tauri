use serde_json::Value;
use tauri::State;

use crate::api::PrivateApi;
use crate::error::AppResult;
use crate::models::api_requests::{
    ApiAddMarginRequest, ApiCancelAllOrdersRequest, ApiCloseAllPositionsRequest,
    ApiCreateTpslRequest, ApiReplaceOrderRequest, ApiReplaceTpslRequest, ApiSetLeverageRequest,
    ApiSwitchMarginModeRequest, ApiSwitchSeparatePositionModeRequest,
};
use crate::models::trading::{CancelOrderRequest, Order, PlaceOrderRequest};
use crate::services::account_profiles::run_account_private_mutation;
use crate::state::AppState;

async fn run_value_mutation<F, Fut>(state: &AppState, mutation: F) -> AppResult<Value>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<Value>>,
{
    run_account_private_mutation(state.account_lifecycle.as_ref(), mutation).await
}

#[tauri::command]
pub async fn place_order(
    state: State<'_, AppState>,
    request: PlaceOrderRequest,
) -> AppResult<Order> {
    state
        .trading
        .place_order(state.account_lifecycle.as_ref(), request)
        .await
}

#[tauri::command]
pub async fn cancel_order(
    state: State<'_, AppState>,
    request: CancelOrderRequest,
) -> AppResult<Order> {
    state
        .trading
        .cancel_order(state.account_lifecycle.as_ref(), request)
        .await
}

#[tauri::command]
pub async fn cancel_all_orders(
    state: State<'_, AppState>,
    request: ApiCancelAllOrdersRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || {
        PrivateApi::cancel_all_orders(&state.api, &request)
    })
    .await
}

#[tauri::command]
pub async fn replace_order(
    state: State<'_, AppState>,
    request: ApiReplaceOrderRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || PrivateApi::replace_order(&state.api, &request)).await
}

#[tauri::command]
pub async fn set_leverage(
    state: State<'_, AppState>,
    request: ApiSetLeverageRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || PrivateApi::set_leverage(&state.api, &request)).await
}

#[tauri::command]
pub async fn add_margin(
    state: State<'_, AppState>,
    request: ApiAddMarginRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || PrivateApi::add_margin(&state.api, &request)).await
}

#[tauri::command]
pub async fn close_all_positions(
    state: State<'_, AppState>,
    request: ApiCloseAllPositionsRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || {
        PrivateApi::close_all_positions(&state.api, &request)
    })
    .await
}

#[tauri::command]
pub async fn create_tpsl(
    state: State<'_, AppState>,
    request: ApiCreateTpslRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || PrivateApi::create_tpsl(&state.api, &request)).await
}

#[tauri::command]
pub async fn replace_tpsl(
    state: State<'_, AppState>,
    request: ApiReplaceTpslRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || PrivateApi::replace_tpsl(&state.api, &request)).await
}

#[tauri::command]
pub async fn switch_margin_mode(
    state: State<'_, AppState>,
    request: ApiSwitchMarginModeRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || {
        PrivateApi::switch_margin_mode(&state.api, &request)
    })
    .await
}

#[tauri::command]
pub async fn switch_separate_position_mode(
    state: State<'_, AppState>,
    request: ApiSwitchSeparatePositionModeRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || {
        PrivateApi::switch_separate_position_mode(&state.api, &request)
    })
    .await
}
