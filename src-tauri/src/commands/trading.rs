use serde_json::Value;
use tauri::State;

use crate::api::PrivateApi;
use crate::error::AppResult;
use crate::models::api_requests::{
    ApiAddMarginRequest, ApiCancelAllOrdersRequest, ApiCloseAllPositionsRequest,
    ApiCreateTpslRequest, ApiReplaceOrderRequest, ApiReplaceTpslRequest,
    ApiSetLeverageRequest, ApiSwitchMarginModeRequest, ApiSwitchSeparatePositionModeRequest,
};
use crate::models::trading::{CancelOrderRequest, Order, PlaceOrderRequest};
use crate::state::AppState;

#[tauri::command]
pub async fn place_order(state: State<'_, AppState>, request: PlaceOrderRequest) -> AppResult<Order> {
    let order = state.trading.place_order(request).await?;
    state.analytics.record_order(order.clone()).await;
    Ok(order)
}

#[tauri::command]
pub async fn cancel_order(
    state: State<'_, AppState>,
    request: CancelOrderRequest,
) -> AppResult<Order> {
    let order = state.trading.cancel_order(request).await?;
    state.analytics.record_order(order.clone()).await;
    Ok(order)
}

#[tauri::command]
pub async fn refresh_orders(
    state: State<'_, AppState>,
    symbol: Option<String>,
) -> AppResult<Vec<Order>> {
    let sym = symbol.as_deref();
    state.trading.refresh_orders(sym).await
}

#[tauri::command]
pub async fn cancel_all_orders(
    state: State<'_, AppState>,
    request: ApiCancelAllOrdersRequest,
) -> AppResult<Value> {
    PrivateApi::cancel_all_orders(&state.api, &request).await
}

#[tauri::command]
pub async fn replace_order(
    state: State<'_, AppState>,
    request: ApiReplaceOrderRequest,
) -> AppResult<Value> {
    PrivateApi::replace_order(&state.api, &request).await
}

#[tauri::command]
pub async fn fetch_orders(
    state: State<'_, AppState>,
    symbol: Option<String>,
    coin: Option<String>,
    order_id: Option<String>,
    order_link_id: Option<String>,
    order_filter: Option<String>,
    limit: Option<u32>,
    cursor: Option<String>,
) -> AppResult<Value> {
    PrivateApi::orders(
        &state.api,
        symbol.as_deref(),
        coin.as_deref(),
        order_id.as_deref(),
        order_link_id.as_deref(),
        order_filter.as_deref(),
        limit,
        cursor.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn fetch_trade_fills(
    state: State<'_, AppState>,
    symbol: Option<String>,
    coin: Option<String>,
    order_id: Option<String>,
    start_time: Option<i64>,
    end_time: Option<i64>,
    exec_type: Option<String>,
    limit: Option<u32>,
    cursor: Option<String>,
) -> AppResult<Value> {
    PrivateApi::trade_fills(
        &state.api,
        symbol.as_deref(),
        coin.as_deref(),
        order_id.as_deref(),
        start_time,
        end_time,
        exec_type.as_deref(),
        limit,
        cursor.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn fetch_fee_rate(
    state: State<'_, AppState>,
    symbol: Option<String>,
    coin: Option<String>,
) -> AppResult<Value> {
    PrivateApi::fee_rate(&state.api, symbol.as_deref(), coin.as_deref()).await
}

#[tauri::command]
pub async fn set_leverage(
    state: State<'_, AppState>,
    request: ApiSetLeverageRequest,
) -> AppResult<Value> {
    PrivateApi::set_leverage(&state.api, &request).await
}

#[tauri::command]
pub async fn add_margin(
    state: State<'_, AppState>,
    request: ApiAddMarginRequest,
) -> AppResult<Value> {
    PrivateApi::add_margin(&state.api, &request).await
}

#[tauri::command]
pub async fn close_all_positions(
    state: State<'_, AppState>,
    request: ApiCloseAllPositionsRequest,
) -> AppResult<Value> {
    PrivateApi::close_all_positions(&state.api, &request).await
}

#[tauri::command]
pub async fn fetch_closed_pnl(
    state: State<'_, AppState>,
    symbol: Option<String>,
    coin: Option<String>,
    start_time: Option<i64>,
    end_time: Option<i64>,
    limit: Option<u32>,
    cursor: Option<String>,
) -> AppResult<Value> {
    PrivateApi::closed_pnl(
        &state.api,
        symbol.as_deref(),
        coin.as_deref(),
        start_time,
        end_time,
        limit,
        cursor.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn create_tpsl(
    state: State<'_, AppState>,
    request: ApiCreateTpslRequest,
) -> AppResult<Value> {
    PrivateApi::create_tpsl(&state.api, &request).await
}

#[tauri::command]
pub async fn replace_tpsl(
    state: State<'_, AppState>,
    request: ApiReplaceTpslRequest,
) -> AppResult<Value> {
    PrivateApi::replace_tpsl(&state.api, &request).await
}

#[tauri::command]
pub async fn switch_margin_mode(
    state: State<'_, AppState>,
    request: ApiSwitchMarginModeRequest,
) -> AppResult<Value> {
    PrivateApi::switch_margin_mode(&state.api, &request).await
}

#[tauri::command]
pub async fn switch_separate_position_mode(
    state: State<'_, AppState>,
    request: ApiSwitchSeparatePositionModeRequest,
) -> AppResult<Value> {
    PrivateApi::switch_separate_position_mode(&state.api, &request).await
}
