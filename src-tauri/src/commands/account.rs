use tauri::State;

use crate::error::AppResult;
use crate::models::account::{AccountSummary, Balance};
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
