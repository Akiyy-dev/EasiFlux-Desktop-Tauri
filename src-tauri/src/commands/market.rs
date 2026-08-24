use serde_json::Value;
use tauri::State;

use crate::api::PublicApi;
use crate::error::{AppError, AppResult};
use crate::models::chart_workspace::ChartWorkspaceKey;
use crate::models::config::{normalize_account_id, ConnectionStatus};
use crate::models::market::{Depth, Kline, Ticker};
use crate::models::trading::OrderStreamContext;
use crate::services::account_profiles::run_account_public_operation;
use crate::state::AppState;

mod config_update;
#[cfg(not(test))]
use config_update::persist_market_config_update;
#[cfg(test)]
pub(crate) use config_update::persist_market_config_update;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChartContextRefreshPlan {
    None,
    BackfillOnly,
    SymbolAndAccount,
}

fn chart_context_refresh_plan(
    current: &ChartWorkspaceKey,
    next: &ChartWorkspaceKey,
) -> ChartContextRefreshPlan {
    if current == next {
        ChartContextRefreshPlan::None
    } else if current.symbol == next.symbol {
        ChartContextRefreshPlan::BackfillOnly
    } else {
        ChartContextRefreshPlan::SymbolAndAccount
    }
}

fn parse_chart_context(symbol: &str, interval: &str) -> AppResult<ChartWorkspaceKey> {
    ChartWorkspaceKey::parse(symbol, interval).map_err(AppError::Storage)
}

#[tauri::command]
pub async fn set_active_symbol(state: State<'_, AppState>, symbol: String) -> AppResult<()> {
    update_chart_context(&state, Some(symbol), None).await?;
    Ok(())
}

#[tauri::command]
pub async fn set_kline_interval(state: State<'_, AppState>, interval: String) -> AppResult<()> {
    update_chart_context(&state, None, Some(interval)).await?;
    Ok(())
}

#[tauri::command]
pub async fn set_chart_context(
    state: State<'_, AppState>,
    symbol: String,
    interval: String,
) -> AppResult<Vec<Kline>> {
    update_chart_context(&state, Some(symbol), Some(interval)).await
}

async fn update_chart_context(
    state: &AppState,
    symbol: Option<String>,
    interval: Option<String>,
) -> AppResult<Vec<Kline>> {
    let _mutation_guard = state.market.chart_context_mutation_guard().await;
    let current = state.market.chart_context().await;
    let next = parse_chart_context(
        symbol.as_deref().unwrap_or(&current.symbol),
        interval.as_deref().unwrap_or(&current.interval),
    )?;
    let refresh_plan = chart_context_refresh_plan(&current, &next);
    if refresh_plan == ChartContextRefreshPlan::None {
        return state.market.load_local_klines(&current).await;
    }

    let chart_workspace = state.chart_workspace.clone();
    let old_key = current.clone();
    match tauri::async_runtime::spawn_blocking(move || chart_workspace.flush_kline_key(&old_key))
        .await
    {
        Ok(_) => {}
        Err(error) => state
            .emitter
            .emit_error(&AppError::Internal(format!("K线缓存写入任务失败: {error}")).to_string()),
    }

    let local = state.market.load_local_klines(&next).await?;
    let next_for_config = next.clone();
    persist_market_config_update(
        state.account_lifecycle.as_ref(),
        &state.config_store,
        &state.config,
        move |config| {
            config.active_symbol = next_for_config.symbol.clone();
            config.kline_interval = next_for_config.interval.clone();
        },
        || async {
            state
                .market
                .replace_runtime_chart_context(next.clone())
                .await;
            if state.connection.status().await == ConnectionStatus::Connected {
                if let Err(error) = state.connection.refresh_realtime(&next.symbol).await {
                    state
                        .emitter
                        .emit_error(&format!("WebSocket 重订阅失败: {error}"));
                }
            }
        },
    )
    .await?;

    state.market.publish_local_klines(&next, local.clone());
    schedule_chart_context_refresh(state, next, refresh_plan).await;
    Ok(local)
}

async fn schedule_chart_context_refresh(
    state: &AppState,
    key: ChartWorkspaceKey,
    refresh_plan: ChartContextRefreshPlan,
) {
    match refresh_plan {
        ChartContextRefreshPlan::None => {}
        ChartContextRefreshPlan::BackfillOnly => {
            state
                .market
                .schedule_kline_backfill(&key.symbol, &key.interval);
        }
        ChartContextRefreshPlan::SymbolAndAccount => {
            if state.connection.status().await != ConnectionStatus::Connected {
                state
                    .market
                    .schedule_kline_backfill(&key.symbol, &key.interval);
                return;
            }
            let connection = state.connection.clone();
            let market = state.market.clone();
            let trading = state.trading.clone();
            let account = state.account.clone();
            let emitter = state.emitter.clone();
            let account_lifecycle = state.account_lifecycle.clone();
            let config = state.config.clone();
            tauri::async_runtime::spawn(async move {
                run_account_public_operation(account_lifecycle.as_ref(), || async {
                    if let Err(error) = market.backfill_gaps(&key.symbol, &key.interval).await {
                        emitter.emit_error(&format!("K线回填失败: {error}"));
                    }
                    if connection.status().await != ConnectionStatus::Connected {
                        return;
                    }
                    if let Err(error) = market.refresh_snapshot(&key.symbol).await {
                        emitter.emit_error(&format!("行情快照失败: {error}"));
                    }
                    let context = OrderStreamContext {
                        account_id: normalize_account_id(&config.read().await.active_account_id),
                        session_epoch: account_lifecycle.current_session_epoch(),
                    };
                    if let Err(error) = trading.refresh_orders(&context, None).await {
                        emitter.emit_error(&format!("订单刷新失败: {error}"));
                    }
                    if let Err(error) = account.refresh_positions(None).await {
                        emitter.emit_error(&format!("持仓刷新失败: {error}"));
                    }
                })
                .await;
            });
        }
    }
}

