use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use rust_decimal::Decimal;
use tokio::sync::RwLock;

use crate::api::diagnostic::warn_if_parse_empty;
use crate::api::response::{extract_list, get_str};
use crate::api::{ApiClient, PrivateApi};
use crate::error::AppResult;
use crate::events::EventEmitter;
use crate::models::trading::{Order, OrderStatus, Position, TradeStats};

const HISTORY_LIMIT: u32 = 200;
const FILLS_LIMIT: u32 = 200;
const CLOSED_PNL_LIMIT: u32 = 200;

#[derive(Default)]
struct ApiSnapshot {
    history_total: u32,
    history_filled: u32,
    history_cancelled: u32,
    fill_volume: Decimal,
    realized_pnl: Decimal,
    win_count: u32,
    loss_count: u32,
}

#[derive(Default)]
struct AnalyticsData {
    orders: HashMap<String, Order>,
    positions: HashMap<String, Position>,
    snapshot: ApiSnapshot,
}

pub struct AnalyticsService {
    api: Arc<ApiClient>,
    data: RwLock<AnalyticsData>,
}

impl AnalyticsService {
    pub fn new(api: Arc<ApiClient>) -> Self {
        Self {
            api,
            data: RwLock::new(AnalyticsData::default()),
        }
    }

    fn position_key(position: &Position) -> String {
        format!("{}:{}", position.symbol, position.position_idx)
    }

    pub async fn record_order(&self, order: Order) {
        self.data
            .write()
            .await
            .orders
            .insert(order.order_id.clone(), order);
    }

    pub async fn record_position(&self, position: Position) {
        let key = Self::position_key(&position);
        self.data.write().await.positions.insert(key, position);
    }

    pub async fn clear_account_data(&self) {
        *self.data.write().await = AnalyticsData::default();
    }

    async fn replace_account_data(
        &self,
        orders: Vec<Order>,
        positions: Vec<Position>,
        snapshot: ApiSnapshot,
    ) {
        let orders = orders
            .into_iter()
            .map(|order| (order.order_id.clone(), order))
            .collect();
        let positions = positions
            .into_iter()
            .map(|position| (Self::position_key(&position), position))
            .collect();
        *self.data.write().await = AnalyticsData {
            orders,
            positions,
            snapshot,
        };
    }

    async fn replace_after_success<F, Fut>(&self, load: F) -> AppResult<()>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = AppResult<(Vec<Order>, Vec<Position>, ApiSnapshot)>>,
    {
        let (orders, positions, snapshot) = load().await?;
        self.replace_account_data(orders, positions, snapshot).await;
        Ok(())
    }

    pub async fn refresh_from_api(&self, emitter: &EventEmitter) -> AppResult<()> {
        if !self.api.has_credential().await {
            return Ok(());
        }

        self.replace_after_success(|| self.fetch_account_data(emitter))
            .await
    }

    async fn fetch_account_data(
        &self,
        emitter: &EventEmitter,
    ) -> AppResult<(Vec<Order>, Vec<Position>, ApiSnapshot)> {
        let history_payload = PrivateApi::orders(
            &self.api,
            None,
            None,
            None,
            None,
            None,
            Some(HISTORY_LIMIT),
            None,
        )
        .await?;
        let history_orders = crate::api::mapper::parse_orders(&history_payload);
        warn_if_parse_empty(
            emitter,
            "trade/orders",
            &history_payload,
            history_orders.len(),
        );

        let fills_payload = PrivateApi::trade_fills(
            &self.api,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(FILLS_LIMIT),
            None,
        )
        .await?;
        let fills = extract_list(&fills_payload);
        warn_if_parse_empty(emitter, "trade/fills", &fills_payload, fills.len());

        let closed_payload = PrivateApi::closed_pnl(
            &self.api,
            None,
            None,
            None,
            None,
            Some(CLOSED_PNL_LIMIT),
            None,
        )
        .await?;
        let closed_rows = extract_list(&closed_payload);
        warn_if_parse_empty(
            emitter,
            "position/closed-pnl",
            &closed_payload,
            closed_rows.len(),
        );

        let positions_payload = {
            let params = crate::api::mapper::build_order_query_params(
                None, None, None, None, None, None, None, None, None, None,
            );
            self.api
                .private_get(crate::api::endpoints::POSITIONS, params)
                .await?
        };
        let positions = crate::api::mapper::parse_positions(&positions_payload);
        warn_if_parse_empty(
            emitter,
            "position/list",
            &positions_payload,
            positions.len(),
        );

        let mut snapshot = ApiSnapshot {
            history_total: history_orders.len() as u32,
            ..ApiSnapshot::default()
        };
        for order in &history_orders {
            match order.status {
                OrderStatus::Filled => snapshot.history_filled += 1,
                OrderStatus::Cancelled => snapshot.history_cancelled += 1,
                _ => {}
            }
        }

        for fill in fills {
            if let Some(qty) = get_str(fill, &["qty", "quantity", "size", "execQty", "exec_qty"]) {
                snapshot.fill_volume += Decimal::from_str(&qty).unwrap_or(Decimal::ZERO);
            }
        }

        for row in closed_rows {
            let pnl_raw = get_str(
                row,
                &[
                    "closedPnl",
                    "closed_pnl",
                    "realisedPnl",
                    "realised_pnl",
                    "realizedPnl",
                    "realized_pnl",
                    "pnl",
                ],
            )
            .unwrap_or_else(|| "0".into());
            let pnl = Decimal::from_str(&pnl_raw).unwrap_or(Decimal::ZERO);
            snapshot.realized_pnl += pnl;
            if pnl > Decimal::ZERO {
                snapshot.win_count += 1;
            } else if pnl < Decimal::ZERO {
                snapshot.loss_count += 1;
            }
        }

        Ok((history_orders, positions, snapshot))
    }

