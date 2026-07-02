use std::sync::Arc;

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
        let orders = PrivateApi::open_orders(&self.api, symbol).await?;
        for order in &orders {
            self.emitter.emit_order(order.clone());
        }
        Ok(orders)
    }
}
