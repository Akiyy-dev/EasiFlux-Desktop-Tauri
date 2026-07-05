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

pub struct AnalyticsService {
    api: Arc<ApiClient>,
    orders: Arc<RwLock<HashMap<String, Order>>>,
    positions: Arc<RwLock<HashMap<String, Position>>>,
    snapshot: Arc<RwLock<ApiSnapshot>>,
}

impl AnalyticsService {
    pub fn new(api: Arc<ApiClient>) -> Self {
        Self {
            api,
            orders: Arc::new(RwLock::new(HashMap::new())),
            positions: Arc::new(RwLock::new(HashMap::new())),
            snapshot: Arc::new(RwLock::new(ApiSnapshot::default())),
        }
    }

    fn position_key(position: &Position) -> String {
        format!("{}:{}", position.symbol, position.position_idx)
    }

    pub async fn record_order(&self, order: Order) {
        let mut map = self.orders.write().await;
        map.insert(order.order_id.clone(), order);
    }

    pub async fn record_position(&self, position: Position) {
        let key = Self::position_key(&position);
        let mut map = self.positions.write().await;
        map.insert(key, position);
    }

    pub async fn refresh_from_api(&self, emitter: &EventEmitter) -> AppResult<()> {
        if !self.api.has_credential().await {
            return Ok(());
        }

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

        {
            let mut pos_map = self.positions.write().await;
            pos_map.clear();
            for position in positions {
                pos_map.insert(Self::position_key(&position), position);
            }
        }

        let mut snapshot = ApiSnapshot::default();
        snapshot.history_total = history_orders.len() as u32;
        for order in &history_orders {
            match order.status {
                OrderStatus::Filled => snapshot.history_filled += 1,
                OrderStatus::Cancelled => snapshot.history_cancelled += 1,
                _ => {}
            }
            self.record_order(order.clone()).await;
        }

        for fill in fills {
            if let Some(qty) = get_str(fill, &["qty", "quantity", "size", "execQty", "exec_qty"]) {
                snapshot.fill_volume += Decimal::from_str(&qty).unwrap_or(Decimal::ZERO);
            }
        }

        for row in closed_rows {
            let pnl_raw = get_str(row, &[
                "closedPnl",
                "closed_pnl",
                "realisedPnl",
                "realised_pnl",
                "realizedPnl",
                "realized_pnl",
                "pnl",
            ])
            .unwrap_or_else(|| "0".into());
            let pnl = Decimal::from_str(&pnl_raw).unwrap_or(Decimal::ZERO);
            snapshot.realized_pnl += pnl;
            if pnl > Decimal::ZERO {
                snapshot.win_count += 1;
            } else if pnl < Decimal::ZERO {
                snapshot.loss_count += 1;
            }
        }

        *self.snapshot.write().await = snapshot;
        Ok(())
    }

    pub async fn compute_stats(&self) -> TradeStats {
        let orders = self.orders.read().await;
        let positions = self.positions.read().await;
        let snapshot = self.snapshot.read().await;

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
