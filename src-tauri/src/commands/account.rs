use serde_json::Value;
use tauri::State;

use crate::api::PrivateApi;
use crate::error::AppResult;
use crate::models::account::{AccountSummary, Balance};
use crate::models::api_requests::ApiTransferRequest;
use crate::models::trading::{Position, TradeStats};
use crate::state::AppState;

#[tauri::command]
pub async fn refresh_account(state: State<'_, AppState>) -> AppResult<AccountSummary> {
    let (account_id, symbol) = {
        let config = state.config.read().await;
        (config.active_account_id.clone(), config.active_symbol.clone())
    };
    state
        .account
        .refresh_account(&account_id, Some(&symbol))
        .await
}

#[tauri::command]
pub async fn refresh_balances(state: State<'_, AppState>) -> AppResult<Vec<Balance>> {
    state.account.refresh_balances().await
}

#[tauri::command]
pub async fn refresh_positions(
    state: State<'_, AppState>,
    symbol: Option<String>,
) -> AppResult<Vec<Position>> {
    state.account.refresh_positions(symbol.as_deref()).await
}

#[tauri::command]
pub async fn get_trade_stats(state: State<'_, AppState>) -> AppResult<TradeStats> {
    Ok(state.analytics.compute_stats().await)
}

#[tauri::command]
pub async fn export_trade_log(state: State<'_, AppState>) -> AppResult<String> {
    Ok(state
        .trade_log
        .export_path()
        .to_string_lossy()
        .to_string())
}

#[tauri::command]
pub async fn fetch_funding_balances(state: State<'_, AppState>) -> AppResult<Value> {
    PrivateApi::funding_balances(&state.api).await
}

#[tauri::command]
pub async fn transfer_funds(
    state: State<'_, AppState>,
    request: ApiTransferRequest,
) -> AppResult<Value> {
    PrivateApi::transfer_funds(&state.api, &request).await
}

#[tauri::command]
pub async fn fetch_user_id(state: State<'_, AppState>) -> AppResult<Value> {
    PrivateApi::user_id(&state.api).await
}

#[tauri::command]
pub async fn fetch_transfer_history(
    state: State<'_, AppState>,
    start_time: i64,
    end_time: i64,
    coin: Option<String>,
    page_num: Option<u32>,
    page_size: Option<u32>,
) -> AppResult<Value> {
    PrivateApi::transfer_history(
        &state.api,
        start_time,
        end_time,
        coin.as_deref(),
        page_num,
        page_size,
    )
    .await
}
