use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use rust_decimal::Decimal;
use rust_decimal::prelude::FromStr;

use crate::error::{AppError, AppResult};
use crate::models::config::RiskConfig;
use crate::models::trading::PlaceOrderRequest;

pub struct RiskService {
    config: RiskConfig,
    daily_count: AtomicU32,
}

impl RiskService {
    pub fn new(config: RiskConfig) -> Self {
        Self {
            config,
            daily_count: AtomicU32::new(0),
        }
    }

    pub fn update_config(&mut self, config: RiskConfig) {
        self.config = config;
    }

    pub fn validate_order(
        &self,
        request: &PlaceOrderRequest,
        reference_price: Option<&str>,
    ) -> AppResult<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let qty = Decimal::from_str(&request.qty).unwrap_or(Decimal::ZERO);
        let max_qty = Decimal::from_str(&self.config.max_order_qty).unwrap_or(Decimal::MAX);
        if qty > max_qty {
            return Err(AppError::Risk(format!(
                "订单数量 {} 超过最大限制 {}",
                qty, max_qty
            )));
        }

        let count = self.daily_count.load(Ordering::Relaxed);
        if count >= self.config.max_daily_orders {
            return Err(AppError::Risk(format!(
                "今日下单次数已达上限 {}",
                self.config.max_daily_orders
            )));
        }

        if request.order_type.to_lowercase() == "limit" {
            if let (Some(price_str), Some(ref_price_str)) = (&request.price, reference_price) {
                let price = Decimal::from_str(price_str).unwrap_or(Decimal::ZERO);
                let ref_price = Decimal::from_str(ref_price_str).unwrap_or(Decimal::ZERO);
                if ref_price > Decimal::ZERO {
                    let deviation = ((price - ref_price).abs() / ref_price) * Decimal::from(100);
                    let max_dev = Decimal::from_str(&self.config.max_price_deviation_pct)
                        .unwrap_or(Decimal::from(5));
                    if deviation > max_dev {
                        return Err(AppError::Risk(format!(
                            "限价偏离市价 {:.2}%，超过限制 {}%",
                            deviation, max_dev
                        )));
                    }
                }
            }
        }

        self.daily_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn reset_daily_count(&self) {
        self.daily_count.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_oversized_order() {
        let svc = RiskService::new(RiskConfig {
            max_order_qty: "1".into(),
            ..Default::default()
        });
        let req = PlaceOrderRequest {
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Market".into(),
            qty: "10".into(),
            position_idx: 0,
            price: None,
            time_in_force: None,
            order_link_id: None,
            reduce_only: None,
        };
        assert!(svc.validate_order(&req, None).is_err());
    }
}
