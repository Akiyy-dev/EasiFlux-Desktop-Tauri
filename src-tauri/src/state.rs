use std::sync::Arc;

use tokio::sync::RwLock;

use crate::api::ApiClient;
use crate::events::EventEmitter;
use crate::models::chart_workspace::ChartWorkspaceKey;
use crate::models::config::{AppConfig, EnvironmentStatus};
use crate::plugin::PluginRegistry;
use crate::services::connection::SessionNotificationObserver;
use crate::services::notification::{
    NotificationEmitter, NotificationRuntime, NotificationService,
};
use crate::services::trading::OrderNotificationObserver;
use crate::services::{
    AccountLifecycleCoordinator, AccountService, AnalyticsService, ChartWorkspaceService,
    ConnectionService, DailyPnlService, MarketService, RiskService, SchedulerService, TimeService,
    TradingService,
};
use crate::storage::{
    CacheStore, ChartStateStore, ConfigStore, KlineStore, NotificationStore, TradeLogStore,
};
use crate::ws::WsManager;

pub struct AppState {
    pub config_store: ConfigStore,
    pub config: Arc<RwLock<AppConfig>>,
    pub api: Arc<ApiClient>,
    pub cache: Arc<CacheStore>,
    pub trade_log: Arc<TradeLogStore>,
    pub connection: Arc<ConnectionService>,
    pub market: Arc<MarketService>,
    pub chart_workspace: Arc<ChartWorkspaceService>,
    pub trading: Arc<TradingService>,
    pub account: Arc<AccountService>,
    pub risk: Arc<RwLock<RiskService>>,
    pub analytics: Arc<AnalyticsService>,
    pub plugins: Arc<RwLock<PluginRegistry>>,
    pub emitter: EventEmitter,
    pub notification: Arc<NotificationRuntime>,
    pub time: Arc<TimeService>,
    pub daily_pnl: Arc<DailyPnlService>,
    pub scheduler: Arc<SchedulerService>,
    pub ws: Arc<WsManager>,
    pub environment_status: Arc<RwLock<EnvironmentStatus>>,
    pub account_lifecycle: Arc<AccountLifecycleCoordinator>,
}

fn initialize_notification_runtime(
    store: NotificationStore,
    configured_accounts: &[String],
    now_ms: u64,
    emit_changed: NotificationEmitter,
) -> Arc<NotificationRuntime> {
    Arc::new(
        match NotificationService::load(store, configured_accounts, now_ms, emit_changed) {
            Ok(service) => NotificationRuntime::Available(Arc::new(service)),
            Err(error) => {
                tracing::warn!(
                    code = error.code(),
                    "notification center unavailable at startup"
                );
                NotificationRuntime::Unavailable(error.availability())
            }
        },
    )
}

