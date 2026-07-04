use tauri::State;

use crate::error::AppResult;
use crate::models::config::{normalize_account_id, ApiCredential};
use crate::state::AppState;

#[tauri::command]
pub async fn test_connection(credential: ApiCredential) -> AppResult<()> {
    let temp = crate::api::ApiClient::new();
    temp.set_credential(credential.normalize()).await;
    crate::auth::time_sync::sync_from_server(
        temp.time_sync().as_ref(),
        crate::api::PublicApi::server_time(&temp),
    )
    .await?;
    crate::api::PublicApi::ticker(&temp, "BTCUSDT").await?;
    crate::api::PrivateApi::balances(&temp, None).await?;
    Ok(())
}

#[tauri::command]
pub async fn connect(
    state: State<'_, AppState>,
    start_realtime: Option<bool>,
    credential: Option<ApiCredential>,
) -> AppResult<()> {
    let (account_id, symbol, use_ws, kline_interval) = {
        let config = state.config.read().await;
        (
            normalize_account_id(&config.active_account_id),
            config.active_symbol.clone(),
            config.use_websocket,
            config.kline_interval.clone(),
        )
    };
    state
        .connection
        .connect(
            &account_id,
            start_realtime.unwrap_or(use_ws),
            &symbol,
            credential,
        )
        .await?;
    state.market.set_active_symbol(&symbol).await;
    state.market.set_kline_interval(&kline_interval).await;
    if let Err(e) = state.market.refresh_snapshot(&symbol).await {
        state
            .emitter
            .emit_error(&format!("行情快照失败: {}", e));
    }
    if let Err(e) = state
        .account
        .refresh_account(&account_id, Some(&symbol))
        .await
    {
        state
            .emitter
            .emit_error(&format!("账户刷新失败: {}", e));
    }
    match state.trading.refresh_orders(None).await {
        Ok(orders) => {
            for order in orders {
                state.analytics.record_order(order).await;
            }
        }
        Err(e) => {
            state
                .emitter
                .emit_error(&format!("订单刷新失败: {}", e));
        }
    }
    if let Err(e) = state.account.refresh_positions(None).await {
        state
            .emitter
            .emit_error(&format!("持仓刷新失败: {}", e));
    }
    Ok(())
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> AppResult<()> {
    state.connection.disconnect().await;
    Ok(())
}

#[tauri::command]
pub async fn get_connection_status(
    state: State<'_, AppState>,
) -> AppResult<crate::models::config::ConnectionStatus> {
    Ok(state.connection.status().await)
}
