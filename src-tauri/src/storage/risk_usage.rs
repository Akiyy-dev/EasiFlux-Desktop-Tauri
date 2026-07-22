use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::models::config::APP_NAME;

pub const RISK_USAGE_FILENAME: &str = "risk_usage.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskUsage {
    pub schema_version: u32,
    pub trading_day: String,
    pub timezone: String,
    pub occupied_orders: u32,
    pub updated_at_ms: u64,
}

pub struct RiskUsageStore {
    path: PathBuf,
}

impl RiskUsageStore {
    pub fn new() -> Self {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(APP_NAME);
        Self {
            path: dir.join(RISK_USAGE_FILENAME),
        }
    }

    pub(crate) fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> AppResult<Option<RiskUsage>> {
        if self.path.exists() {
            return read_usage(&self.path).map(Some);
        }

        let temp_path = sibling_path(&self.path, "tmp");
        if temp_path.exists() {
            return read_usage(&temp_path).map(Some);
        }

        let backup_path = sibling_path(&self.path, "bak");
        if backup_path.exists() {
            return Err(AppError::Storage(format!(
                "风控用量主账本缺失，且仅存在无法确认时效的备份: {}",
                backup_path.display()
            )));
        }

        Ok(None)
    }

    pub fn save(&self, usage: &RiskUsage) -> AppResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(storage_error)?;
        }

        let text =
            toml::to_string_pretty(usage).map_err(|error| AppError::Storage(error.to_string()))?;
        let temp_path = sibling_path(&self.path, "tmp");
        let backup_path = sibling_path(&self.path, "bak");

        let mut temp = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)
            .map_err(storage_error)?;
        temp.write_all(text.as_bytes()).map_err(storage_error)?;
        temp.sync_all().map_err(storage_error)?;
        drop(temp);

        if backup_path.exists() {
            fs::remove_file(&backup_path).map_err(storage_error)?;
        }

        let had_main = self.path.exists();
        if had_main {
            fs::rename(&self.path, &backup_path).map_err(storage_error)?;
        }

        if let Err(error) = fs::rename(&temp_path, &self.path) {
            if had_main && backup_path.exists() {
                let _ = fs::rename(&backup_path, &self.path);
            }
            return Err(storage_error(error));
        }

        Ok(())
    }
}

impl Default for RiskUsageStore {
    fn default() -> Self {
        Self::new()
    }
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}.{suffix}", path.display()))
}

fn storage_error(error: std::io::Error) -> AppError {
    AppError::Storage(error.to_string())
}

fn read_usage(path: &Path) -> AppResult<RiskUsage> {
    let text = fs::read_to_string(path).map_err(storage_error)?;
    toml::from_str(&text).map_err(|error| {
        AppError::Storage(format!("无法读取风控用量账本 {}: {error}", path.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path(label: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "easiflux-risk-usage-{label}-{}-{sequence}.toml",
            std::process::id()
        ))
    }

    fn cleanup_test_files(path: &Path) {
        for candidate in [
            path.to_path_buf(),
            sibling_path(path, "bak"),
            sibling_path(path, "tmp"),
        ] {
            let _ = std::fs::remove_file(candidate);
        }
    }

    fn sample_usage(occupied_orders: u32) -> RiskUsage {
        RiskUsage {
            schema_version: 1,
            trading_day: "2026-07-21".into(),
            timezone: "Asia/Shanghai".into(),
            occupied_orders,
            updated_at_ms: 1_753_113_600_000,
        }
    }

    #[test]
    fn round_trips_usage() {
        let path = test_path("round-trip");
        let store = RiskUsageStore::with_path(path.clone());
        let usage = sample_usage(7);

        store.save(&usage).unwrap();

        assert_eq!(store.load().unwrap(), Some(usage));
        cleanup_test_files(&path);
    }

    #[test]
    fn refuses_stale_backup_when_main_is_corrupt() {
        let path = test_path("backup");
        let store = RiskUsageStore::with_path(path.clone());
        let first = sample_usage(3);
        let second = sample_usage(4);

        store.save(&first).unwrap();
        store.save(&second).unwrap();
        std::fs::write(&path, "not toml").unwrap();

        assert!(store.load().is_err());
        cleanup_test_files(&path);
    }

    #[test]
    fn recovers_new_temp_when_main_is_missing() {
        let path = test_path("temp-recovery");
        let store = RiskUsageStore::with_path(path.clone());
        let first = sample_usage(3);
        let second = sample_usage(4);
        store.save(&first).unwrap();
        std::fs::write(
            sibling_path(&path, "tmp"),
            toml::to_string_pretty(&second).unwrap(),
        )
        .unwrap();
        std::fs::rename(&path, sibling_path(&path, "bak")).unwrap();

        assert_eq!(store.load().unwrap(), Some(second));
        cleanup_test_files(&path);
    }

    #[test]
    fn reports_error_when_all_candidates_are_corrupt() {
        let path = test_path("corrupt");
        std::fs::write(&path, "not toml").unwrap();

        assert!(RiskUsageStore::with_path(path.clone()).load().is_err());
        cleanup_test_files(&path);
    }

    #[test]
    fn refuses_backup_only_state() {
        let path = test_path("backup-only");
        let usage = sample_usage(2);
        std::fs::write(
            sibling_path(&path, "bak"),
            toml::to_string_pretty(&usage).unwrap(),
        )
        .unwrap();

        assert!(RiskUsageStore::with_path(path.clone()).load().is_err());
        cleanup_test_files(&path);
    }
}
