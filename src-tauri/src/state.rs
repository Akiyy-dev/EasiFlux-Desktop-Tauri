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
    NotificationStorageFailureReporter,
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
    storage_failure_reporter: NotificationStorageFailureReporter,
) -> Arc<NotificationRuntime> {
    Arc::new(
        match NotificationService::load_with_reporter(
            store,
            configured_accounts,
            now_ms,
            emit_changed,
            storage_failure_reporter,
        ) {
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

fn configured_notification_accounts(config: &AppConfig) -> Vec<String> {
    crate::services::account_profiles::normalize_account_ids(
        &config.accounts,
        &config.active_account_id,
    )
}

impl AppState {
    pub fn new(app: tauri::AppHandle) -> crate::error::AppResult<Self> {
        let config_store = ConfigStore::new();
        let loaded = config_store.load()?;
        let configured_accounts = configured_notification_accounts(&loaded);
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
            NotificationStorageFailureReporter::new(emitter.clone()),
        );
        let session_notification_observer = SessionNotificationObserver::new(
            notification.clone(),
            config.clone(),
            account_lifecycle.clone(),
            emitter.clone(),
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

    use crate::events::EventEmitter;
    use crate::models::notification::{
        NotificationAction, NotificationCategory, NotificationChangedEvent, NotificationContent,
        NotificationEntity, NotificationEntityType, NotificationKind, NotificationRecord,
        NotificationScalar, NotificationScope, NotificationSeverity,
    };
    use crate::services::notification::{
        NotificationEmitter, NotificationRuntime, NotificationStorageFailureReporter,
    };
    use crate::storage::notification_store::{
        NotificationFileV1, NotificationPartition, NotificationSourceEventIndexEntry,
        NotificationStore,
    };

    use super::{configured_notification_accounts, initialize_notification_runtime};

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

    fn storage_failure_reporter() -> NotificationStorageFailureReporter {
        NotificationStorageFailureReporter::new(EventEmitter::new_test(Arc::new(Mutex::new(
            Vec::new(),
        ))))
    }

    fn startup_record(
        number: u128,
        scope: NotificationScope,
        source_event_id: &str,
    ) -> NotificationRecord {
        let (category, kind, severity, content, entity, action) = match &scope {
            NotificationScope::Global => (
                NotificationCategory::ConnectionSystem,
                NotificationKind::ConnectionUnavailable,
                NotificationSeverity::Warning,
                NotificationContent::new(
                    "connection.unavailable",
                    [("channel", NotificationScalar::String("api".into()))],
                    "连接不可用",
                    "交易连接暂时不可用，请检查网络或稍后重试。",
                )
                .unwrap(),
                None,
                Some(NotificationAction::OpenGeneralSettings),
            ),
            NotificationScope::Account { .. } => (
                NotificationCategory::Trading,
                NotificationKind::OrderFilled,
                NotificationSeverity::Success,
                NotificationContent::new(
                    "order.filled",
                    [("orderId", NotificationScalar::String("order-1".into()))],
                    "订单已成交",
                    "订单已完全成交，请前往交易页查看。",
                )
                .unwrap(),
                Some(NotificationEntity {
                    entity_type: NotificationEntityType::Order,
                    id: "order-1".into(),
                }),
                Some(NotificationAction::OpenTrading {
                    order_id: Some("order-1".into()),
                }),
            ),
        };
        NotificationRecord {
            id: format!("00000000-0000-4000-8000-{number:012x}"),
            scope,
            category,
            kind,
            severity,
            content,
            entity,
            action,
            source_event_id: Some(source_event_id.into()),
            dedupe_key: format!("{source_event_id}-dedupe"),
            occurrence_count: 1,
            created_at_ms: 1_700_000_000_000,
            updated_at_ms: 1_700_000_000_000,
            read_at_ms: None,
        }
    }

    #[tokio::test]
    async fn startup_normalizes_configured_and_active_accounts_before_orphan_prune() {
        let path = test_path("normalized-configured-accounts");
        let global = NotificationScope::Global;
        let listed = NotificationScope::Account {
            account_id: "listed".into(),
        };
        let active = NotificationScope::Account {
            account_id: "live-active".into(),
        };
        let orphan = NotificationScope::Account {
            account_id: "orphan".into(),
        };
        let records = vec![
            startup_record(1, global.clone(), "global-source"),
            startup_record(2, listed.clone(), "listed-source"),
            startup_record(3, active.clone(), "active-source"),
            startup_record(4, orphan.clone(), "orphan-source"),
        ];
        let source_event_index = records
            .iter()
            .map(|record| NotificationSourceEventIndexEntry {
                scope: record.scope.clone(),
                source_event_id: record.source_event_id.clone().unwrap(),
                notification_id: record.id.clone(),
            })
            .collect();
        NotificationStore::with_path(path.clone())
            .save(&NotificationFileV1 {
                schema_version: 1,
                revision: 12,
                source_event_index,
                partitions: vec![
                    NotificationPartition {
                        scope: global.clone(),
                        items: vec![records[0].clone()],
                    },
                    NotificationPartition {
                        scope: listed.clone(),
                        items: vec![records[1].clone()],
                    },
                    NotificationPartition {
                        scope: active.clone(),
                        items: vec![records[2].clone()],
                    },
                    NotificationPartition {
                        scope: orphan,
                        items: vec![records[3].clone()],
                    },
                ],
            })
            .unwrap();
        let config = crate::models::config::AppConfig {
            accounts: vec![" listed ".into(), "listed".into(), " ".into()],
            active_account_id: " live-active ".into(),
            ..Default::default()
        };
        let configured_accounts = configured_notification_accounts(&config);
        assert_eq!(configured_accounts, vec!["live-active", "listed"]);
        let (emitter, events) = capture_emitter();

        let runtime = initialize_notification_runtime(
            NotificationStore::with_path(path.clone()),
            &configured_accounts,
            1_700_000_000_100,
            emitter,
            storage_failure_reporter(),
        );

        let NotificationRuntime::Available(service) = runtime.as_ref() else {
            panic!("valid normalized startup history must remain available");
        };
        assert_eq!(service.revision().await, "13");
        assert_eq!(events.lock().unwrap().len(), 1);
        let persisted = NotificationStore::with_path(path.clone())
            .load()
            .unwrap()
            .file;
        assert_eq!(persisted.partitions.len(), 3);
        assert!(persisted
            .partitions
            .iter()
            .any(|partition| partition.scope == global));
        assert!(persisted
            .partitions
            .iter()
            .any(|partition| partition.scope == listed));
        assert!(persisted
            .partitions
            .iter()
            .any(|partition| partition.scope == active));
        assert_eq!(persisted.source_event_index.len(), 3);
        assert!(persisted
            .source_event_index
            .iter()
            .all(|entry| entry.source_event_id != "orphan-source"));
        drop(runtime);

        let (restart_emitter, restart_events) = capture_emitter();
        let restarted = initialize_notification_runtime(
            NotificationStore::with_path(path.clone()),
            &configured_accounts,
            1_700_000_000_200,
            restart_emitter,
            storage_failure_reporter(),
        );
        let NotificationRuntime::Available(restarted_service) = restarted.as_ref() else {
            panic!("cleaned history must remain available on restart");
        };
        assert_eq!(restarted_service.revision().await, "13");
        assert!(restart_events.lock().unwrap().is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
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
            storage_failure_reporter(),
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
            storage_failure_reporter(),
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
            storage_failure_reporter(),
        );

        let NotificationRuntime::Available(service) = runtime.as_ref() else {
            panic!("valid backup history must be available");
        };
        assert_eq!(service.revision().await, "5");
        assert!(events.lock().unwrap().is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
