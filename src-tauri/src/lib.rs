mod api;
mod auth;
mod commands;
mod error;
mod events;
mod models;
mod plugin;
mod services;
mod state;
mod storage;
mod ws;

use tauri::Manager;

use commands::*;
use state::AppState;

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
            app.manage(state);

            let emitter = {
                let state: tauri::State<AppState> = app.state();
                state.emitter.clone()
            };
            emitter.emit_app_ready(env!("CARGO_PKG_VERSION"));

            if let Some(window) = app.get_webview_window("main") {
                let state: tauri::State<AppState> = app.state();
                let config = tauri::async_runtime::block_on(async {
                    state.config.read().await.clone()
                });
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
            get_config,
            save_config,
            save_credentials,
            has_credentials,
            save_window_size,
            connect,
            disconnect,
            get_connection_status,
            test_connection,
            set_active_symbol,
            set_kline_interval,
            refresh_market,
            fetch_ticker,
            fetch_depth,
            fetch_klines,
            place_order,
            cancel_order,
            refresh_orders,
            refresh_account,
            refresh_balances,
            refresh_positions,
            get_trade_stats,
            export_trade_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
