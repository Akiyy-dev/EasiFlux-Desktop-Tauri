mod mutations;

pub use mutations::*;

use serde_json::Value;
use tauri::State;

use crate::api::PrivateApi;
use crate::error::AppResult;
use crate::models::config::normalize_account_id;
use crate::models::trading::OrderStreamContext;
use crate::models::trading::{Order, PrivatePanelsSnapshot};
use crate::services::account_profiles::run_account_private_operation;
use crate::state::AppState;

#[tauri::command]
pub async fn refresh_orders(
    state: State<'_, AppState>,
    symbol: Option<String>,
) -> AppResult<Vec<Order>> {
    run_account_private_operation(state.account_lifecycle.as_ref(), || async {
        let context = OrderStreamContext {
            account_id: normalize_account_id(&state.config.read().await.active_account_id),
            session_epoch: state.account_lifecycle.current_session_epoch(),
        };
        let orders = state
            .trading
            .refresh_orders(&context, symbol.as_deref())
            .await?;
        state
            .ws
            .observe_manual_order_snapshots(&context, &orders)
            .await;
        Ok(orders)
    })
    .await
}

#[tauri::command]
pub async fn refresh_order_history(
    state: State<'_, AppState>,
    symbol: Option<String>,
    limit: Option<u32>,
) -> AppResult<Vec<Order>> {
    run_account_private_operation(state.account_lifecycle.as_ref(), || async {
        let context = OrderStreamContext {
            account_id: normalize_account_id(&state.config.read().await.active_account_id),
            session_epoch: state.account_lifecycle.current_session_epoch(),
        };
        let orders = state
            .trading
            .refresh_order_history(&context, symbol.as_deref(), limit)
            .await?;
        state
            .ws
            .observe_manual_order_snapshots(&context, &orders)
            .await;
        for order in &orders {
            state.analytics.record_order(order.clone()).await;
        }
        Ok(orders)
    })
    .await
}

#[tauri::command]
pub async fn refresh_private_panels(
    state: State<'_, AppState>,
    symbol: Option<String>,
) -> AppResult<PrivatePanelsSnapshot> {
    run_account_private_operation(state.account_lifecycle.as_ref(), || async {
        let context = OrderStreamContext {
            account_id: normalize_account_id(&state.config.read().await.active_account_id),
            session_epoch: state.account_lifecycle.current_session_epoch(),
        };
        let sym = symbol.as_deref();
        let open_orders = state.trading.refresh_orders(&context, sym).await?;
        let order_history = state
            .trading
            .refresh_order_history(&context, sym, Some(50))
            .await?;
        let order_snapshots = open_orders
            .iter()
            .chain(&order_history)
            .cloned()
            .collect::<Vec<_>>();
        state
            .ws
            .observe_manual_order_snapshots(&context, &order_snapshots)
            .await;
        let positions = state.account.refresh_positions(&context, sym).await?;
        for order in &order_history {
            state.analytics.record_order(order.clone()).await;
        }
        Ok(PrivatePanelsSnapshot {
            open_orders,
            order_history,
            positions,
        })
    })
    .await
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
    run_account_private_operation(state.account_lifecycle.as_ref(), || {
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
    })
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
    run_account_private_operation(state.account_lifecycle.as_ref(), || {
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
    })
    .await
}

#[tauri::command]
pub async fn fetch_fee_rate(
    state: State<'_, AppState>,
    symbol: Option<String>,
    coin: Option<String>,
) -> AppResult<Value> {
    run_account_private_operation(state.account_lifecycle.as_ref(), || {
        PrivateApi::fee_rate(&state.api, symbol.as_deref(), coin.as_deref())
    })
    .await
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
    run_account_private_operation(state.account_lifecycle.as_ref(), || {
        PrivateApi::closed_pnl(
            &state.api,
            symbol.as_deref(),
            coin.as_deref(),
            start_time,
            end_time,
            limit,
            cursor.as_deref(),
        )
    })
    .await
}
