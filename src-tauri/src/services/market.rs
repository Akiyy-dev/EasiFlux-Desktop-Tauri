use std::sync::Arc;

use tokio::sync::RwLock;

use crate::api::{ApiClient, PublicApi};
use crate::error::AppResult;
use crate::events::EventEmitter;
use crate::models::market::{Depth, Kline, Ticker};
use crate::storage::CacheStore;

pub struct MarketService {
    api: Arc<ApiClient>,
    cache: Arc<CacheStore>,
    emitter: EventEmitter,
    active_symbol: Arc<RwLock<String>>,
    kline_interval: Arc<RwLock<String>>,
}

impl MarketService {
    pub fn new(api: Arc<ApiClient>, cache: Arc<CacheStore>, emitter: EventEmitter) -> Self {
        Self {
            api,
            cache,
            emitter,
            active_symbol: Arc::new(RwLock::new("BTCUSDT".into())),
            kline_interval: Arc::new(RwLock::new("1".into())),
        }
    }

    pub async fn set_active_symbol(&self, symbol: &str) {
        *self.active_symbol.write().await = symbol.to_string();
        self.cache.touch_symbol(symbol);
    }

    pub async fn active_symbol(&self) -> String {
        self.active_symbol.read().await.clone()
    }

    pub async fn set_kline_interval(&self, interval: &str) {
        *self.kline_interval.write().await = interval.to_string();
    }

    pub async fn fetch_ticker(&self, symbol: &str) -> AppResult<Ticker> {
        let ticker = PublicApi::ticker(&self.api, symbol).await?;
        self.cache.set_ticker(ticker.clone());
        self.emitter.emit_ticker(ticker.clone());
        Ok(ticker)
    }

    pub async fn fetch_depth(&self, symbol: &str) -> AppResult<Depth> {
        let depth = PublicApi::depth(&self.api, symbol, 20).await?;
        self.emitter.emit_depth(depth.clone());
        Ok(depth)
    }

    pub async fn fetch_klines(&self, symbol: &str, interval: &str) -> AppResult<Vec<Kline>> {
        let klines = PublicApi::klines(&self.api, symbol, interval, 200).await?;
        self.cache.set_klines(symbol, interval, klines.clone());
        self.emitter.emit_klines(&klines);
        Ok(klines)
    }

    pub async fn refresh_snapshot(&self, symbol: &str) -> AppResult<()> {
        let interval = self.kline_interval.read().await.clone();
        let (ticker, depth, klines) =
            PublicApi::market_snapshot(&self.api, symbol, &interval).await?;
        self.cache.set_ticker(ticker.clone());
        self.emitter.emit_ticker(ticker);
        self.emitter.emit_depth(depth);
        self.cache.set_klines(symbol, &interval, klines.clone());
        self.emitter.emit_klines(&klines);
        Ok(())
    }
}
