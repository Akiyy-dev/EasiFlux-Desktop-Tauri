use std::sync::Arc;

use tokio::sync::RwLock;

use crate::api::ApiClient;
use crate::events::EventEmitter;
use crate::models::config::AppConfig;
use crate::plugin::PluginRegistry;
use crate::services::{
    AccountService, AnalyticsService, ConnectionService, MarketService, RiskService, TradingService,
};
use crate::storage::{CacheStore, ConfigStore, TradeLogStore};
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
}

impl AppState {
    pub fn new(app: tauri::AppHandle) -> crate::error::AppResult<Self> {
        let config_store = ConfigStore::new();
        let loaded = config_store.load()?;
        let risk_config = crate::models::config::RiskConfig::from(&loaded);
        let config = Arc::new(RwLock::new(loaded));

        let api = Arc::new(ApiClient::new());
        let cache = Arc::new(CacheStore::new());
        let trade_log = Arc::new(TradeLogStore::new());
        let emitter = EventEmitter::new(app);

        let time_sync = api.time_sync();
        let ws = Arc::new(WsManager::new(emitter.clone(), time_sync));

        let market = Arc::new(MarketService::new(api.clone(), cache.clone(), emitter.clone()));
        ws.set_market(market.clone());
        let connection = Arc::new(ConnectionService::new(
            api.clone(),
            ws,
            market.clone(),
            config.clone(),
            emitter.clone(),
        ));
        let risk = Arc::new(RwLock::new(RiskService::new(risk_config)));
        let trading = Arc::new(TradingService::new(
            api.clone(),
            risk.clone(),
            trade_log.clone(),
            cache.clone(),
            emitter.clone(),
        ));
        let account = Arc::new(AccountService::new(api.clone(), emitter.clone()));
        let analytics = Arc::new(AnalyticsService::new());
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
        })
    }
}