impl AppState {
    pub fn new(app: tauri::AppHandle) -> crate::error::AppResult<Self> {
        let config_store = ConfigStore::new();
        let loaded = config_store.load()?;
        let configured_accounts = loaded.accounts.clone();
        let risk_config = crate::models::config::RiskConfig::from(&loaded);
        let initial_chart_context =
            ChartWorkspaceKey::parse(&loaded.active_symbol, &loaded.kline_interval)
                .map_err(crate::error::AppError::Config)?;
        let config = Arc::new(RwLock::new(loaded));

        let api = Arc::new(ApiClient::new());
        let cache = Arc::new(CacheStore::new());
        let kline_store = Arc::new(KlineStore::new());
        kline_store.load_range(&initial_chart_context, None, None, 200)?;
        let trade_log = Arc::new(TradeLogStore::new());
        let analytics = Arc::new(AnalyticsService::new(api.clone()));
        let account_lifecycle = Arc::new(AccountLifecycleCoordinator::new());
        let emitter = EventEmitter::new(app.clone());
        let notification_emitter: NotificationEmitter = Arc::new({
            let emitter = emitter.clone();
            move |event| emitter.emit_notification_changed(event)
        });
        let notification = initialize_notification_runtime(
            NotificationStore::new(),
            &configured_accounts,
            chrono::Utc::now().timestamp_millis().max(0) as u64,
            notification_emitter,
        );
        let session_notification_observer = SessionNotificationObserver::new(
            notification.clone(),
            config.clone(),
            account_lifecycle.clone(),
        );
        let auth_observer: crate::api::client::AuthFailureObserver = Arc::new({
            let observer = session_notification_observer.clone();
            move |context, failure| {
                let observer = observer.clone();
                Box::pin(async move {
                    observer
                        .observe_auth_failure_guarded(
                            &context,
                            failure,
                            chrono::Utc::now().timestamp_millis().max(0) as u64,
                        )
                        .await
                })
            }
        });
        api.set_auth_failure_observer(auth_observer);

        let time_sync = api.time_sync();
        let ws = Arc::new(WsManager::new(
            emitter.clone(),
            time_sync.clone(),
            OrderNotificationObserver::new(notification.clone()),
            config.clone(),
            account_lifecycle.clone(),
            session_notification_observer.clone(),
        ));
        let time = Arc::new(TimeService::new(time_sync, api.clone(), emitter.clone()));
        let chart_state_store = Arc::new(ChartStateStore::new());
        let chart_workspace = Arc::new(ChartWorkspaceService::new(
            kline_store.clone(),
            chart_state_store,
            Arc::new({
                let emitter = emitter.clone();
                move |message| emitter.emit_error(&message)
            }),
        ));

        let market = Arc::new(MarketService::new(
            api.clone(),
            cache.clone(),
            kline_store,
            emitter.clone(),
            time.clone(),
            account_lifecycle.clone(),
            initial_chart_context,
        ));
        ws.set_market(market.clone());
        let connection = Arc::new(ConnectionService::new(
            api.clone(),
            ws.clone(),
            market.clone(),
            config.clone(),
            emitter.clone(),
            time.clone(),
            account_lifecycle.clone(),
            session_notification_observer.clone(),
        ));
        let risk = Arc::new(RwLock::new(RiskService::new(risk_config)));
        let trading = Arc::new(TradingService::new(
            api.clone(),
            risk.clone(),
            trade_log.clone(),
            cache.clone(),
            emitter.clone(),
            time.clone(),
            analytics.clone(),
            notification.clone(),
        ));
        let account = Arc::new(AccountService::new(
            api.clone(),
            emitter.clone(),
            analytics.clone(),
        ));
        let daily_pnl = Arc::new(DailyPnlService::new(
            api.clone(),
            time.clone(),
            config.clone(),
            emitter.clone(),
        ));
        let environment_status = Arc::new(RwLock::new(EnvironmentStatus {
            base_url: crate::models::config::DEFAULT_BASE_URL.to_string(),
            label: "未知".to_string(),
            reachable: false,
            checked_at: 0,
            error: None,
        }));
        let scheduler = Arc::new(SchedulerService::new(
            time.clone(),
            market.clone(),
            chart_workspace.clone(),
            trading.clone(),
            daily_pnl.clone(),
            connection.clone(),
            ws.clone(),
            config.clone(),
            notification.clone(),
            emitter.clone(),
            api.clone(),
            environment_status.clone(),
            account_lifecycle.clone(),
            session_notification_observer,
        ));

        let plugins = Arc::new(RwLock::new(PluginRegistry::new()));

        Ok(Self {
            config_store,
            config,
            api,
            cache,
            trade_log,
            connection,
            market,
            chart_workspace,
            trading,
            account,
            risk,
            analytics,
            plugins,
            emitter,
            notification,
            time,
            daily_pnl,
            scheduler,
            ws,
            environment_status,
            account_lifecycle,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use crate::models::notification::{
        NotificationCategory, NotificationChangedEvent, NotificationContent, NotificationKind,
        NotificationRecord, NotificationScope, NotificationSeverity,
    };
    use crate::services::notification::{NotificationEmitter, NotificationRuntime};
    use crate::storage::notification_store::{
        NotificationFileV1, NotificationPartition, NotificationStore,
    };

    use super::initialize_notification_runtime;

    fn test_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!(
                "easiflux-notification-runtime-{label}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4(),
            ))
            .join("notifications.v1.json")
    }

    fn capture_emitter() -> (
        NotificationEmitter,
        Arc<Mutex<Vec<NotificationChangedEvent>>>,
    ) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        (
            Arc::new(move |event| {
                captured.lock().unwrap().push(event.clone());
                Ok(())
            }),
            events,
        )
    }

