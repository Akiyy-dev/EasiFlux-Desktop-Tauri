use std::collections::HashMap;
use std::sync::RwLock;

use crate::models::market::{Kline, Ticker};

pub struct CacheStore {
    tickers: RwLock<HashMap<String, Ticker>>,
    klines: RwLock<HashMap<String, Vec<Kline>>>,
    recent_symbols: RwLock<Vec<String>>,
}

impl CacheStore {
    pub fn new() -> Self {
        Self {
            tickers: RwLock::new(HashMap::new()),
            klines: RwLock::new(HashMap::new()),
            recent_symbols: RwLock::new(Vec::new()),
        }
    }

    pub fn set_ticker(&self, ticker: Ticker) {
        if let Ok(mut map) = self.tickers.write() {
            map.insert(ticker.symbol.clone(), ticker);
        }
    }

    pub fn get_ticker(&self, symbol: &str) -> Option<Ticker> {
        self.tickers.read().ok()?.get(symbol).cloned()
    }

    pub fn set_klines(&self, symbol: &str, interval: &str, klines: Vec<Kline>) {
        let key = format!("{}:{}", symbol, interval);
        if let Ok(mut map) = self.klines.write() {
            map.insert(key, klines);
        }
    }

    pub fn get_klines(&self, symbol: &str, interval: &str) -> Option<Vec<Kline>> {
        let key = format!("{}:{}", symbol, interval);
        self.klines.read().ok()?.get(&key).cloned()
    }

    pub fn touch_symbol(&self, symbol: &str) {
        if let Ok(mut recent) = self.recent_symbols.write() {
            recent.retain(|s| s != symbol);
            recent.insert(0, symbol.to_string());
            recent.truncate(20);
        }
    }
}

impl Default for CacheStore {
    fn default() -> Self {
        Self::new()
    }
}