#[tauri::command]
pub async fn refresh_market(state: State<'_, AppState>) -> AppResult<()> {
    run_account_public_operation(state.account_lifecycle.as_ref(), || async {
        let symbol = state.market.active_symbol().await;
        state.market.refresh_snapshot(&symbol).await
    })
    .await
}

#[tauri::command]
pub async fn refresh_funding_rate(state: State<'_, AppState>) -> AppResult<()> {
    run_account_public_operation(state.account_lifecycle.as_ref(), || async {
        let symbol = state.market.active_symbol().await;
        state.market.refresh_funding_rate(&symbol).await
    })
    .await
}

#[tauri::command]
pub async fn fetch_ticker(state: State<'_, AppState>, symbol: String) -> AppResult<Ticker> {
    run_account_public_operation(state.account_lifecycle.as_ref(), || {
        state.market.fetch_ticker(&symbol)
    })
    .await
}

#[tauri::command]
pub async fn fetch_depth(state: State<'_, AppState>, symbol: String) -> AppResult<Depth> {
    run_account_public_operation(state.account_lifecycle.as_ref(), || {
        state.market.fetch_depth(&symbol)
    })
    .await
}

#[tauri::command]
pub async fn fetch_klines(
    state: State<'_, AppState>,
    symbol: String,
    interval: String,
    limit: Option<u32>,
    start: Option<i64>,
    end: Option<i64>,
) -> AppResult<Vec<Kline>> {
    let key = parse_chart_context(&symbol, &interval)?;
    run_account_public_operation(state.account_lifecycle.as_ref(), || {
        state.market.fetch_kline_range(&key, start, end, limit)
    })
    .await
}

#[tauri::command]
pub async fn fetch_public_trades(
    state: State<'_, AppState>,
    symbol: String,
    limit: Option<u32>,
) -> AppResult<Value> {
    run_account_public_operation(state.account_lifecycle.as_ref(), || {
        PublicApi::public_trades(&state.api, &symbol, limit)
    })
    .await
}

#[tauri::command]
pub async fn fetch_funding_rate_history(
    state: State<'_, AppState>,
    symbol: String,
    from_time: Option<i64>,
    to_time: Option<i64>,
    limit: Option<u32>,
) -> AppResult<Value> {
    run_account_public_operation(state.account_lifecycle.as_ref(), || {
        PublicApi::funding_rate_history(&state.api, &symbol, from_time, to_time, limit)
    })
    .await
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
    run_account_public_operation(state.account_lifecycle.as_ref(), || {
        PublicApi::mark_price_kline(&state.api, &symbol, &interval, limit, start, end)
    })
    .await
}

#[tauri::command]
pub async fn fetch_instruments(
    state: State<'_, AppState>,
    symbol: Option<String>,
) -> AppResult<Value> {
    run_account_public_operation(state.account_lifecycle.as_ref(), || {
        PublicApi::instruments(&state.api, symbol.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn fetch_risk_limit(state: State<'_, AppState>, symbol: String) -> AppResult<Value> {
    run_account_public_operation(state.account_lifecycle.as_ref(), || {
        PublicApi::risk_limit(&state.api, &symbol)
    })
    .await
}

#[tauri::command]
pub async fn fetch_market_close_time(state: State<'_, AppState>) -> AppResult<Value> {
    run_account_public_operation(state.account_lifecycle.as_ref(), || {
        PublicApi::market_close_time(&state.api)
    })
    .await
}

#[tauri::command]
pub async fn fetch_fiat_rate(
    state: State<'_, AppState>,
    symbol_list: Option<String>,
) -> AppResult<Value> {
    run_account_public_operation(state.account_lifecycle.as_ref(), || {
        PublicApi::fiat_rate(&state.api, symbol_list.as_deref())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chart_workspace::ChartWorkspaceKey;

    fn key(symbol: &str, interval: &str) -> ChartWorkspaceKey {
        ChartWorkspaceKey::parse(symbol, interval).unwrap()
    }

    #[test]
    fn unchanged_context_has_no_refresh_side_effects() {
        assert_eq!(
            chart_context_refresh_plan(&key("BTCUSDT", "1"), &key("BTCUSDT", "1")),
            ChartContextRefreshPlan::None
        );
    }

    #[test]
    fn interval_only_context_change_schedules_only_kline_backfill() {
        assert_eq!(
            chart_context_refresh_plan(&key("BTCUSDT", "1"), &key("BTCUSDT", "5")),
            ChartContextRefreshPlan::BackfillOnly
        );
    }

    #[test]
    fn symbol_context_change_preserves_market_order_and_position_refresh() {
        assert_eq!(
            chart_context_refresh_plan(&key("BTCUSDT", "1"), &key("ETHUSDT", "1")),
            ChartContextRefreshPlan::SymbolAndAccount
        );
    }
}
