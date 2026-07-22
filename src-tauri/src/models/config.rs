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

/// Normalize account id: trim whitespace, fall back to `"default"` when empty.
pub fn normalize_account_id(account_id: &str) -> String {
    let trimmed = account_id.trim();
    if trimmed.is_empty() {
        "default".to_string()
    } else {
        trimmed.to_string()
    }
}

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

impl ApiCredential {
    pub fn normalize(mut self) -> Self {
        self.api_key = self.api_key.trim().to_string();
        self.api_secret = self.api_secret.trim().to_string();
        self.base_url = self.base_url.trim().trim_end_matches('/').to_string();
        if self.base_url.is_empty() {
            self.base_url = DEFAULT_BASE_URL.to_string();
        }
        self.label = self.label.trim().to_string();
        self
    }

    pub fn is_valid(&self) -> bool {
        !self.api_key.is_empty() && !self.api_secret.is_empty()
    }

    pub fn has_secret(&self) -> bool {
        !self.api_secret.is_empty()
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
    #[serde(default = "default_trading_day_timezone")]
    pub trading_day_timezone: String,
}

fn default_trading_day_timezone() -> String {
    crate::models::time::DEFAULT_TRADING_DAY_TIMEZONE.to_string()
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
            ticker_poll_interval: 1.0,
            window_width: 1400,
            window_height: 900,
            accounts: vec!["default".to_string()],
            risk_enabled: true,
            risk_max_order_qty: "100".to_string(),
            risk_max_price_deviation_pct: "5".to_string(),
            risk_max_daily_orders: 500,
            trading_day_timezone: crate::models::time::DEFAULT_TRADING_DAY_TIMEZONE.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskConfig {
    pub max_order_qty: String,
    pub max_price_deviation_pct: String,
    pub max_daily_orders: u32,
    pub trading_day_timezone: String,
    pub enabled: bool,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_order_qty: "100".to_string(),
            max_price_deviation_pct: "5".to_string(),
            max_daily_orders: 500,
            trading_day_timezone: crate::models::time::DEFAULT_TRADING_DAY_TIMEZONE.to_string(),
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
            trading_day_timezone: config.trading_day_timezone.clone(),
            enabled: config.risk_enabled,
        }
    }
}

pub fn environment_label(base_url: &str) -> &'static str {
    match url::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_string()))
        .as_deref()
    {
        Some("api.easicoin.io") => "正式",
        Some(_) => "开发",
        None if base_url.contains("api.easicoin.io") => "正式",
        None => "未知",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentStatus {
    pub base_url: String,
    pub label: String,
    pub reachable: bool,
    pub checked_at: u64,
    pub error: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_account_id_empty_to_default() {
        assert_eq!(normalize_account_id(""), "default");
        assert_eq!(normalize_account_id("   "), "default");
        assert_eq!(normalize_account_id("main"), "main");
    }
}
