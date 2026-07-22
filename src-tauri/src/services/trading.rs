use std::sync::Arc;

use crate::api::diagnostic::{warn_if_parse_empty, warn_if_raw_parsed_mismatch};
use crate::api::endpoints;
use crate::api::mapper::build_order_query_params;
use crate::api::{ApiClient, PrivateApi};
use crate::error::{AppError, AppResult};
use crate::events::EventEmitter;
use crate::models::trading::{CancelOrderRequest, Order, PlaceOrderRequest};
use crate::services::risk::RiskService;
use crate::services::time::TimeService;
use crate::storage::{CacheStore, TradeLogStore};

pub struct TradingService {
    api: Arc<ApiClient>,
    risk: Arc<tokio::sync::RwLock<RiskService>>,
    trade_log: Arc<TradeLogStore>,
    cache: Arc<CacheStore>,
    emitter: EventEmitter,
    time: Arc<TimeService>,
}

impl TradingService {
    pub fn new(
        api: Arc<ApiClient>,
        risk: Arc<tokio::sync::RwLock<RiskService>>,
        trade_log: Arc<TradeLogStore>,
        cache: Arc<CacheStore>,
        emitter: EventEmitter,
        time: Arc<TimeService>,
    ) -> Self {
        Self {
            api,
            risk,
            trade_log,
            cache,
            emitter,
            time,
        }
    }

    pub async fn place_order(&self, request: PlaceOrderRequest) -> AppResult<Order> {
        let ref_price = self.cache.get_ticker(&request.symbol).map(|t| t.last_price);
        let order = execute_reserved_order(
            &self.risk,
            &request,
            ref_price.as_deref(),
            self.time.now_ms(),
            || PrivateApi::create_order(&self.api, &request),
        )
        .await?;
        let _ = self.trade_log.append_order(&order);
        self.emitter.emit_order(order.clone());
        self.emitter
            .emit_log("info", &format!("下单成功: {}", order.order_id));
        Ok(order)
    }

    pub async fn cancel_order(&self, request: CancelOrderRequest) -> AppResult<Order> {
        let order = PrivateApi::cancel_order(&self.api, &request).await?;
        let _ = self.trade_log.append_order(&order);
        self.emitter.emit_order(order.clone());
        self.emitter
            .emit_log("info", &format!("撤单成功: {}", order.order_id));
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
        let params =
            build_order_query_params(symbol, None, None, None, None, None, None, None, None, None);
        let payload = self.api.private_get(endpoints::OPEN_ORDERS, params).await?;
        let meta = crate::api::mapper::list_envelope_meta(&payload);
        let orders = crate::api::mapper::parse_orders(&payload);
        warn_if_parse_empty(&self.emitter, "activity-orders", &payload, orders.len());
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

async fn execute_reserved_order<F, Fut>(
    risk: &Arc<tokio::sync::RwLock<RiskService>>,
    request: &PlaceOrderRequest,
    reference_price: Option<&str>,
    now_ms: u64,
    submit: F,
) -> AppResult<Order>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<Order>>,
{
    let reservation = risk
        .read()
        .await
        .reserve_order(request, reference_price, now_ms)?;

    match submit().await {
        Ok(order) => Ok(order),
        Err(submit_error) => {
            if !is_confirmed_submission_failure(&submit_error) {
                return Err(submit_error);
            }
            if let Err(release_error) = risk.read().await.release_reservation(&reservation, now_ms)
            {
                return Err(AppError::Internal(format!(
                    "订单提交失败: {}; 风控预占回滚失败: {}",
                    submit_error.user_message(),
                    release_error.user_message()
                )));
            }
            Err(submit_error)
        }
    }
}

fn is_confirmed_submission_failure(error: &AppError) -> bool {
    !matches!(error, AppError::Connection(_) | AppError::Internal(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use crate::error::AppError;
    use crate::models::config::RiskConfig;
    use crate::storage::RiskUsageStore;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    const NOW_MS: u64 = 1_784_606_400_000;

    fn test_path(label: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "easiflux-trading-risk-{label}-{}-{sequence}.toml",
            std::process::id()
        ))
    }

    fn cleanup(path: &Path) {
        for candidate in [
            path.to_path_buf(),
            PathBuf::from(format!("{}.bak", path.display())),
            PathBuf::from(format!("{}.tmp", path.display())),
        ] {
            let _ = std::fs::remove_file(candidate);
        }
    }

    fn market_order() -> PlaceOrderRequest {
        PlaceOrderRequest {
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Market".into(),
            qty: "1".into(),
            position_idx: 0,
            price: None,
            time_in_force: None,
            order_link_id: None,
            reduce_only: None,
        }
    }

    fn risk_with_limit(path: &Path, limit: u32) -> Arc<tokio::sync::RwLock<RiskService>> {
        Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
            RiskConfig {
                max_daily_orders: limit,
                ..Default::default()
            },
            RiskUsageStore::with_path(path.to_path_buf()),
        )))
    }

    #[tokio::test]
    async fn submission_error_releases_reservation() {
        let path = test_path("release");
        let risk = risk_with_limit(&path, 1);
        let request = market_order();

        let result = execute_reserved_order(&risk, &request, None, NOW_MS, || {
            std::future::ready(Err::<Order, AppError>(AppError::Trading("rejected".into())))
        })
        .await;

        assert!(result.is_err());
        assert!(risk
            .read()
            .await
            .reserve_order(&request, None, NOW_MS)
            .is_ok());
        cleanup(&path);
    }

    #[tokio::test]
    async fn ambiguous_connection_error_keeps_reservation() {
        let path = test_path("ambiguous");
        let risk = risk_with_limit(&path, 1);
        let request = market_order();

        let result = execute_reserved_order(&risk, &request, None, NOW_MS, || {
            std::future::ready(Err::<Order, AppError>(AppError::Connection(
                "request timed out".into(),
            )))
        })
        .await;

        assert!(result.is_err());
        assert!(risk
            .read()
            .await
            .reserve_order(&request, None, NOW_MS)
            .is_err());
        cleanup(&path);
    }

    #[tokio::test]
    async fn ambiguous_internal_error_keeps_reservation() {
        let path = test_path("ambiguous-internal");
        let risk = risk_with_limit(&path, 1);
        let request = market_order();

        let result = execute_reserved_order(&risk, &request, None, NOW_MS, || {
            std::future::ready(Err::<Order, AppError>(AppError::Internal(
                "response status is unknown".into(),
            )))
        })
        .await;

        assert!(matches!(result, Err(AppError::Internal(_))));
        assert!(risk
            .read()
            .await
            .reserve_order(&request, None, NOW_MS)
            .is_err());
        cleanup(&path);
    }

    #[tokio::test]
    async fn reservation_failure_prevents_submission() {
        let path = test_path("deny");
        let risk = risk_with_limit(&path, 1);
        let request = market_order();
        risk.read()
            .await
            .reserve_order(&request, None, NOW_MS)
            .unwrap();
        let called = Arc::new(AtomicBool::new(false));
        let called_by_submit = Arc::clone(&called);

        let result = execute_reserved_order(&risk, &request, None, NOW_MS, move || {
            called_by_submit.store(true, Ordering::SeqCst);
            std::future::ready(Err::<Order, AppError>(AppError::Trading("called".into())))
        })
        .await;

        assert!(result.is_err());
        assert!(!called.load(Ordering::SeqCst));
        cleanup(&path);
    }
}
