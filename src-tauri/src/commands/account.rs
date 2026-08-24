use serde_json::Value;
use tauri::State;

use crate::api::diagnostic::warn_if_raw_parsed_mismatch;
use crate::api::mapper::{list_envelope_meta, parse_funding_balances};
use crate::api::PrivateApi;
use crate::error::{AppError, AppResult};
use crate::models::account::{AccountSummary, Balance, FundingBalance};
use crate::models::api_requests::ApiTransferRequest;
use crate::models::config::normalize_account_id;
use crate::models::trading::{Position, SessionContext, TradeStats};
use crate::services::account_profiles::{
    run_account_private_mutation, run_account_private_operation,
};
use crate::state::AppState;

#[tauri::command]
pub async fn refresh_account(state: State<'_, AppState>) -> AppResult<AccountSummary> {
    run_account_private_operation(state.account_lifecycle.as_ref(), || async {
        let (account_id, symbol) = {
            let config = state.config.read().await;
            (
                config.active_account_id.clone(),
                config.active_symbol.clone(),
            )
        };
        let context = SessionContext {
            account_id: normalize_account_id(&account_id),
            session_epoch: state.account_lifecycle.current_session_epoch(),
        };
        state.account.refresh_account(&context, Some(&symbol)).await
    })
    .await
}

#[tauri::command]
pub async fn refresh_balances(state: State<'_, AppState>) -> AppResult<Vec<Balance>> {
    run_account_private_operation(state.account_lifecycle.as_ref(), || async {
        let context = SessionContext {
            account_id: normalize_account_id(&state.config.read().await.active_account_id),
            session_epoch: state.account_lifecycle.current_session_epoch(),
        };
        state.account.refresh_balances(&context).await
    })
    .await
}

#[tauri::command]
pub async fn refresh_positions(
    state: State<'_, AppState>,
    symbol: Option<String>,
) -> AppResult<Vec<Position>> {
    run_account_private_operation(state.account_lifecycle.as_ref(), || async {
        let context = SessionContext {
            account_id: normalize_account_id(&state.config.read().await.active_account_id),
            session_epoch: state.account_lifecycle.current_session_epoch(),
        };
        state
            .account
            .refresh_positions(&context, symbol.as_deref())
            .await
    })
    .await
}

#[tauri::command]
pub async fn get_trade_stats(state: State<'_, AppState>) -> AppResult<TradeStats> {
    run_account_private_operation(state.account_lifecycle.as_ref(), || async {
        Ok(refresh_trade_stats_with_stale(
            &state.emitter,
            || state.analytics.refresh_from_api(&state.emitter),
            || state.analytics.compute_stats(),
        )
        .await)
    })
    .await
}

async fn refresh_trade_stats_with_stale<R, RFut, C, CFut>(
    emitter: &crate::events::EventEmitter,
    refresh: R,
    compute: C,
) -> TradeStats
where
    R: FnOnce() -> RFut,
    RFut: std::future::Future<Output = AppResult<()>>,
    C: FnOnce() -> CFut,
    CFut: std::future::Future<Output = TradeStats>,
{
    if let Err(error) = refresh().await {
        if !matches!(
            &error,
            AppError::Notified {
                code: "AUTH_SESSION_EXPIRED",
                notification_id,
                ..
            } if !notification_id.trim().is_empty()
        ) {
            emitter.emit_error(&format!("分析数据刷新失败: {}", error.user_message()));
        }
    }
    compute().await
}

#[tauri::command]
pub async fn export_trade_log(state: State<'_, AppState>) -> AppResult<String> {
    Ok(state.trade_log.export_path().to_string_lossy().to_string())
}

#[tauri::command]
pub async fn fetch_funding_balances(state: State<'_, AppState>) -> AppResult<Vec<FundingBalance>> {
    run_account_private_operation(state.account_lifecycle.as_ref(), || async {
        let payload = PrivateApi::funding_balances(&state.api).await?;
        let meta = list_envelope_meta(&payload);
        let balances = parse_funding_balances(&payload);
        warn_if_raw_parsed_mismatch(&state.emitter, "funding/balances", &meta, balances.len());
        Ok(balances)
    })
    .await
}

#[tauri::command]
pub async fn transfer_funds(
    state: State<'_, AppState>,
    request: ApiTransferRequest,
) -> AppResult<Value> {
    run_account_private_mutation(state.account_lifecycle.as_ref(), || {
        PrivateApi::transfer_funds(&state.api, &request)
    })
    .await
}

#[tauri::command]
pub async fn fetch_user_id(state: State<'_, AppState>) -> AppResult<Value> {
    run_account_private_operation(state.account_lifecycle.as_ref(), || {
        PrivateApi::user_id(&state.api)
    })
    .await
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
    run_account_private_operation(state.account_lifecycle.as_ref(), || {
        PrivateApi::transfer_history(
            &state.api,
            start_time,
            end_time,
            coin.as_deref(),
            page_num,
            page_size,
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{AppError, NotificationCause};
    use std::sync::{Arc, Mutex};

    fn stale_stats() -> TradeStats {
        TradeStats {
            total_orders: 7,
            filled_orders: 5,
            cancelled_orders: 2,
            total_volume: "12".into(),
            realized_pnl: "3".into(),
            unrealised_pnl: "4".into(),
            win_rate_pct: "60.00".into(),
            win_count: 3,
            loss_count: 2,
        }
    }

    #[tokio::test]
    async fn trade_stats_session_marker_returns_stale_stats_without_redelivery() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let emitter = crate::events::EventEmitter::new_test(Arc::clone(&events));

        let stats = refresh_trade_stats_with_stale(
            &emitter,
            || async {
                Err(AppError::Notified {
                    code: "AUTH_SESSION_EXPIRED",
                    message: "账户会话已失效",
                    notification_id: "notification-trade-stats".into(),
                    cause: Some(NotificationCause::AuthFailure(
                        crate::api::response::AuthFailureKind::SessionExpired,
                    )),
                })
            },
            || async { stale_stats() },
        )
        .await;

        assert_eq!(stats.total_orders, 7);
        assert!(events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn trade_stats_ordinary_failure_keeps_paired_delivery_and_stale_stats() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let emitter = crate::events::EventEmitter::new_test(Arc::clone(&events));

        let stats = refresh_trade_stats_with_stale(
            &emitter,
            || async { Err(AppError::Connection("analytics unavailable".into())) },
            || async { stale_stats() },
        )
        .await;

        assert_eq!(stats.total_orders, 7);
        let delivered = events.lock().unwrap();
        assert_eq!(delivered.len(), 2);
        assert_eq!(delivered[0].0, "log:entry");
        assert_eq!(delivered[1].0, "error:occurred");
        drop(delivered);

        let _ = refresh_trade_stats_with_stale(
            &emitter,
            || async {
                Err(AppError::Notified {
                    code: "AUTH_SESSION_EXPIRED",
                    message: "账户会话已失效",
                    notification_id: "".into(),
                    cause: None,
                })
            },
            || async { stale_stats() },
        )
        .await;
        assert_eq!(events.lock().unwrap().len(), 4);
    }
}
