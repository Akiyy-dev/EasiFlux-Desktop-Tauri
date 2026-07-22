use super::super::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::storage::RiskUsageStore;

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);
pub(super) const NOW_MS: u64 = 1_784_606_400_000;

pub(super) fn test_path(label: &str) -> PathBuf {
    let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "easiflux-risk-command-{label}-{}-{sequence}",
        std::process::id()
    ))
}

pub(super) fn request(timezone: &str) -> UpdateRiskConfigRequest {
    UpdateRiskConfigRequest {
        enabled: true,
        max_order_qty: " 25.5 ".into(),
        max_price_deviation_pct: " 0 ".into(),
        max_daily_orders: 20,
        trading_day_timezone: format!(" {timezone} "),
    }
}

pub(super) fn runtime(
    config: AppConfig,
    ledger: &Path,
) -> (Arc<RwLock<AppConfig>>, Arc<RwLock<RiskService>>) {
    let risk_config = RiskConfig::from(&config);
    (
        Arc::new(RwLock::new(config)),
        Arc::new(RwLock::new(RiskService::with_store(
            risk_config,
            RiskUsageStore::with_path(ledger.to_path_buf()),
        ))),
    )
}
