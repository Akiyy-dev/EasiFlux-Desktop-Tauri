use serde_json::Value;
use tauri::State;

use crate::api::PublicApi;
use crate::error::AppResult;
use crate::models::config::ConnectionStatus;
use crate::models::market::{Depth, Kline, Ticker};
use crate::state::AppState;

#[tauri::command]
pub async fn set_active_symbol(state: State<'_, AppState>, symbol: String) -> AppResult<()> {
    state.market.set_active_symbol(&symbol).await;
    {
        let mut config = state.config.write().await;
        config.active_symbol = symbol.clone();
        state.config_store.save(&config)?;
    }

    let interval = state.market.kline_interval().await;
    if let Err(e) = state.market.restore_klines(&symbol, &interval) {
        state.emitter.emit_error(&format!("K线恢复失败: {}", e));
    }

    if state.connection.status().await == ConnectionStatus::Connected {
        let connection = state.connection.clone();
        let market = state.market.clone();
        let trading = state.trading.clone();
        let account = state.account.clone();
        let analytics = state.analytics.clone();
        let emitter = state.emitter.clone();
        let symbol_bg = symbol.clone();
        let interval = interval.clone();

        tauri::async_runtime::spawn(async move {
            if let Err(e) = market.backfill_gaps(&symbol_bg, &interval).await {
                emitter.emit_error(&format!("K线回填失败: {}", e));
            }
            if let Err(e) = connection.refresh_realtime(&symbol_bg).await {
                emitter.emit_error(&format!("WebSocket 重订阅失败: {}", e));
            }
            if let Err(e) = market.refresh_snapshot(&symbol_bg).await {
                emitter.emit_error(&format!("行情快照失败: {}", e));
            }
            match trading.refresh_orders(None).await {
                Ok(orders) => {
                    for order in orders {
                        analytics.record_order(order).await;
                    }
                }
                Err(e) => emitter.emit_error(&format!("订单刷新失败: {}", e)),
            }
            match account.refresh_positions(None).await {
                Ok(positions) => {
                    for position in positions {
                        analytics.record_position(position).await;
                    }
                }
                Err(e) => emitter.emit_error(&format!("持仓刷新失败: {}", e)),
            }
        });
    }

    Ok(())
}

#[tauri::command]
pub async fn set_kline_interval(state: State<'_, AppState>, interval: String) -> AppResult<()> {
    state.market.set_kline_interval(&interval).await;
    {
        let mut config = state.config.write().await;
        config.kline_interval = interval.clone();
        state.config_store.save(&config)?;
    }
    let symbol = state.market.active_symbol().await;
    if let Err(e) = state.market.restore_klines(&symbol, &interval) {
        state
            .emitter
            .emit_error(&format!("K线恢复失败: {}", e));
    }
    state.market.backfill_gaps(&symbol, &interval).await?;
    if state.connection.status().await == ConnectionStatus::Connected {
        if let Err(e) = state.connection.refresh_realtime(&symbol).await {
            state
                .emitter
                .emit_error(&format!("K线 WebSocket 重订阅失败: {}", e));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn refresh_market(state: State<'_, AppState>) -> AppResult<()> {
    let symbol = state.market.active_symbol().await;
    state.market.refresh_snapshot(&symbol).await
}

#[tauri::command]
pub async fn refresh_funding_rate(state: State<'_, AppState>) -> AppResult<()> {
    let symbol = state.market.active_symbol().await;
    state.market.refresh_funding_rate(&symbol).await
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

#[tauri::command]
pub async fn fetch_public_trades(
    state: State<'_, AppState>,
    symbol: String,
    limit: Option<u32>,
) -> AppResult<Value> {
    PublicApi::public_trades(&state.api, &symbol, limit).await
}

#[tauri::command]
pub async fn fetch_funding_rate_history(
    state: State<'_, AppState>,
    symbol: String,
    from_time: Option<i64>,
    to_time: Option<i64>,
    limit: Option<u32>,
) -> AppResult<Value> {
    PublicApi::funding_rate_history(&state.api, &symbol, from_time, to_time, limit).await
}

#[tauri::command]
pub async fn fetch_mark_price_klines(
    state: State<'_, AppState>,
    symbol: String,
    interval: String,
    limit: Option<u32>,
    start: Option<i64>,
    end: Option<i64>,
) -> AppResult<Value> {
    PublicApi::mark_price_kline(&state.api, &symbol, &interval, limit, start, end).await
}

#[tauri::command]
pub async fn fetch_instruments(
    state: State<'_, AppState>,
    symbol: Option<String>,
) -> AppResult<Value> {
    PublicApi::instruments(&state.api, symbol.as_deref()).await
}

#[tauri::command]
pub async fn fetch_risk_limit(state: State<'_, AppState>, symbol: String) -> AppResult<Value> {
    PublicApi::risk_limit(&state.api, &symbol).await
}

#[tauri::command]
pub async fn fetch_market_close_time(state: State<'_, AppState>) -> AppResult<Value> {
    PublicApi::market_close_time(&state.api).await
}

#[tauri::command]
pub async fn fetch_fiat_rate(
    state: State<'_, AppState>,
    symbol_list: Option<String>,
) -> AppResult<Value> {
    PublicApi::fiat_rate(&state.api, symbol_list.as_deref()).await
}
