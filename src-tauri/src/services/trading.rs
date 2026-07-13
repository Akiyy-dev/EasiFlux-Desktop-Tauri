use std::sync::Arc;

use crate::api::diagnostic::{warn_if_parse_empty, warn_if_raw_parsed_mismatch};
use crate::api::mapper::build_order_query_params;
use crate::api::endpoints;
use crate::api::{ApiClient, PrivateApi};
use crate::error::AppResult;
use crate::events::EventEmitter;
use crate::models::trading::{CancelOrderRequest, Order, PlaceOrderRequest};
use crate::services::risk::RiskService;
use crate::storage::{CacheStore, TradeLogStore};

pub struct TradingService {
    api: Arc<ApiClient>,
    risk: Arc<tokio::sync::RwLock<RiskService>>,
    trade_log: Arc<TradeLogStore>,
    cache: Arc<CacheStore>,
    emitter: EventEmitter,
}

impl TradingService {
    pub fn new(
        api: Arc<ApiClient>,
        risk: Arc<tokio::sync::RwLock<RiskService>>,
        trade_log: Arc<TradeLogStore>,
        cache: Arc<CacheStore>,
        emitter: EventEmitter,
    ) -> Self {
        Self {
            api,
            risk,
            trade_log,
            cache,
            emitter,
        }
    }

    pub async fn place_order(&self, request: PlaceOrderRequest) -> AppResult<Order> {
        let ref_price = self
            .cache
            .get_ticker(&request.symbol)
            .map(|t| t.last_price);
        self.risk
            .read()
            .await
            .validate_order(&request, ref_price.as_deref())?;
        let order = PrivateApi::create_order(&self.api, &request).await?;
        let _ = self.trade_log.append_order(&order);
        self.emitter.emit_order(order.clone());
        self.emitter.emit_log("info", &format!("下单成功: {}", order.order_id));
        Ok(order)
    }

    pub async fn cancel_order(&self, request: CancelOrderRequest) -> AppResult<Order> {
        let order = PrivateApi::cancel_order(&self.api, &request).await?;
        let _ = self.trade_log.append_order(&order);
        self.emitter.emit_order(order.clone());
        self.emitter.emit_log("info", &format!("撤单成功: {}", order.order_id));
        Ok(order)
    }

    pub async fn refresh_orders(&self, symbol: Option<&str>) -> AppResult<Vec<Order>> {
        let orders = self.fetch_open_orders(symbol).await?;
        for order in &orders {
            self.emitter.emit_order(order.clone());
        }
        Ok(orders)
    }

    pub async fn fetch_open_orders(&self, symbol: Option<&str>) -> AppResult<Vec<Order>> {
        let params = build_order_query_params(
            symbol,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let payload = self.api.private_get(endpoints::OPEN_ORDERS, params).await?;
        let meta = crate::api::mapper::list_envelope_meta(&payload);
        let orders = crate::api::mapper::parse_orders(&payload);
        warn_if_parse_empty(
            &self.emitter,
            "activity-orders",
            &payload,
            orders.len(),
        );
        warn_if_raw_parsed_mismatch(&self.emitter, "activity-orders", &meta, orders.len());
        Ok(orders)
    }

    pub async fn refresh_order_history(
        &self,
        symbol: Option<&str>,
        limit: Option<u32>,
    ) -> AppResult<Vec<Order>> {
        self.fetch_order_history(symbol, limit).await
    }

    pub async fn fetch_order_history(
        &self,
        symbol: Option<&str>,
        limit: Option<u32>,
    ) -> AppResult<Vec<Order>> {
        PrivateApi::order_history(&self.api, symbol, limit).await
    }
}
