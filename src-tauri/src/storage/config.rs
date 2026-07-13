use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::models::config::{
    AppConfig, ThemeMode, DEFAULT_WS_PRIVATE_URL, DEFAULT_WS_PUBLIC_URL, APP_NAME, CONFIG_FILENAME,
};

#[derive(Debug, Serialize, Deserialize)]
struct TomlConfig {
    #[serde(default = "default_symbol")]
    active_symbol: String,
    #[serde(default = "default_account")]
    active_account_id: String,
    #[serde(default = "default_watchlist")]
    watchlist_symbols: Vec<String>,
    #[serde(default)]
    theme: String,
    #[serde(default = "default_interval")]
    kline_interval: String,
    #[serde(default = "default_true")]
    use_websocket: bool,
    #[serde(default = "default_ws_public")]
    ws_public_url: String,
    #[serde(default = "default_ws_private")]
    ws_private_url: String,
    #[serde(default = "default_poll")]
    ticker_poll_interval: f64,
    #[serde(default = "default_width")]
    window_width: u32,
    #[serde(default = "default_height")]
    window_height: u32,
    #[serde(default = "default_accounts")]
    accounts: Vec<String>,
    #[serde(default = "default_true")]
    risk_enabled: bool,
    #[serde(default = "default_qty")]
    risk_max_order_qty: String,
    #[serde(default = "default_deviation")]
    risk_max_price_deviation_pct: String,
    #[serde(default = "default_daily")]
    risk_max_daily_orders: u32,
    #[serde(default = "default_trading_day_timezone")]
    trading_day_timezone: String,
}

fn default_symbol() -> String {
    "BTCUSDT".into()
}
fn default_account() -> String {
    "default".into()
}
fn default_watchlist() -> Vec<String> {
    vec![
        "BTCUSDT".into(),
        "ETHUSDT".into(),
        "SOLUSDT".into(),
        "XRPUSDT".into(),
    ]
}
fn default_interval() -> String {
    "1".into()
}
fn default_true() -> bool {
    true
}
fn default_poll() -> f64 {
    1.0
}
fn default_ws_public() -> String {
    DEFAULT_WS_PUBLIC_URL.to_string()
}
fn default_ws_private() -> String {
    DEFAULT_WS_PRIVATE_URL.to_string()
}
fn default_width() -> u32 {
    1400
}
fn default_height() -> u32 {
    900
}
fn default_accounts() -> Vec<String> {
    vec!["default".into()]
}
fn default_qty() -> String {
    "100".into()
}
fn default_deviation() -> String {
    "5".into()
}
fn default_daily() -> u32 {
    500
}
fn default_trading_day_timezone() -> String {
    crate::models::time::DEFAULT_TRADING_DAY_TIMEZONE.to_string()
}

impl From<TomlConfig> for AppConfig {
    fn from(t: TomlConfig) -> Self {
        AppConfig {
            active_symbol: t.active_symbol,
            active_account_id: t.active_account_id,
            watchlist_symbols: t.watchlist_symbols,
            theme: if t.theme == "light" {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            },
            kline_interval: t.kline_interval,
            use_websocket: t.use_websocket,
            ws_public_url: t.ws_public_url,
            ws_private_url: t.ws_private_url,
            ticker_poll_interval: t.ticker_poll_interval,
            window_width: t.window_width,
            window_height: t.window_height,
            accounts: t.accounts,
            risk_enabled: t.risk_enabled,
            risk_max_order_qty: t.risk_max_order_qty,
            risk_max_price_deviation_pct: t.risk_max_price_deviation_pct,
            risk_max_daily_orders: t.risk_max_daily_orders,
            trading_day_timezone: t.trading_day_timezone,
        }
    }
}

impl From<&AppConfig> for TomlConfig {
    fn from(c: &AppConfig) -> Self {
        TomlConfig {
            active_symbol: c.active_symbol.clone(),
            active_account_id: c.active_account_id.clone(),
            watchlist_symbols: c.watchlist_symbols.clone(),
            theme: format!("{:?}", c.theme).to_lowercase(),
            kline_interval: c.kline_interval.clone(),
            use_websocket: c.use_websocket,
            ws_public_url: c.ws_public_url.clone(),
            ws_private_url: c.ws_private_url.clone(),
            ticker_poll_interval: c.ticker_poll_interval,
            window_width: c.window_width,
            window_height: c.window_height,
            accounts: c.accounts.clone(),
            risk_enabled: c.risk_enabled,
            risk_max_order_qty: c.risk_max_order_qty.clone(),
            risk_max_price_deviation_pct: c.risk_max_price_deviation_pct.clone(),
            risk_max_daily_orders: c.risk_max_daily_orders,
            trading_day_timezone: c.trading_day_timezone.clone(),
        }
    }
}

pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new() -> Self {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(APP_NAME);
        let _ = fs::create_dir_all(&dir);
        Self {
            path: dir.join(CONFIG_FILENAME),
        }
    }

    pub fn load(&self) -> AppResult<AppConfig> {
        if !self.path.exists() {
            return Ok(AppConfig::default());
        }
        let text = fs::read_to_string(&self.path)?;
        let toml_cfg: TomlConfig = toml::from_str(&text)?;
        Ok(toml_cfg.into())
    }

    pub fn save(&self, config: &AppConfig) -> AppResult<()> {
        let toml_cfg = TomlConfig::from(config);
        let text = toml::to_string_pretty(&toml_cfg)?;
        fs::write(&self.path, text)?;
        Ok(())
    }
}

impl Default for ConfigStore {
    fn default() -> Self {
        Self::new()
    }
}
