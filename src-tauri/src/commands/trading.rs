use tauri::State;

use crate::error::AppResult;
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
