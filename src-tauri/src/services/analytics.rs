use std::collections::HashMap;
use std::sync::Arc;

use rust_decimal::Decimal;
use rust_decimal::prelude::FromStr;
use tokio::sync::RwLock;

use crate::models::trading::{Order, OrderStatus, TradeStats};

pub struct AnalyticsService {
    orders: Arc<RwLock<HashMap<String, Order>>>,
}

impl AnalyticsService {
    pub fn new() -> Self {
        Self {
            orders: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn record_order(&self, order: Order) {
        let mut map = self.orders.write().await;
        map.insert(order.order_id.clone(), order);
    }

    pub async fn compute_stats(&self) -> TradeStats {
        let orders = self.orders.read().await;
        let total = orders.len() as u32;
        let filled = orders
            .values()
            .filter(|o| o.status == OrderStatus::Filled)
            .count() as u32;
        let cancelled = orders
            .values()
            .filter(|o| o.status == OrderStatus::Cancelled)
            .count() as u32;
        let total_volume: Decimal = orders
            .values()
            .map(|o| Decimal::from_str(&o.qty).unwrap_or(Decimal::ZERO))
            .sum();
        let win_rate = if total > 0 {
            (filled as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        TradeStats {
            total_orders: total,
            filled_orders: filled,
            cancelled_orders: cancelled,
            total_volume: total_volume.to_string(),
            realized_pnl: "0".into(),
            win_rate_pct: format!("{:.2}", win_rate),
        }
    }
}

impl Default for AnalyticsService {
    fn default() -> Self {
        Self::new()
    }
}
