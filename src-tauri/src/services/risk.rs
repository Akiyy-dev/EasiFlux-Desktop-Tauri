use std::sync::Mutex;

use rust_decimal::prelude::FromStr;
use rust_decimal::Decimal;

use crate::error::{AppError, AppResult};
use crate::models::config::RiskConfig;
use crate::models::trading::PlaceOrderRequest;
use crate::services::time::{resolve_trading_day_timezone, trading_day_key};
use crate::storage::{RiskUsage, RiskUsageStore};

#[derive(Debug, Clone)]
pub struct RiskReservation {
    trading_day: String,
    timezone: String,
    counted: bool,
}

impl RiskReservation {
    fn not_counted() -> Self {
        Self {
            trading_day: String::new(),
            timezone: String::new(),
            counted: false,
        }
    }

    fn counted(trading_day: String, timezone: String) -> Self {
        Self {
            trading_day,
            timezone,
            counted: true,
        }
    }
}

struct RiskUsageState {
    usage: Option<RiskUsage>,
    load_error: Option<String>,
}

pub struct RiskService {
    config: RiskConfig,
    store: RiskUsageStore,
    usage: Mutex<RiskUsageState>,
}

impl RiskService {
    pub fn new(config: RiskConfig) -> Self {
        Self::with_store(config, RiskUsageStore::new())
    }

    pub(crate) fn with_store(config: RiskConfig, store: RiskUsageStore) -> Self {
        let (usage, load_error) = match store.load() {
            Ok(usage) => (usage, None),
            Err(error) => (None, Some(error.user_message())),
        };
        Self {
            config,
            store,
            usage: Mutex::new(RiskUsageState { usage, load_error }),
        }
    }

    pub fn update_config(&mut self, config: RiskConfig) {
        self.config = config;
    }

    pub fn reserve_order(
        &self,
        request: &PlaceOrderRequest,
        reference_price: Option<&str>,
        now_ms: u64,
    ) -> AppResult<RiskReservation> {
        if !self.config.enabled {
            return Ok(RiskReservation::not_counted());
        }

        self.validate_order(request, reference_price)?;

        let timezone = resolve_trading_day_timezone(&self.config.trading_day_timezone);
        let trading_day = trading_day_key(now_ms, &timezone);
        let mut state = self
            .usage
            .lock()
            .map_err(|_| AppError::Storage("风控用量锁已损坏".into()))?;
        if let Some(error) = state.load_error.as_ref() {
            return Err(AppError::Storage(error.clone()));
        }

        let mut next = match state.usage.as_ref() {
            Some(usage) if usage.trading_day == trading_day && usage.timezone == timezone => {
                usage.clone()
            }
            _ => RiskUsage {
                schema_version: 1,
                trading_day: trading_day.clone(),
                timezone: timezone.clone(),
                occupied_orders: 0,
                updated_at_ms: now_ms,
            },
        };

        if next.occupied_orders >= self.config.max_daily_orders {
            return Err(AppError::Risk(format!(
                "今日下单次数已达上限 {}",
                self.config.max_daily_orders
            )));
        }

        next.occupied_orders += 1;
        next.updated_at_ms = now_ms;
        self.store.save(&next)?;
        state.usage = Some(next);

        Ok(RiskReservation::counted(trading_day, timezone))
    }

    pub fn release_reservation(&self, reservation: &RiskReservation, now_ms: u64) -> AppResult<()> {
        if !reservation.counted {
            return Ok(());
        }

        let mut state = self
            .usage
            .lock()
            .map_err(|_| AppError::Storage("风控用量锁已损坏".into()))?;
        let Some(current) = state.usage.as_ref() else {
            return Ok(());
        };
        if current.trading_day != reservation.trading_day
            || current.timezone != reservation.timezone
        {
            return Ok(());
        }

        let mut next = current.clone();
        next.occupied_orders = next.occupied_orders.saturating_sub(1);
        next.updated_at_ms = now_ms;
        self.store.save(&next)?;
        state.usage = Some(next);
        Ok(())
    }

