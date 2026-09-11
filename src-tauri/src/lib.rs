mod api;
mod auth;
mod commands;
mod error;
mod events;
mod models;
mod plugin;
#[cfg(any(test, feature = "plugin-smoke"))]
mod plugin_smoke;
#[cfg(feature = "plugin-smoke")]
pub use plugin_smoke::run_plugin_smoke;
mod services;
mod state;
mod storage;
mod ws;

use std::sync::Arc;

use tauri::{Manager, RunEvent};

use commands::plugin::PluginCommandState;
use commands::*;
use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter("easiflux_desktop=info")
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let state = AppState::new(handle.clone())?;
            let scheduler = state.scheduler.clone();
            // Publish the scheduler's desired-running claim before app-ready
            // or command handling can expose startup to the frontend. The
            // returned driver performs network initialization asynchronously.
            let scheduler_start = scheduler.start();
            app.manage(PluginCommandState::new(Arc::clone(&state.plugins)));
            app.manage(state);

            let emitter = {
                let state: tauri::State<AppState> = app.state();
                state.emitter.clone()
            };
            emitter.emit_app_ready(&handle.package_info().version.to_string());

            tauri::async_runtime::spawn(scheduler_start);
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
            scheduler_run_task,
            get_config,
            save_config,
            update_general_settings,
            get_notification_settings,
            update_notification_settings,
            list_notifications,
            get_notification_summary,
            mark_notification_read,
            mark_visible_notifications_read,
            delete_notification,
            clear_account_notifications,
            create_client_notification,
            get_plugin_catalog,
            reload_plugin_catalog,
            set_plugin_enabled,
            prepare_local_manifest_import,
            cancel_local_manifest_import,
            commit_local_manifest_import,
            remove_managed_local_plugin,
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
            list_pending_order_submissions,
            acknowledge_order_submission,
            reconcile_order_submission,
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
                tauri::async_runtime::block_on(async {
                    state.scheduler.shutdown().await;
                    if let Err(error) = state.scheduler.flush_klines_for_shutdown().await {
                        tracing::error!(%error, "final kline flush failed");
                    }
                });
            }
        });
}
#[cfg(test)]
mod capability_tests {
    use tauri::ipc::Origin;

    #[test]
    fn main_window_can_force_close_after_chart_workspace_flush() {
        let mut context: tauri::Context<tauri::Wry> = tauri::generate_context!(test = true);

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

    // Catches granting the plugin control surface to another window or remote content.
    #[test]
    fn plugin_commands_are_available_only_to_the_local_main_webview() {
        let mut context: tauri::Context<tauri::Wry> = tauri::generate_context!(test = true);
        let authority = context.runtime_authority_mut();
        let remote = Origin::Remote {
            url: "https://example.invalid".parse().unwrap(),
        };

        for command in [
            "get_plugin_catalog",
            "reload_plugin_catalog",
            "set_plugin_enabled",
            "prepare_local_manifest_import",
            "cancel_local_manifest_import",
            "commit_local_manifest_import",
            "remove_managed_local_plugin",
        ] {
            assert!(
                authority
                    .resolve_access(command, "main", "main", &Origin::Local)
                    .is_some(),
                "local main must resolve {command}"
            );
            assert!(
                authority
                    .resolve_access(command, "main", "secondary", &Origin::Local)
                    .is_none(),
                "another webview in the main window must not resolve {command}"
            );
            assert!(
                authority
                    .resolve_access(command, "secondary", "secondary", &Origin::Local)
                    .is_none(),
                "another window must not resolve {command}"
            );
            assert!(
                authority
                    .resolve_access(command, "main", "main", &remote)
                    .is_none(),
                "remote content must not resolve {command}"
            );
        }
        assert!(
            authority
                .resolve_access("plugin:dialog|open", "main", "main", &Origin::Local)
                .is_none(),
            "the Rust-owned picker must not grant frontend dialog open access"
        );
    }

    // Catches wildcard or path-scoped grants expanding this fixed IPC surface.
    #[test]
    fn plugin_capability_grants_exactly_the_seven_fixed_commands_without_scope() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/plugin-runtime.json")).unwrap();
        assert_eq!(capability["webviews"], serde_json::json!(["main"]));
        assert!(capability.get("windows").is_none());
        assert!(capability.get("remote").is_none());
        assert!(capability.get("scope").is_none());
        assert_eq!(
            capability["permissions"],
            serde_json::json!([
                "allow-get-plugin-catalog",
                "allow-reload-plugin-catalog",
                "allow-set-plugin-enabled",
                "allow-prepare-local-manifest-import",
                "allow-cancel-local-manifest-import",
                "allow-commit-local-manifest-import",
                "allow-remove-managed-local-plugin"
            ])
        );
    }

    // Catches scoped/remote removal authority or plugin-runtime exposing backend-only plugins.
    #[test]
    fn remove_permission_has_no_scope_and_plugin_runtime_has_no_fs_shell_dialog_or_remote_grant() {
        let permission_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("permissions/autogenerated/remove_managed_local_plugin.toml");
        let permission: toml::Value =
            toml::from_str(&std::fs::read_to_string(permission_path).unwrap()).unwrap();
        assert_eq!(
            permission.as_table().unwrap().keys().collect::<Vec<_>>(),
            ["permission"]
        );
        let entries = permission["permission"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry.get("scope").is_none()));
        assert_eq!(
            entries[0]["identifier"].as_str(),
            Some("allow-remove-managed-local-plugin")
        );
        assert_eq!(
            entries[0]["commands"]["allow"]
                .as_array()
                .unwrap()
                .as_slice(),
            [toml::Value::String("remove_managed_local_plugin".into())]
        );
        assert_eq!(
            entries[1]["identifier"].as_str(),
            Some("deny-remove-managed-local-plugin")
        );
        assert_eq!(
            entries[1]["commands"]["deny"]
                .as_array()
                .unwrap()
                .as_slice(),
            [toml::Value::String("remove_managed_local_plugin".into())]
        );

        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/plugin-runtime.json")).unwrap();
        assert!(capability.get("remote").is_none());
        let identifiers = capability["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|permission| {
                permission
                    .as_str()
                    .or_else(|| {
                        permission
                            .get("identifier")
                            .and_then(serde_json::Value::as_str)
                    })
                    .expect("capability permission identifier")
            })
            .collect::<Vec<_>>();
        assert!(
            identifiers
                .iter()
                .all(|identifier| !identifier.starts_with("dialog:")
                    && !identifier.starts_with("fs:")
                    && !identifier.starts_with("shell:")
                    && !identifier.starts_with("opener:")),
            "dialog/fs/shell/opener grant found in plugin-runtime capability: {identifiers:?}"
        );
    }
}
