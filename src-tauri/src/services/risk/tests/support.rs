use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::models::config::RiskConfig;
use crate::models::trading::PlaceOrderRequest;
use crate::storage::RiskUsageStore;

use super::super::RiskService;

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) const SHANGHAI_NOON: u64 = 1_784_606_400_000;
pub(super) const BEFORE_SHANGHAI_MIDNIGHT: u64 = 1_784_649_599_000;
pub(super) const AFTER_SHANGHAI_MIDNIGHT: u64 = 1_784_649_601_000;

pub(super) fn test_path(label: &str) -> PathBuf {
    let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "easiflux-risk-service-{label}-{}-{sequence}.toml",
        std::process::id()
    ))
}

pub(super) fn cleanup_test_files(path: &Path) {
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}.bak", path.display())),
        PathBuf::from(format!("{}.tmp", path.display())),
    ] {
        let _ = std::fs::remove_file(candidate);
    }
}

pub(super) fn market_order(qty: &str) -> PlaceOrderRequest {
    PlaceOrderRequest {
        symbol: "BTCUSDT".into(),
        side: "Buy".into(),
        order_type: "Market".into(),
        qty: qty.into(),
        position_idx: 0,
        price: None,
        time_in_force: None,
        order_link_id: None,
        reduce_only: None,
    }
}

pub(super) fn limit_order(price: &str) -> PlaceOrderRequest {
    PlaceOrderRequest {
        order_type: "Limit".into(),
        price: Some(price.into()),
        ..market_order("1")
    }
}

pub(super) fn service_with_limit(path: &Path, max_daily_orders: u32) -> RiskService {
    RiskService::with_store(
        RiskConfig {
            max_daily_orders,
            ..RiskConfig::default()
        },
        RiskUsageStore::with_path(path.to_path_buf()),
    )
}