    fn validate_order(
        &self,
        request: &PlaceOrderRequest,
        reference_price: Option<&str>,
    ) -> AppResult<()> {
        let qty = Decimal::from_str(&request.qty)
            .map_err(|_| AppError::Risk(format!("无效订单数量: {}", request.qty)))?;
        if qty <= Decimal::ZERO {
            return Err(AppError::Risk("订单数量必须大于 0".into()));
        }

        let max_qty = Decimal::from_str(&self.config.max_order_qty)
            .map_err(|_| AppError::Config("风控最大订单数量配置无效".into()))?;
        if qty > max_qty {
            return Err(AppError::Risk(format!(
                "订单数量 {} 超过最大限制 {}",
                qty, max_qty
            )));
        }

        if request.order_type.to_lowercase() == "limit" {
            let price_str = request
                .price
                .as_deref()
                .ok_or_else(|| AppError::Risk("限价单必须提供价格".into()))?;
            let price = Decimal::from_str(price_str)
                .map_err(|_| AppError::Risk(format!("无效限价: {price_str}")))?;
            if price <= Decimal::ZERO {
                return Err(AppError::Risk("限价必须大于 0".into()));
            }

            if let Some(ref_price_str) = reference_price {
                let ref_price = Decimal::from_str(ref_price_str).unwrap_or(Decimal::ZERO);
                if ref_price > Decimal::ZERO {
                    let deviation = ((price - ref_price).abs() / ref_price) * Decimal::from(100);
                    let max_dev = Decimal::from_str(&self.config.max_price_deviation_pct)
                        .map_err(|_| AppError::Config("风控价格偏离配置无效".into()))?;
                    if deviation > max_dev {
                        return Err(AppError::Risk(format!(
                            "限价偏离市价 {:.2}%，超过限制 {}%",
                            deviation, max_dev
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    use crate::storage::RiskUsageStore;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    const SHANGHAI_NOON: u64 = 1_784_606_400_000;
    const BEFORE_SHANGHAI_MIDNIGHT: u64 = 1_784_649_599_000;
    const AFTER_SHANGHAI_MIDNIGHT: u64 = 1_784_649_601_000;

    fn test_path(label: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
        std::env::temp_dir().join(format!(
            "easiflux-risk-service-{label}-{}-{sequence}.toml",
            std::process::id()
        ))
    }

    fn cleanup_test_files(path: &Path) {
        for candidate in [
            path.to_path_buf(),
            PathBuf::from(format!("{}.bak", path.display())),
            PathBuf::from(format!("{}.tmp", path.display())),
        ] {
            let _ = std::fs::remove_file(candidate);
        }
    }

    fn market_order(qty: &str) -> PlaceOrderRequest {
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

    fn limit_order(price: &str) -> PlaceOrderRequest {
        PlaceOrderRequest {
            order_type: "Limit".into(),
            price: Some(price.into()),
            ..market_order("1")
        }
    }

    fn service_with_limit(path: &Path, max_daily_orders: u32) -> RiskService {
        RiskService::with_store(
            RiskConfig {
                max_daily_orders,
                ..Default::default()
            },
            RiskUsageStore::with_path(path.to_path_buf()),
        )
    }

    #[test]
    fn rejects_oversized_order() {
        let path = test_path("oversized");
        let service = RiskService::with_store(
            RiskConfig {
                max_order_qty: "1".into(),
                ..Default::default()
            },
            RiskUsageStore::with_path(path.clone()),
        );

        assert!(service
            .reserve_order(&market_order("10"), None, SHANGHAI_NOON)
            .is_err());
        cleanup_test_files(&path);
    }

    #[test]
    fn rejects_non_positive_and_invalid_quantities() {
        let path = test_path("quantity");
        let service = service_with_limit(&path, 10);

        for qty in ["0", "-1", "abc"] {
            assert!(service
                .reserve_order(&market_order(qty), None, SHANGHAI_NOON)
                .is_err());
        }
        cleanup_test_files(&path);
    }

    #[test]
    fn rejects_non_positive_limit_prices() {
        let path = test_path("price");
        let service = service_with_limit(&path, 10);

        for price in ["0", "-1"] {
            assert!(service
                .reserve_order(&limit_order(price), Some("100"), SHANGHAI_NOON)
                .is_err());
        }
        cleanup_test_files(&path);
    }

    #[test]
    fn enforces_daily_limit_across_service_restart() {
        let path = test_path("restart");
        let request = market_order("1");

        {
            let service = service_with_limit(&path, 1);
            service
                .reserve_order(&request, None, SHANGHAI_NOON)
                .unwrap();
        }

        let restarted = service_with_limit(&path, 1);
        assert!(restarted
            .reserve_order(&request, None, SHANGHAI_NOON)
            .is_err());
        cleanup_test_files(&path);
    }

    #[test]
    fn rotates_usage_on_configured_timezone_day_boundary() {
        let path = test_path("rotate");
        let service = service_with_limit(&path, 1);
        let request = market_order("1");

        service
            .reserve_order(&request, None, BEFORE_SHANGHAI_MIDNIGHT)
            .unwrap();

        assert!(service
            .reserve_order(&request, None, AFTER_SHANGHAI_MIDNIGHT)
            .is_ok());
        cleanup_test_files(&path);
    }

    #[test]
    fn release_restores_quota_after_submission_failure() {
        let path = test_path("release");
        let service = service_with_limit(&path, 1);
        let request = market_order("1");
        let reservation = service
            .reserve_order(&request, None, SHANGHAI_NOON)
            .unwrap();

        service
            .release_reservation(&reservation, SHANGHAI_NOON)
            .unwrap();

        assert!(service.reserve_order(&request, None, SHANGHAI_NOON).is_ok());
        cleanup_test_files(&path);
    }

    #[test]
    fn release_does_not_decrement_a_new_timezone_ledger() {
        let path = test_path("timezone-release");
        let mut service = service_with_limit(&path, 1);
        let request = market_order("1");
        let old_reservation = service
            .reserve_order(&request, None, SHANGHAI_NOON)
            .unwrap();
        service.update_config(RiskConfig {
            max_daily_orders: 1,
            trading_day_timezone: "UTC".into(),
            ..Default::default()
        });
        service
            .reserve_order(&request, None, SHANGHAI_NOON)
            .unwrap();

        service
            .release_reservation(&old_reservation, SHANGHAI_NOON)
            .unwrap();

        assert!(service
            .reserve_order(&request, None, SHANGHAI_NOON)
            .is_err());
        cleanup_test_files(&path);
    }

    #[test]
    fn concurrent_reservations_cannot_overshoot_limit() {
        let path = test_path("concurrent");
        let service = Arc::new(service_with_limit(&path, 1));
        let barrier = Arc::new(Barrier::new(8));

        let successes = thread::scope(|scope| {
            let handles = (0..8)
                .map(|_| {
                    let service = Arc::clone(&service);
                    let barrier = Arc::clone(&barrier);
                    scope.spawn(move || {
                        barrier.wait();
                        service
                            .reserve_order(&market_order("1"), None, SHANGHAI_NOON)
                            .is_ok()
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .filter(|success| *success)
                .count()
        });

        assert_eq!(successes, 1);
        cleanup_test_files(&path);
    }

    #[test]
    fn persistence_failure_denies_reservation() {
        let blocker = test_path("blocked-parent");
        std::fs::write(&blocker, "file blocks directory creation").unwrap();
        let ledger = blocker.join("risk_usage.toml");
        let service = service_with_limit(&ledger, 1);

        assert!(matches!(
            service.reserve_order(&market_order("1"), None, SHANGHAI_NOON),
            Err(AppError::Storage(_))
        ));
        let _ = std::fs::remove_file(blocker);
    }

    #[test]
    fn disabled_risk_does_not_touch_unwritable_ledger() {
        let blocker = test_path("disabled-parent");
        std::fs::write(&blocker, "file blocks directory creation").unwrap();
        let service = RiskService::with_store(
            RiskConfig {
                enabled: false,
                ..Default::default()
            },
            RiskUsageStore::with_path(blocker.join("risk_usage.toml")),
        );

        assert!(service
            .reserve_order(&market_order("1"), None, SHANGHAI_NOON)
            .is_ok());
        let _ = std::fs::remove_file(blocker);
    }
}
