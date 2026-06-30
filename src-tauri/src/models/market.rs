use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ticker {
    pub symbol: String,
    pub last_price: String,
    pub bid_price: String,
    pub ask_price: String,
    pub volume_24h: String,
    pub change_24h_pct: String,
}

impl Default for Ticker {
    fn default() -> Self {
        Self {
            symbol: String::new(),
            last_price: "0".to_string(),
            bid_price: "0".to_string(),
            ask_price: "0".to_string(),
            volume_24h: "0".to_string(),
            change_24h_pct: "0".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepthLevel {
    pub price: String,
    pub qty: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Depth {
    pub symbol: String,
    pub bids: Vec<DepthLevel>,
    pub asks: Vec<DepthLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Kline {
    pub symbol: String,
    pub interval: String,
    pub open_time: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketSnapshot {
    pub symbol: String,
    pub ticker: Option<Ticker>,
    pub depth: Option<Depth>,
    pub klines: Vec<Kline>,
}
