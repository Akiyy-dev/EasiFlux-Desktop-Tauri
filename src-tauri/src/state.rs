use std::sync::Arc;

use tokio::sync::RwLock;

use crate::api::ApiClient;
use crate::events::EventEmitter;
use crate::models::chart_workspace::ChartWorkspaceKey;
use crate::models::config::{AppConfig, EnvironmentStatus};
use crate::plugin::PluginRegistry;
use crate::services::{
    AccountLifecycleCoordinator, AccountService, AnalyticsService, ChartWorkspaceService,
    ConnectionService, DailyPnlService, MarketService, RiskService, SchedulerService, TimeService,
    TradingService,
};
use crate::storage::{CacheStore, ChartStateStore, ConfigStore, KlineStore, TradeLogStore};
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
    pub time: Arc<TimeService>,
    pub daily_pnl: Arc<DailyPnlService>,
    pub scheduler: Arc<SchedulerService>,
    pub environment_status: Arc<RwLock<EnvironmentStatus>>,
    pub account_lifecycle: Arc<AccountLifecycleCoordinator>,
}

impl AppState {
    pub fn new(app: tauri::AppHandle) -> crate::error::AppResult<Self> {
        let config_store = ConfigStore::new();
        let loaded = config_store.load()?;
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
        let emitter = EventEmitter::new(app.clone(), account_lifecycle.clone());

        let time_sync = api.time_sync();
        let ws = Arc::new(WsManager::new(emitter.clone(), time_sync.clone()));
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
            ws,
            config.clone(),
            emitter.clone(),
            api.clone(),
            environment_status.clone(),
            account_lifecycle.clone(),
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
            time,
            daily_pnl,
            scheduler,
            environment_status,
            account_lifecycle,
        })
    }
}
