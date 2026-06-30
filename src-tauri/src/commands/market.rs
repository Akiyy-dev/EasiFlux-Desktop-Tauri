use tauri::State;

use crate::error::AppResult;
use crate::models::market::{Depth, Kline, Ticker};
use crate::state::AppState;

#[tauri::command]
pub async fn set_active_symbol(state: State<'_, AppState>, symbol: String) -> AppResult<()> {
    state.market.set_active_symbol(&symbol).await;
    let mut config = state.config.write().await;
    config.active_symbol = symbol.clone();
    state.config_store.save(&config)?;
    if state.connection.status().await == crate::models::config::ConnectionStatus::Connected {
        if let Err(e) = state.connection.refresh_realtime(&symbol).await {
            state
                .emitter
                .emit_error(&format!("WebSocket 重订阅失败: {}", e));
        }
        if let Err(e) = state.market.refresh_snapshot(&symbol).await {
            state
                .emitter
                .emit_error(&format!("行情快照失败: {}", e));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn set_kline_interval(state: State<'_, AppState>, interval: String) -> AppResult<()> {
    state.market.set_kline_interval(&interval).await;
    let mut config = state.config.write().await;
    config.kline_interval = interval.clone();
    state.config_store.save(&config)?;
    let symbol = state.market.active_symbol().await;
    state.market.fetch_klines(&symbol, &interval).await?;
    Ok(())
}

#[tauri::command]
pub async fn refresh_market(state: State<'_, AppState>) -> AppResult<()> {
    let symbol = state.market.active_symbol().await;
    state.market.refresh_snapshot(&symbol).await
}

#[tauri::command]
pub async fn fetch_ticker(state: State<'_, AppState>, symbol: String) -> AppResult<Ticker> {
    state.market.fetch_ticker(&symbol).await
}

#[tauri::command]
pub async fn fetch_depth(state: State<'_, AppState>, symbol: String) -> AppResult<Depth> {
    state.market.fetch_depth(&symbol).await
}

#[tauri::command]
pub async fn fetch_klines(
    state: State<'_, AppState>,
    symbol: String,
    interval: String,
) -> AppResult<Vec<Kline>> {
    state.market.fetch_klines(&symbol, &interval).await
}
