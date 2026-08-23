use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::models::config::{
    AppConfig, ThemeMode, APP_NAME, CONFIG_FILENAME, DEFAULT_WS_PRIVATE_URL, DEFAULT_WS_PUBLIC_URL,
};
use crate::models::notification::NotificationSettings;

use super::config_persistence;

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
    #[serde(default)]
    notification_settings: NotificationSettings,
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
            notification_settings: t.notification_settings,
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
            notification_settings: c.notification_settings,
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

    #[cfg(test)]
    pub(crate) fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> AppResult<AppConfig> {
        config_persistence::load(&self.path, |text| {
            let toml_cfg: TomlConfig = toml::from_str(text)?;
            Ok(toml_cfg.into())
        })
        .map(|config| config.unwrap_or_default())
    }

    pub fn save(&self, config: &AppConfig) -> AppResult<()> {
        let toml_cfg = TomlConfig::from(config);
        let text = toml::to_string_pretty(&toml_cfg)?;
        config_persistence::save(&self.path, text.as_bytes())
    }
}

impl Default for ConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new(label: &str) -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "easiflux-config-{label}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
        let mut value: OsString = path.as_os_str().to_os_string();
        value.push(suffix);
        PathBuf::from(value)
    }

    fn config(symbol: &str, width: u32) -> AppConfig {
        AppConfig {
            active_symbol: symbol.into(),
            window_width: width,
            ..AppConfig::default()
        }
    }

    fn write_valid_candidate(root: &Path, target: &Path, value: &AppConfig) {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let seed = root.join(format!("seed-{sequence}.toml"));
        ConfigStore::with_path(seed.clone()).save(value).unwrap();
        fs::rename(seed, target).unwrap();
    }

    #[test]
    fn roundtrip_persists_config() {
        let root = TestRoot::new("roundtrip");
        let store = ConfigStore::with_path(root.path().join("config.toml"));
        let expected = config("ETHUSDT", 1510);

        store.save(&expected).unwrap();

        let actual = store.load().unwrap();
        assert_eq!(actual.active_symbol, "ETHUSDT");
        assert_eq!(actual.window_width, 1510);
    }

    #[test]
    fn legacy_toml_defaults_notification_settings() {
        let legacy = r#"
active_symbol = "ETHUSDT"
active_account_id = "default"
watchlist_symbols = ["ETHUSDT"]
theme = "dark"
kline_interval = "1"
use_websocket = true
ticker_poll_interval = 1.0
window_width = 1400
window_height = 900
accounts = ["default"]
risk_enabled = true
risk_max_order_qty = "100"
risk_max_price_deviation_pct = "5"
risk_max_daily_orders = 500
"#;

        let config: AppConfig = toml::from_str::<TomlConfig>(legacy).unwrap().into();

        assert_eq!(
            config.notification_settings,
            crate::models::notification::NotificationSettings::default()
        );
    }

    #[test]
    fn notification_settings_round_trip() {
        let root = TestRoot::new("notification-settings");
        let store = ConfigStore::with_path(root.path().join("config.toml"));
        let mut expected = AppConfig::default();
        expected.notification_settings = crate::models::notification::NotificationSettings {
            trading_toast: false,
            risk_account_toast: true,
            connection_system_toast: false,
        };

        store.save(&expected).unwrap();

        assert_eq!(
            store.load().unwrap().notification_settings,
            expected.notification_settings
        );
    }

    #[test]
    fn failed_temp_write_preserves_readable_main() {
        let root = TestRoot::new("temp-failure");
        let main = root.path().join("config.toml");
        let temp = sidecar_path(&main, ".tmp");
        let store = ConfigStore::with_path(main);
        store.save(&config("BTCUSDT", 1400)).unwrap();
        fs::create_dir(&temp).unwrap();

        let result = store.save(&config("SOLUSDT", 1777));

        assert!(result.is_err());
        let recovered = store.load().unwrap();
        assert_eq!(recovered.active_symbol, "BTCUSDT");
        assert_eq!(recovered.window_width, 1400);
    }

    #[test]
    fn load_recovers_latest_config_from_temp_when_main_missing() {
        let root = TestRoot::new("temp-recovery");
        let main = root.path().join("config.toml");
        let temp = sidecar_path(&main, ".tmp");
        write_valid_candidate(root.path(), &temp, &config("SOLUSDT", 1666));
        let store = ConfigStore::with_path(main);

        let recovered = store.load().unwrap();

        assert_eq!(recovered.active_symbol, "SOLUSDT");
        assert_eq!(recovered.window_width, 1666);
    }

    #[test]
    fn load_recovers_config_from_backup_when_main_and_temp_missing() {
        let root = TestRoot::new("backup-recovery");
        let main = root.path().join("config.toml");
        let backup = sidecar_path(&main, ".bak");
        write_valid_candidate(root.path(), &backup, &config("XRPUSDT", 1444));
        let store = ConfigStore::with_path(main);

        let recovered = store.load().unwrap();

        assert_eq!(recovered.active_symbol, "XRPUSDT");
        assert_eq!(recovered.window_width, 1444);
    }

    #[test]
    fn load_skips_corrupt_main_and_recovers_from_valid_temp() {
        let root = TestRoot::new("corrupt-main");
        let main = root.path().join("config.toml");
        fs::write(&main, "broken = [toml").unwrap();
        write_valid_candidate(
            root.path(),
            &sidecar_path(&main, ".tmp"),
            &config("ETHUSDT", 1777),
        );
        let store = ConfigStore::with_path(main);

        let recovered = store.load().unwrap();

        assert_eq!(recovered.active_symbol, "ETHUSDT");
        assert_eq!(recovered.window_width, 1777);
    }

    #[test]
    fn load_returns_error_when_existing_candidates_are_all_invalid() {
        let root = TestRoot::new("all-invalid");
        let main = root.path().join("config.toml");
        fs::write(sidecar_path(&main, ".tmp"), "not = [valid").unwrap();
        fs::write(sidecar_path(&main, ".bak"), "also = [broken").unwrap();
        let store = ConfigStore::with_path(main);

        assert!(store.load().is_err());
    }

    #[test]
    fn second_save_promotes_new_config_and_preserves_same_directory_backup() {
        let root = TestRoot::new("second-save");
        let main = root.path().join("config.toml");
        let temp = sidecar_path(&main, ".tmp");
        let backup = sidecar_path(&main, ".bak");
        let store = ConfigStore::with_path(main.clone());
        store.save(&config("BTCUSDT", 1400)).unwrap();

        store.save(&config("ETHUSDT", 1888)).unwrap();

        let current = store.load().unwrap();
        assert_eq!(current.active_symbol, "ETHUSDT");
        assert_eq!(current.window_width, 1888);
        assert_eq!(temp.parent(), main.parent());
        assert_eq!(backup.parent(), main.parent());
        assert!(backup.exists());
        let previous = ConfigStore::with_path(backup).load().unwrap();
        assert_eq!(previous.active_symbol, "BTCUSDT");
        assert_eq!(previous.window_width, 1400);
    }

    #[test]
    fn load_prefers_valid_main_over_temp_and_backup() {
        let root = TestRoot::new("main-priority");
        let main = root.path().join("config.toml");
        let store = ConfigStore::with_path(main.clone());
        store.save(&config("BTCUSDT", 1400)).unwrap();
        write_valid_candidate(
            root.path(),
            &sidecar_path(&main, ".tmp"),
            &config("ETHUSDT", 1555),
        );
        write_valid_candidate(
            root.path(),
            &sidecar_path(&main, ".bak"),
            &config("SOLUSDT", 1666),
        );

        let selected = store.load().unwrap();

        assert_eq!(selected.active_symbol, "BTCUSDT");
        assert_eq!(selected.window_width, 1400);
    }
}
