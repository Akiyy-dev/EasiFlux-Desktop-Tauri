use tauri::State;

use crate::error::AppResult;
use crate::models::config::ApiCredential;
use crate::state::AppState;

#[tauri::command]
pub async fn test_connection(credential: ApiCredential) -> AppResult<()> {
    let temp = crate::api::ApiClient::new();
    temp.set_credential(credential).await;
    crate::auth::time_sync::sync_from_server(
        temp.time_sync().as_ref(),
        crate::api::PublicApi::server_time(&temp),
    )
    .await?;
    crate::api::PublicApi::ticker(&temp, "BTCUSDT").await?;
    Ok(())
}

#[tauri::command]
pub async fn connect(
    state: State<'_, AppState>,
    start_realtime: Option<bool>,
) -> AppResult<()> {
    let (account_id, symbol, use_ws) = {
        let config = state.config.read().await;
        (
            config.active_account_id.clone(),
            config.active_symbol.clone(),
            config.use_websocket,
        )
    };
    state
        .connection
        .connect(&account_id, start_realtime.unwrap_or(use_ws), &symbol)
        .await?;
    state.market.set_active_symbol(&symbol).await;
    state.market.refresh_snapshot(&symbol).await?;
    let _ = state
        .account
        .refresh_account(&account_id, Some(&symbol))
        .await;
    let _ = state.trading.refresh_orders(Some(&symbol)).await;
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
