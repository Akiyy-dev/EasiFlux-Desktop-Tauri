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
    pub mark_price: String,
    pub high_24h: String,
    pub low_24h: String,
    pub funding_rate: String,
    pub funding_rate_updated_at: Option<u64>,
    pub funding_rate_error: Option<String>,
    pub next_funding_time: Option<i64>,
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
            mark_price: String::new(),
            high_24h: String::new(),
            low_24h: String::new(),
            funding_rate: String::new(),
            funding_rate_updated_at: None,
            funding_rate_error: None,
            next_funding_time: None,
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