    pub async fn compute_stats(&self) -> TradeStats {
        let data = self.data.read().await;
        let orders = &data.orders;
        let positions = &data.positions;
        let snapshot = &data.snapshot;

        let session_total = orders.len() as u32;
        let session_filled = orders
            .values()
            .filter(|o| o.status == OrderStatus::Filled)
            .count() as u32;
        let session_cancelled = orders
            .values()
            .filter(|o| o.status == OrderStatus::Cancelled)
            .count() as u32;

        let total_orders = session_total.max(snapshot.history_total);
        let filled_orders = session_filled.max(snapshot.history_filled);
        let cancelled_orders = session_cancelled.max(snapshot.history_cancelled);

        let session_volume: Decimal = orders
            .values()
            .map(|o| Decimal::from_str(&o.qty).unwrap_or(Decimal::ZERO))
            .sum();
        let total_volume = if snapshot.fill_volume > Decimal::ZERO {
            snapshot.fill_volume
        } else {
            session_volume
        };

        let unrealised_pnl: Decimal = positions
            .values()
            .map(|p| Decimal::from_str(&p.unrealised_pnl).unwrap_or(Decimal::ZERO))
            .sum();

        let win_rate = {
            let total = snapshot.win_count + snapshot.loss_count;
            if total > 0 {
                (snapshot.win_count as f64 / total as f64) * 100.0
            } else {
                0.0
            }
        };

        TradeStats {
            total_orders,
            filled_orders,
            cancelled_orders,
            total_volume: total_volume.to_string(),
            realized_pnl: snapshot.realized_pnl.to_string(),
            unrealised_pnl: unrealised_pnl.to_string(),
            win_rate_pct: format!("{win_rate:.2}"),
            win_count: snapshot.win_count,
            loss_count: snapshot.loss_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    fn order(id: &str, qty: &str) -> Order {
        Order {
            order_id: id.into(),
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Limit".into(),
            price: "100".into(),
            qty: qty.into(),
            status: OrderStatus::New,
            order_link_id: None,
            filled_qty: "0".into(),
            avg_price: "0".into(),
        }
    }

    fn position(symbol: &str, unrealised_pnl: &str) -> Position {
        Position {
            symbol: symbol.into(),
            side: "Buy".into(),
            size: "1".into(),
            entry_price: "100".into(),
            leverage: "1".into(),
            unrealised_pnl: unrealised_pnl.into(),
            position_idx: 0,
        }
    }

    fn service() -> AnalyticsService {
        AnalyticsService::new(Arc::new(ApiClient::new()))
    }

    #[tokio::test]
    async fn successful_refresh_commit_replaces_orders_and_positions_including_empty_lists() {
        let analytics = service();
        analytics.record_order(order("old", "9")).await;
        analytics.record_position(position("OLDUSDT", "9")).await;

        analytics
            .replace_account_data(
                vec![order("new", "2")],
                vec![position("BTCUSDT", "7")],
                ApiSnapshot::default(),
            )
            .await;

        let replaced = analytics.compute_stats().await;
        assert_eq!(replaced.total_orders, 1);
        assert_eq!(replaced.total_volume, "2");
        assert_eq!(replaced.unrealised_pnl, "7");

        analytics
            .replace_account_data(Vec::new(), Vec::new(), ApiSnapshot::default())
            .await;
        let empty = analytics.compute_stats().await;
        assert_eq!(empty.total_orders, 0);
        assert_eq!(empty.total_volume, "0");
        assert_eq!(empty.unrealised_pnl, "0");
    }

    #[tokio::test]
    async fn failed_refresh_loader_preserves_the_previous_complete_snapshot() {
        let analytics = service();
        analytics.record_order(order("old", "3")).await;
        analytics.record_position(position("BTCUSDT", "4")).await;

        let result = analytics
            .replace_after_success(|| async {
                Err::<(Vec<Order>, Vec<Position>, ApiSnapshot), AppError>(AppError::Connection(
                    "simulated fetch failure".into(),
                ))
            })
            .await;

        assert!(result.is_err());
        let stats = analytics.compute_stats().await;
        assert_eq!(stats.total_orders, 1);
        assert_eq!(stats.total_volume, "3");
        assert_eq!(stats.unrealised_pnl, "4");
    }

    #[tokio::test]
    async fn clear_account_data_resets_all_account_bound_analytics_atomically() {
        let analytics = service();
        analytics.record_order(order("old", "5")).await;
        analytics.record_position(position("BTCUSDT", "6")).await;

        analytics.clear_account_data().await;

        let stats = analytics.compute_stats().await;
        assert_eq!(stats.total_orders, 0);
        assert_eq!(stats.total_volume, "0");
        assert_eq!(stats.unrealised_pnl, "0");
        assert_eq!(stats.realized_pnl, "0");
    }
}
