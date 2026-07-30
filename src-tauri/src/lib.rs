mod api;
mod auth;
mod commands;
mod error;
mod events;
mod models;
pub mod news_provision;
mod news_runtime;
mod plugin;
mod services;
mod state;
mod storage;
mod ws;

use std::future::Future;
use std::time::Duration;

use tauri::{Manager, RunEvent};

use commands::*;
use services::news::NewsShutdownOutcome;
use state::AppState;
pub use storage::{
    KeyringNewsTokenStore, NewsApiToken, NewsTokenError, NewsTokenStore, NewsTokenStoreError,
};

const NEWS_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(20);

async fn stop_news_with<Stop, StopFuture, OnTimeout>(stop: Stop, on_timeout: OnTimeout)
where
    Stop: FnOnce(Duration) -> StopFuture,
    StopFuture: Future<Output = NewsShutdownOutcome>,
    OnTimeout: FnOnce(),
{
    match stop(NEWS_SHUTDOWN_TIMEOUT).await {
        NewsShutdownOutcome::Stopped => {}
        NewsShutdownOutcome::TimedOut => on_timeout(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter("easiflux_desktop=info")
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let state = AppState::new(handle.clone())?;
            let scheduler = state.scheduler.clone();
            let news = state.news.clone();
            app.manage(state);

            let emitter = {
                let state: tauri::State<AppState> = app.state();
                state.emitter.clone()
            };
            emitter.emit_app_ready(&handle.package_info().version.to_string());

            tauri::async_runtime::spawn(async move {
                scheduler.start().await;
            });
            tauri::async_runtime::spawn(async move {
                news.start();
            });

            if let Some(window) = app.get_webview_window("main") {
                let state: tauri::State<AppState> = app.state();
                let config =
                    tauri::async_runtime::block_on(async { state.config.read().await.clone() });
                let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
                    width: config.window_width as f64,
                    height: config.window_height as f64,
                }));
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            get_version,
            get_server_time,
            get_time_snapshot,
            sync_time_now,
            get_environment_status,
            get_news_status,
            list_news_messages,
            mark_news_seen,
            recheck_news_credentials,
            retry_news_sync,
            scheduler_run_task,
            get_config,
            save_config,
            get_risk_status,
            update_risk_config,
            save_credentials,
            has_credentials,
            save_window_size,
            list_account_profiles,
            switch_account,
            delete_account,
            connect,
            disconnect,
            get_connection_status,
            get_websocket_status,
            test_connection,
            set_active_symbol,
            set_kline_interval,
            set_chart_context,
            load_chart_workspace,
            save_chart_workspace,
            refresh_market,
            refresh_funding_rate,
            fetch_ticker,
            fetch_depth,
            fetch_klines,
            fetch_public_trades,
            fetch_funding_rate_history,
            fetch_mark_price_klines,
            fetch_instruments,
            fetch_risk_limit,
            fetch_market_close_time,
            fetch_fiat_rate,
            place_order,
            cancel_order,
            refresh_orders,
            cancel_all_orders,
            replace_order,
            fetch_orders,
            fetch_trade_fills,
            fetch_fee_rate,
            set_leverage,
            add_margin,
            close_all_positions,
            fetch_closed_pnl,
            create_tpsl,
            replace_tpsl,
            switch_margin_mode,
            switch_separate_position_mode,
            refresh_account,
            refresh_balances,
            refresh_positions,
            fetch_funding_balances,
            transfer_funds,
            fetch_user_id,
            fetch_transfer_history,
            get_trade_stats,
            export_trade_log,
            probe_private_endpoints,
            refresh_order_history,
            refresh_private_panels,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                let state: tauri::State<AppState> = app.state();
                let news = state.news.clone();
                tauri::async_runtime::block_on(async {
                    stop_news_with(
                        move |timeout| async move { news.stop_and_join(timeout).await },
                        || {
                            tracing::warn!(
                                category = "news_shutdown_timeout",
                                "news shutdown timed out"
                            );
                        },
                    )
                    .await;
                    state.scheduler.stop().await;
                });
                for (key, result) in state.chart_workspace.flush_dirty_klines() {
                    if let Err(error) = result {
                        tracing::error!(
                            symbol = %key.symbol,
                            interval = %key.interval,
                            %error,
                            "final kline flush failed"
                        );
                    }
                }
            }
        });
}

#[cfg(test)]
mod capability_tests {
    use tauri::ipc::Origin;

    #[test]
    fn main_window_can_force_close_after_chart_workspace_flush() {
        let mut context: tauri::Context<tauri::Wry> = tauri::generate_context!();

        let access = context.runtime_authority_mut().resolve_access(
            "plugin:window|destroy",
            "main",
            "main",
            &Origin::Local,
        );

        assert!(
            access.is_some(),
            "main window must be allowed to destroy itself after the close guard flushes chart workspaces"
        );
    }
}

#[cfg(test)]
#[path = "news_lifecycle_tests.rs"]
mod news_lifecycle_tests;