    #[tokio::test]
    async fn unsupported_schema_keeps_app_startup_non_fatal_and_commands_unavailable() {
        let path = test_path("future-schema");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            br#"{"schemaVersion":2,"revision":9,"partitions":[]}"#,
        )
        .unwrap();
        let (emitter, events) = capture_emitter();
        let unrelated_state = "still-initialized";

        let runtime = initialize_notification_runtime(
            NotificationStore::with_path(path.clone()),
            &["primary".into()],
            1_700_000_000_000,
            emitter,
        );

        match runtime.as_ref() {
            NotificationRuntime::Unavailable(availability) => {
                assert_eq!(availability.code(), "UNSUPPORTED_NOTIFICATION_SCHEMA");
            }
            NotificationRuntime::Available(_) => panic!("future schema must be unavailable"),
        }
        assert_eq!(unrelated_state, "still-initialized");
        assert!(events.lock().unwrap().is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn all_corrupt_candidates_recover_to_available_empty_without_created_event() {
        let path = test_path("all-corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"broken-main").unwrap();
        std::fs::write(format!("{}.tmp", path.display()), b"broken-temp").unwrap();
        std::fs::write(format!("{}.bak", path.display()), b"broken-backup").unwrap();
        let (emitter, events) = capture_emitter();

        let runtime = initialize_notification_runtime(
            NotificationStore::with_path(path.clone()),
            &["primary".into()],
            1_700_000_000_000,
            emitter,
        );

        let NotificationRuntime::Available(service) = runtime.as_ref() else {
            panic!("all-corrupt v1 history must recover to an available empty service");
        };
        assert_eq!(service.revision().await, "0");
        assert!(events.lock().unwrap().is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn recovered_history_becomes_available_without_replaying_created() {
        let path = test_path("backup-history");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"broken-main").unwrap();
        let backup = NotificationFileV1 {
            schema_version: 1,
            revision: 5,
            source_event_index: Vec::new(),
            partitions: vec![NotificationPartition {
                scope: NotificationScope::Global,
                items: vec![NotificationRecord {
                    id: "00000000-0000-4000-8000-000000000005".into(),
                    scope: NotificationScope::Global,
                    category: NotificationCategory::ConnectionSystem,
                    kind: NotificationKind::ConnectionUnavailable,
                    severity: NotificationSeverity::Error,
                    content: NotificationContent {
                        message_key: "connection.unavailable".into(),
                        params: BTreeMap::new(),
                        fallback_title: "连接不可用".into(),
                        fallback_body: "交易连接暂时不可用，请检查网络或稍后重试。".into(),
                    },
                    entity: None,
                    action: None,
                    source_event_id: None,
                    dedupe_key: "backup-history".into(),
                    occurrence_count: 1,
                    created_at_ms: 1_700_000_000_000,
                    updated_at_ms: 1_700_000_000_000,
                    read_at_ms: None,
                }],
            }],
        };
        std::fs::write(
            format!("{}.bak", path.display()),
            serde_json::to_vec(&backup).unwrap(),
        )
        .unwrap();
        let (emitter, events) = capture_emitter();

        let runtime = initialize_notification_runtime(
            NotificationStore::with_path(path.clone()),
            &["primary".into()],
            1_700_000_000_100,
            emitter,
        );

        let NotificationRuntime::Available(service) = runtime.as_ref() else {
            panic!("valid backup history must be available");
        };
        assert_eq!(service.revision().await, "5");
        assert!(events.lock().unwrap().is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
