use serde_json::Value;
use tauri::State;

use crate::api::PublicApi;
use crate::error::{AppError, AppResult};
use crate::models::chart_workspace::ChartWorkspaceKey;
use crate::models::config::{normalize_account_id, ConnectionStatus};
use crate::models::market::{Depth, Kline, Ticker};
use crate::models::trading::{Order, SessionContext};
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
            let ws = state.ws.clone();
            tauri::async_runtime::spawn(async move {
                run_chart_refresh_for_active_session(
                    account_lifecycle.as_ref(),
                    config.as_ref(),
                    |context| async move {
                        if let Err(error) = market.backfill_gaps(&key.symbol, &key.interval).await {
                            emitter.emit_error(&format!("K线回填失败: {error}"));
                        }
                        if connection.status().await != ConnectionStatus::Connected {
                            return;
                        }
                        if let Err(error) = market.refresh_snapshot(&key.symbol).await {
                            emitter.emit_error(&format!("行情快照失败: {error}"));
                        }
                        let order_context = context.clone();
                        let order_context_for_observer = context.clone();
                        let position_context = context;
                        run_chart_private_refresh(
                            &emitter,
                            || trading.refresh_orders(&order_context, None),
                            |orders| async move {
                                ws.observe_manual_order_snapshots(
                                    &order_context_for_observer,
                                    &orders,
                                )
                                .await
                            },
                            || async move {
                                account
                                    .refresh_positions(&position_context, None)
                                    .await
                                    .map(|_| ())
                            },
                        )
                        .await;
                    },
                )
                .await;
            });
        }
    }
}

fn is_owned_session_expiry(error: &AppError) -> bool {
    matches!(
        error,
        AppError::Notified {
            code: "AUTH_SESSION_EXPIRED",
            notification_id,
            ..
        } if !notification_id.trim().is_empty()
    )
}

async fn run_chart_private_refresh<O, OFut, W, WFut, P, PFut>(
    emitter: &crate::events::EventEmitter,
    refresh_orders: O,
    observe_orders: W,
    refresh_positions: P,
) where
    O: FnOnce() -> OFut,
    OFut: std::future::Future<Output = AppResult<Vec<Order>>>,
    W: FnOnce(Vec<Order>) -> WFut,
    WFut: std::future::Future<Output = ()>,
    P: FnOnce() -> PFut,
    PFut: std::future::Future<Output = AppResult<()>>,
{
    match refresh_orders().await {
        Ok(orders) => observe_orders(orders).await,
        Err(error) if is_owned_session_expiry(&error) => return,
        Err(error) => emitter.emit_error(&format!("订单刷新失败: {error}")),
    }
    if let Err(error) = refresh_positions().await {
        if !is_owned_session_expiry(&error) {
            emitter.emit_error(&format!("持仓刷新失败: {error}"));
        }
    }
}

