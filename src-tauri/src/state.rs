use std::sync::Arc;

use tokio::sync::RwLock;

use crate::api::ApiClient;
use crate::events::EventEmitter;
use crate::models::config::{AppConfig, EnvironmentStatus};
use crate::plugin::PluginRegistry;
use crate::services::{
    AccountService, AnalyticsService, ConnectionService, DailyPnlService, MarketService,
    RiskService, SchedulerService, TimeService, TradingService,
};
use crate::storage::{CacheStore, ConfigStore, KlineStore, TradeLogStore};
use crate::ws::WsManager;

pub struct AppState {
    pub config_store: ConfigStore,
    pub config: Arc<RwLock<AppConfig>>,
    pub api: Arc<ApiClient>,
    pub cache: Arc<CacheStore>,
    pub trade_log: Arc<TradeLogStore>,
    pub connection: Arc<ConnectionService>,
    pub market: Arc<MarketService>,
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
}

impl AppState {
    pub fn new(app: tauri::AppHandle) -> crate::error::AppResult<Self> {
        let config_store = ConfigStore::new();
        let loaded = config_store.load()?;
        let risk_config = crate::models::config::RiskConfig::from(&loaded);
        let config = Arc::new(RwLock::new(loaded));

        let api = Arc::new(ApiClient::new());
        let cache = Arc::new(CacheStore::new());
        let kline_store = Arc::new(KlineStore::new());
        let trade_log = Arc::new(TradeLogStore::new());
        let analytics = Arc::new(AnalyticsService::new(api.clone()));
        let emitter = EventEmitter::new(app.clone(), analytics.clone());

        let time_sync = api.time_sync();
        let ws = Arc::new(WsManager::new(emitter.clone(), time_sync.clone()));
        let time = Arc::new(TimeService::new(time_sync, api.clone(), emitter.clone()));

        let market = Arc::new(MarketService::new(
            api.clone(),
            cache.clone(),
            kline_store,
            emitter.clone(),
            time.clone(),
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
        ));
        let account = Arc::new(AccountService::new(api.clone(), emitter.clone()));
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
            trading.clone(),
            daily_pnl.clone(),
            connection.clone(),
            ws,
            config.clone(),
            emitter.clone(),
            api.clone(),
            environment_status.clone(),
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
        })
    }
}
