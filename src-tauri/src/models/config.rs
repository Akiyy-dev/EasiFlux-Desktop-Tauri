use serde::{Deserialize, Serialize};

pub const DEFAULT_KLINE_LIMIT: u32 = 200;
pub const DEFAULT_DEPTH_LIMIT: u32 = 20;

pub const APP_NAME: &str = "EasiFlux Desktop";
pub const APP_ORG: &str = "EasiFlux";
pub const DEFAULT_BASE_URL: &str = "https://api.easicoin.io";
pub const DEFAULT_WS_PUBLIC_URL: &str = "wss://ws.easicoin.io/contract/public/v1";
pub const DEFAULT_WS_PRIVATE_URL: &str = "wss://ws.easicoin.io/contract/private/v1";
pub const DEFAULT_SYMBOL: &str = "BTCUSDT";
pub const KEYRING_SERVICE: &str = "easiflux_desktop_tauri";
pub const CONFIG_FILENAME: &str = "config.toml";

pub const DEFAULT_WATCHLIST: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT"];
pub const KLINE_INTERVALS: &[&str] = &["1", "5", "15", "60", "240", "D"];
pub const RECV_WINDOW_MS: u64 = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Dark,
    Light,
}

impl Default for ThemeMode {
    fn default() -> Self {
        Self::Dark
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCredential {
    pub api_key: String,
    pub api_secret: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_label")]
    pub label: String,
}

fn default_base_url() -> String {
    DEFAULT_BASE_URL.to_string()
}

fn default_label() -> String {
    "default".to_string()
}

fn default_ws_public_url() -> String {
    DEFAULT_WS_PUBLIC_URL.to_string()
}

fn default_ws_private_url() -> String {
    DEFAULT_WS_PRIVATE_URL.to_string()
}

impl Default for ApiCredential {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_secret: String::new(),
            base_url: DEFAULT_BASE_URL.to_string(),
            label: "default".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub active_symbol: String,
    pub active_account_id: String,
    pub watchlist_symbols: Vec<String>,
    pub theme: ThemeMode,
    pub kline_interval: String,
    pub use_websocket: bool,
    #[serde(default = "default_ws_public_url")]
    pub ws_public_url: String,
    #[serde(default = "default_ws_private_url")]
    pub ws_private_url: String,
    pub ticker_poll_interval: f64,
    pub window_width: u32,
    pub window_height: u32,
    pub accounts: Vec<String>,
    pub risk_enabled: bool,
    pub risk_max_order_qty: String,
    pub risk_max_price_deviation_pct: String,
    pub risk_max_daily_orders: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            active_symbol: DEFAULT_SYMBOL.to_string(),
            active_account_id: "default".to_string(),
            watchlist_symbols: DEFAULT_WATCHLIST.iter().map(|s| s.to_string()).collect(),
            theme: ThemeMode::Dark,
            kline_interval: "1".to_string(),
            use_websocket: true,
            ws_public_url: DEFAULT_WS_PUBLIC_URL.to_string(),
            ws_private_url: DEFAULT_WS_PRIVATE_URL.to_string(),
            ticker_poll_interval: 5.0,
            window_width: 1400,
            window_height: 900,
            accounts: vec!["default".to_string()],
            risk_enabled: true,
            risk_max_order_qty: "100".to_string(),
            risk_max_price_deviation_pct: "5".to_string(),
            risk_max_daily_orders: 500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskConfig {
    pub max_order_qty: String,
    pub max_price_deviation_pct: String,
    pub max_daily_orders: u32,
    pub enabled: bool,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_order_qty: "100".to_string(),
            max_price_deviation_pct: "5".to_string(),
            max_daily_orders: 500,
            enabled: true,
        }
    }
}

impl From<&AppConfig> for RiskConfig {
    fn from(config: &AppConfig) -> Self {
        Self {
            max_order_qty: config.risk_max_order_qty.clone(),
            max_price_deviation_pct: config.risk_max_price_deviation_pct.clone(),
            max_daily_orders: config.risk_max_daily_orders,
            enabled: config.risk_enabled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveCredentialRequest {
    pub account_id: String,
    pub api_key: String,
    pub api_secret: String,
    pub base_url: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSettings {
    pub account_id: String,
    pub api_key: String,
    pub api_secret: String,
    pub base_url: String,
    pub use_websocket: bool,
}