async fn run_chart_refresh_for_active_session<T, F, Fut>(
    account_lifecycle: &crate::services::AccountLifecycleCoordinator,
    config: &tokio::sync::RwLock<crate::models::config::AppConfig>,
    operation: F,
) -> T
where
    F: FnOnce(SessionContext) -> Fut,
    Fut: std::future::Future<Output = T>,
{
    run_account_public_operation(account_lifecycle, || async {
        let context = SessionContext {
            account_id: normalize_account_id(&config.read().await.active_account_id),
            session_epoch: account_lifecycle.current_session_epoch(),
        };
        operation(context).await
    })
    .await
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
    use crate::models::config::ApiCredential;
    use crate::models::config::AppConfig;
    use crate::models::notification::{
        ListNotificationsRequest, NotificationFilter, NotificationKind,
    };
    use crate::services::connection::SessionNotificationObserver;
    use crate::services::notification::{
        NotificationEmitter, NotificationRuntime, NotificationService, ViewContext,
    };
    use crate::services::AccountLifecycleCoordinator;
    use crate::storage::NotificationStore;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

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

    #[tokio::test]
    async fn queued_chart_refresh_captures_context_after_account_switch_commits() {
        let lifecycle = Arc::new(AccountLifecycleCoordinator::new());
        let config = Arc::new(tokio::sync::RwLock::new(AppConfig::default()));
        let mutation = lifecycle.mutation_guard().await;
        let refresh = run_chart_refresh_for_active_session(
            lifecycle.as_ref(),
            config.as_ref(),
            |context| async move { context },
        );
        tokio::pin!(refresh);
        assert!(matches!(
            futures_util::poll!(&mut refresh),
            std::task::Poll::Pending
        ));

        config.write().await.active_account_id = "backup".into();
        lifecycle.advance_session_epoch();
        drop(mutation);

        let context = refresh.await;
        assert_eq!(context.account_id, "backup");
        assert_eq!(context.session_epoch, 1);
    }

    #[tokio::test]
    async fn chart_private_refresh_stops_after_real_session_marker_without_redelivery() {
        let root = std::env::temp_dir().join(format!(
            "easiflux-chart-session-refresh-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let path = root.join("notifications.v1.json");
        let created = Arc::new(AtomicUsize::new(0));
        let created_by_callback = Arc::clone(&created);
        let changed: NotificationEmitter = Arc::new(move |_| {
            created_by_callback.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        let service = Arc::new(
            NotificationService::load(
                NotificationStore::with_path(path),
                &["alpha".into()],
                1_784_606_400_000,
                changed,
            )
            .unwrap(),
        );
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let emitter = crate::events::EventEmitter::new_test(Arc::clone(&diagnostics));
        let lifecycle = Arc::new(AccountLifecycleCoordinator::new());
        let config = Arc::new(tokio::sync::RwLock::new(AppConfig {
            active_account_id: "alpha".into(),
            ..Default::default()
        }));
        let session_observer = SessionNotificationObserver::new(
            Arc::new(NotificationRuntime::Available(Arc::clone(&service))),
            config,
            lifecycle,
            emitter.clone(),
        );

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for body in [
                r#"{"code":0,"data":{"time":"1782850580"}}"#,
                r#"{"code":26200003,"message":"raw session detail"}"#,
            ] {
                let (mut socket, _) = listener.accept().unwrap();
                let mut request = [0_u8; 4096];
                let _ = socket.read(&mut request).unwrap();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).unwrap();
            }
        });
        let api = Arc::new(crate::api::ApiClient::new());
        let context = SessionContext {
            account_id: "alpha".into(),
            session_epoch: 0,
        };
        api.set_credential_for_session(
            ApiCredential {
                api_key: "test-key".into(),
                api_secret: "test-secret".into(),
                base_url: format!("http://{address}"),
                label: "test".into(),
            },
            context,
        )
        .await;
        api.set_auth_failure_observer(Arc::new(move |context, failure| {
            let observer = session_observer.clone();
            Box::pin(async move {
                observer
                    .observe_api_auth_failure(&context, failure, 1_784_606_400_000)
                    .await
            })
        }));
        let position_requests = Arc::new(AtomicUsize::new(0));
        let counted_positions = Arc::clone(&position_requests);

        run_chart_private_refresh(
            &emitter,
            || async {
                api.private_get(crate::api::endpoints::OPEN_ORDERS, Vec::new())
                    .await
                    .map(|_| Vec::new())
            },
            |_| async {},
            || async move {
                counted_positions.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .await;
        server.join().unwrap();

        assert_eq!(position_requests.load(Ordering::SeqCst), 0);
        assert_eq!(created.load(Ordering::SeqCst), 1);
        let records = service
            .list(
                ViewContext::account("alpha").unwrap(),
                ListNotificationsRequest {
                    account_id: Some("alpha".into()),
                    filter: NotificationFilter::All,
                    cursor: None,
                    limit: 100,
                },
                1_784_606_410_000,
            )
            .await
            .unwrap()
            .items;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, NotificationKind::AccountSessionExpired);
        let diagnostics = diagnostics.lock().unwrap();
        assert_eq!(
            diagnostics
                .iter()
                .filter(|(name, _)| name == "log:entry")
                .count(),
            1
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|(name, _)| name == "error:occurred")
                .count(),
            0
        );
        drop(diagnostics);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn chart_private_refresh_keeps_ordinary_paired_delivery_and_continues() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let emitter = crate::events::EventEmitter::new_test(Arc::clone(&events));
        let position_requests = Arc::new(AtomicUsize::new(0));
        let counted_positions = Arc::clone(&position_requests);

        run_chart_private_refresh(
            &emitter,
            || async { Err(AppError::Connection("orders unavailable".into())) },
            |_| async {},
            || async move {
                counted_positions.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .await;

        assert_eq!(position_requests.load(Ordering::SeqCst), 1);
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, "log:entry");
        assert_eq!(events[1].0, "error:occurred");
    }

    #[tokio::test]
    async fn chart_position_session_marker_has_no_generic_redelivery() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let emitter = crate::events::EventEmitter::new_test(Arc::clone(&events));

        run_chart_private_refresh(
            &emitter,
            || async { Ok(Vec::new()) },
            |_| async {},
            || async {
                Err(AppError::Notified {
                    code: "AUTH_SESSION_EXPIRED",
                    message: "账户会话已失效",
                    notification_id: "notification-chart-position".into(),
                    cause: Some(crate::error::NotificationCause::AuthFailure(
                        crate::api::response::AuthFailureKind::SessionExpired,
                    )),
                })
            },
        )
        .await;

        assert!(events.lock().unwrap().is_empty());

        run_chart_private_refresh(
            &emitter,
            || async { Ok(Vec::new()) },
            |_| async {},
            || async {
                Err(AppError::Notified {
                    code: "AUTH_SESSION_EXPIRED",
                    message: "账户会话已失效",
                    notification_id: "".into(),
                    cause: None,
                })
            },
        )
        .await;
        assert_eq!(events.lock().unwrap().len(), 2);
    }
}
