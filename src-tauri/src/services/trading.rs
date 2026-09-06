use std::sync::Arc;

use crate::api::diagnostic::{warn_if_parse_empty, warn_if_raw_parsed_mismatch};
use crate::api::endpoints;
use crate::api::mapper::build_order_query_params;
use crate::api::{ApiClient, PrivateApi};
use crate::error::{AppError, AppResult};
use crate::events::EventEmitter;
use crate::models::config::{normalize_account_id, AppConfig};
use crate::models::risk::RiskViolation;
use crate::models::trading::{
    CancelOrderRequest, Order, OrderStatus, OrderStreamContext, PlaceOrderRequest,
    SubmissionContext,
};
use crate::services::account_profiles::{
    run_account_private_mutation, AccountLifecycleCoordinator,
};
use crate::services::notification::policy::{PolicyContext, RiskViolationInput};
use crate::services::notification::{
    NotificationPolicy, NotificationRuntime, ObservedOrderStatus, OrderObservation,
    OrderObservationOrigin,
};
use crate::services::risk::{RiskReservation, RiskService};
use crate::services::time::TimeService;
use crate::services::AnalyticsService;
use crate::storage::{CacheStore, TradeLogStore};

mod quote;

#[derive(Clone)]
pub(crate) struct OrderNotificationObserver {
    runtime: Arc<NotificationRuntime>,
}

impl OrderNotificationObserver {
    pub(crate) fn new(runtime: Arc<NotificationRuntime>) -> Self {
        Self { runtime }
    }

    pub(crate) async fn observe(
        &self,
        account_id: &str,
        session_epoch: u64,
        order: &Order,
        origin: OrderObservationOrigin,
        submission_id: Option<&str>,
        now_ms: u64,
    ) -> Option<String> {
        self.observe_checked(
            account_id,
            session_epoch,
            order,
            origin,
            submission_id,
            now_ms,
        )
        .await
        .unwrap_or_default()
    }

    async fn observe_checked(
        &self,
        account_id: &str,
        session_epoch: u64,
        order: &Order,
        origin: OrderObservationOrigin,
        submission_id: Option<&str>,
        now_ms: u64,
    ) -> AppResult<Option<String>> {
        let status = match order.status {
            OrderStatus::New => ObservedOrderStatus::New,
            OrderStatus::PartiallyFilled => ObservedOrderStatus::PartiallyFilled,
            OrderStatus::Filled => ObservedOrderStatus::Filled,
            OrderStatus::Cancelled => ObservedOrderStatus::Canceled,
            OrderStatus::Rejected => ObservedOrderStatus::Rejected,
            OrderStatus::Unknown => return Ok(None),
        };
        let order_id = (!order.order_id.trim().is_empty()).then(|| order.order_id.clone());
        let submission_id = submission_id.map(str::to_owned);
        let order_link_id = order
            .order_link_id
            .clone()
            .filter(|value| is_correlatable_order_link(value));
        if order_id.is_none() && submission_id.is_none() && order_link_id.is_none() {
            return Ok(None);
        }
        let service = match self.runtime.service() {
            Ok(service) => service,
            Err(availability) => {
                tracing::warn!(
                    code = availability.code(),
                    "order notification observer unavailable"
                );
                return Ok(None);
            }
        };
        match service
            .observe_order(
                OrderObservation {
                    account_id: account_id.into(),
                    session_epoch,
                    order_id,
                    submission_id,
                    order_link_id,
                    status,
                    origin,
                },
                now_ms,
            )
            .await
        {
            Ok(outcome) => Ok(outcome.notification.map(|record| record.id)),
            Err(error) => {
                tracing::warn!(code = error.code(), "order notification observation failed");
                if error.is_retryable_persistence_failure() {
                    Err(AppError::Internal(
                        "订单通知同步暂时不可用，请稍后重试".into(),
                    ))
                } else {
                    Ok(None)
                }
            }
        }
    }

    pub(crate) async fn seed_snapshot_then_replay(
        &self,
        context: &OrderStreamContext,
        snapshots: &[Order],
        buffered: Vec<Order>,
        now_ms: u64,
    ) -> AppResult<()> {
        let mut offset = 0_u64;
        let (deferred, baseline): (Vec<_>, Vec<_>) = snapshots.iter().partition(|snapshot| {
            let mut saw_live_nonterminal = false;
            buffered.iter().any(|live| {
                if !orders_share_identity(snapshot, live) {
                    return false;
                }
                if matches!(live.status, OrderStatus::New | OrderStatus::PartiallyFilled) {
                    saw_live_nonterminal = true;
                    return false;
                }
                saw_live_nonterminal
                    && matches!(
                        live.status,
                        OrderStatus::Filled | OrderStatus::Cancelled | OrderStatus::Rejected
                    )
            })
        });
        for snapshot in baseline {
            self.observe_checked(
                &context.account_id,
                context.session_epoch,
                snapshot,
                OrderObservationOrigin::Snapshot,
                None,
                now_ms.saturating_add(offset),
            )
            .await?;
            offset = offset.saturating_add(1);
            tokio::task::yield_now().await;
        }
        for live in buffered {
            self.observe_checked(
                &context.account_id,
                context.session_epoch,
                &live,
                OrderObservationOrigin::Realtime,
                None,
                now_ms.saturating_add(offset),
            )
            .await?;
            offset = offset.saturating_add(1);
            tokio::task::yield_now().await;
        }
        for snapshot in deferred {
            self.observe_checked(
                &context.account_id,
                context.session_epoch,
                snapshot,
                OrderObservationOrigin::Snapshot,
                None,
                now_ms.saturating_add(offset),
            )
            .await?;
            offset = offset.saturating_add(1);
            tokio::task::yield_now().await;
        }
        Ok(())
    }

    pub(crate) async fn observe_rejection(
        &self,
        context: &SubmissionContext,
        order_link_id: Option<&str>,
        now_ms: u64,
    ) -> Option<String> {
        let service = self.runtime.service().ok()?;
        match service
            .observe_order(
                OrderObservation {
                    account_id: context.account_id.clone(),
                    session_epoch: context.session_epoch,
                    order_id: None,
                    submission_id: Some(context.submission_id.clone()),
                    order_link_id: order_link_id
                        .filter(|value| is_correlatable_order_link(value))
                        .map(str::to_owned),
                    status: ObservedOrderStatus::Rejected,
                    origin: OrderObservationOrigin::Command,
                },
                now_ms,
            )
            .await
        {
            Ok(outcome) => outcome.notification.map(|record| record.id),
            Err(error) => {
                tracing::warn!(
                    code = error.code(),
                    "submission rejection notification failed"
                );
                None
            }
        }
    }

    pub(crate) async fn observe_risk_block(
        &self,
        context: &SubmissionContext,
        violation: &RiskViolation,
        now_ms: u64,
    ) -> Option<String> {
        let service = self.runtime.service().ok()?;
        let policy_context = PolicyContext::new(
            context.account_id.clone(),
            context.session_epoch,
            context.submission_id.clone(),
        )
        .ok()?;
        let input = NotificationPolicy
            .risk_order_blocked(
                policy_context,
                RiskViolationInput {
                    code: violation.code,
                    limit: violation.safe_params.limit,
                },
            )
            .ok()?;
        match service.publish(input, now_ms).await {
            Ok(outcome) => outcome.notification.map(|record| record.id),
            Err(error) => {
                tracing::warn!(code = error.code(), "risk notification observation failed");
                None
            }
        }
    }

    pub(crate) async fn observe_realtime_for_session(
        &self,
        coordinator: &AccountLifecycleCoordinator,
        config: &tokio::sync::RwLock<AppConfig>,
        context: &OrderStreamContext,
        order: &Order,
        now_ms: u64,
    ) -> Option<String> {
        let _guard = coordinator.read_guard().await;
        let active_account_id = normalize_account_id(&config.read().await.active_account_id);
        if coordinator.current_session_epoch() != context.session_epoch
            || active_account_id != context.account_id
        {
            return None;
        }
        self.observe(
            &context.account_id,
            context.session_epoch,
            order,
            OrderObservationOrigin::Realtime,
            None,
            now_ms,
        )
        .await
    }
}

fn orders_share_identity(left: &Order, right: &Order) -> bool {
    (!left.order_id.trim().is_empty()
        && !right.order_id.trim().is_empty()
        && left.order_id == right.order_id)
        || matches!(
            (left.order_link_id.as_deref(), right.order_link_id.as_deref()),
            (Some(left), Some(right))
                if is_correlatable_order_link(left)
                    && is_correlatable_order_link(right)
                    && left == right
        )
}

fn is_correlatable_order_link(value: &str) -> bool {
    !value.is_empty() && value.chars().count() <= 36
}

pub struct TradingService {
    api: Arc<ApiClient>,
    risk: Arc<tokio::sync::RwLock<RiskService>>,
    trade_log: Arc<TradeLogStore>,
    emitter: EventEmitter,
    time: Arc<TimeService>,
    analytics: Arc<AnalyticsService>,
    notification_observer: OrderNotificationObserver,
}

impl TradingService {
    pub fn new(
        api: Arc<ApiClient>,
        risk: Arc<tokio::sync::RwLock<RiskService>>,
        trade_log: Arc<TradeLogStore>,
        _cache: Arc<CacheStore>,
        emitter: EventEmitter,
        time: Arc<TimeService>,
        analytics: Arc<AnalyticsService>,
        notification_runtime: Arc<NotificationRuntime>,
    ) -> Self {
        Self {
            api,
            risk,
            trade_log,
            emitter,
            time,
            analytics,
            notification_observer: OrderNotificationObserver::new(notification_runtime),
        }
    }

    pub async fn place_order(
        &self,
        context: SubmissionContext,
        request: PlaceOrderRequest,
    ) -> AppResult<Order> {
        let session_context = OrderStreamContext::from(&context);
        let requires_reference_price = self.risk.read().await.requires_reference_price(&request);
        let reference_price = if requires_reference_price {
            quote::fresh_reference_price(self.api.as_ref(), &request.symbol).await
        } else {
            None
        };
        let now_ms = self.time.now_ms();
        let result = execute_place_order(
            self.api.as_ref(),
            &self.risk,
            &self.notification_observer,
            &context,
            request,
            reference_price.as_deref(),
            now_ms,
            |order| async move {
                let _ = self.trade_log.append_order(&order);
                self.emitter.emit_order(&session_context, order.clone());
                self.emitter
                    .emit_log("info", &format!("下单成功: {}", order.order_id));
                self.analytics.record_order(order).await;
            },
        )
        .await;
        deliver_notified_placement(&self.emitter, result)
    }

    pub async fn cancel_order(
        &self,
        context: OrderStreamContext,
        request: CancelOrderRequest,
    ) -> AppResult<Order> {
        let order = PrivateApi::cancel_order(&self.api, &request).await?;
        self.notification_observer
            .observe(
                &context.account_id,
                context.session_epoch,
                &order,
                OrderObservationOrigin::Command,
                None,
                self.time.now_ms(),
            )
            .await;
        let _ = self.trade_log.append_order(&order);
        self.emitter.emit_order(&context, order.clone());
        self.emitter
            .emit_log("info", &format!("撤单成功: {}", order.order_id));
        self.analytics.record_order(order.clone()).await;
        Ok(order)
    }

    pub async fn refresh_orders(
        &self,
        context: &OrderStreamContext,
        symbol: Option<&str>,
    ) -> AppResult<Vec<Order>> {
        let orders = self.fetch_open_orders(context, symbol).await?;
        for order in &orders {
            self.emitter.emit_order(context, order.clone());
            self.analytics.record_order(order.clone()).await;
        }
        Ok(orders)
    }

    pub async fn fetch_open_orders(
        &self,
        _context: &OrderStreamContext,
        symbol: Option<&str>,
    ) -> AppResult<Vec<Order>> {
        self.fetch_open_orders_unobserved(symbol).await
    }

    pub(crate) async fn fetch_open_orders_unobserved(
        &self,
        symbol: Option<&str>,
    ) -> AppResult<Vec<Order>> {
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
        context: &OrderStreamContext,
        symbol: Option<&str>,
        limit: Option<u32>,
    ) -> AppResult<Vec<Order>> {
        self.fetch_order_history(context, symbol, limit).await
    }

    pub async fn fetch_order_history(
        &self,
        _context: &OrderStreamContext,
        symbol: Option<&str>,
        limit: Option<u32>,
    ) -> AppResult<Vec<Order>> {
        self.fetch_order_history_unobserved(symbol, limit).await
    }

    pub(crate) async fn fetch_order_history_unobserved(
        &self,
        symbol: Option<&str>,
        limit: Option<u32>,
    ) -> AppResult<Vec<Order>> {
        PrivateApi::order_history(&self.api, symbol, limit).await
    }
}

fn deliver_notified_placement<T>(emitter: &EventEmitter, result: AppResult<T>) -> AppResult<T> {
    if let Err(AppError::Notified {
        code,
        notification_id,
        ..
    }) = &result
    {
        if *code != "AUTH_SESSION_EXPIRED" || notification_id.trim().is_empty() {
            emitter.emit_diagnostic(&format!("NOTIFIED_PLACEMENT_FAILURE:{code}"), false);
        }
    }
    result
}

async fn execute_place_order<S, SFut>(
    api: &ApiClient,
    risk: &Arc<tokio::sync::RwLock<RiskService>>,
    observer: &OrderNotificationObserver,
    context: &SubmissionContext,
    mut request: PlaceOrderRequest,
    reference_price: Option<&str>,
    now_ms: u64,
    success_side_effects: S,
) -> AppResult<Order>
where
    S: FnOnce(Order) -> SFut,
    SFut: std::future::Future<Output = ()>,
{
    let transmitted_order_link_id = match request.order_link_id.as_deref() {
        Some(value) if is_correlatable_order_link(value) => value.to_owned(),
        Some(_) => {
            return Err(AppError::Trading(
                "订单关联标识必须为 1 到 36 个字符".into(),
            ))
        }
        None => context.submission_id.clone(),
    };
    request.order_link_id = Some(transmitted_order_link_id.clone());
    let reservation_result = {
        let risk = risk.read().await;
        risk.reserve_order(&request, reference_price, now_ms)
    };
    let reservation = match reservation_result {
        Ok(reservation) => reservation,
        Err(violation) => {
            let notification_id = observer
                .observe_risk_block(context, &violation, now_ms)
                .await;
            return Err(match notification_id {
                Some(notification_id) => AppError::Notified {
                    code: "RISK_ORDER_BLOCKED",
                    message: if violation.code
                        == crate::models::risk::RiskViolationCode::ReferencePriceUnavailable
                    {
                        "无法获取有效的最新市价，请稍后重试"
                    } else {
                        "订单被风控拦截"
                    },
                    notification_id,
                    cause: None,
                },
                None => violation.into(),
            });
        }
    };

    let order = match PrivateApi::create_order(api, &request).await {
        Ok(order) => order,
        Err(submit_error) => {
            return Err(handle_failed_submission(
                risk,
                observer,
                &reservation,
                context,
                &transmitted_order_link_id,
                submit_error,
                now_ms,
            )
            .await);
        }
    };

    let mut observed_order = order.clone();
    if observed_order.order_link_id.is_none() {
        observed_order.order_link_id = Some(transmitted_order_link_id);
    }
    observer
        .observe(
            &context.account_id,
            context.session_epoch,
            &observed_order,
            OrderObservationOrigin::Command,
            Some(&context.submission_id),
            now_ms,
        )
        .await;
    success_side_effects(order.clone()).await;
    Ok(order)
}

pub(crate) async fn execute_coordinated_reserved_order<P, N, F, Fut, S, SFut>(
    coordinator: &AccountLifecycleCoordinator,
    risk: &Arc<tokio::sync::RwLock<RiskService>>,
    request: &PlaceOrderRequest,
    reference_price: P,
    now_ms: N,
    submit: F,
    side_effects: S,
) -> AppResult<Order>
where
    P: FnOnce() -> Option<String>,
    N: FnOnce() -> u64,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<Order>>,
    S: FnOnce(Order) -> SFut,
    SFut: std::future::Future<Output = ()>,
{
    // Lock order: lifecycle coordinator -> risk RwLock -> risk usage Mutex.
    // The risk guards are released before the API future is awaited.
    execute_coordinated_private_mutation(
        coordinator,
        move || async move {
            let reference_price = reference_price();
            execute_reserved_order(risk, request, reference_price.as_deref(), now_ms(), submit)
                .await
        },
        side_effects,
    )
    .await
}

async fn execute_coordinated_private_mutation<T, F, Fut, S, SFut>(
    coordinator: &AccountLifecycleCoordinator,
    mutation: F,
    side_effects: S,
) -> AppResult<T>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<T>>,
    T: Clone,
    S: FnOnce(T) -> SFut,
    SFut: std::future::Future<Output = ()>,
{
    run_account_private_mutation(coordinator, move || async move {
        let result = mutation().await?;
        side_effects(result.clone()).await;
        Ok(result)
    })
    .await
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
            if !is_certain_submission_failure(&submit_error) {
                return Err(submit_error);
            }
            if let Err(release_error) = risk.read().await.release_reservation(&reservation, now_ms)
            {
                tracing::warn!(
                    release_error_kind = ?std::mem::discriminant(&release_error),
                    "risk reservation rollback failed"
                );
                return Err(finalize_submission_failure(submit_error, true, None));
            }
            Err(submit_error)
        }
    }
}

pub(crate) fn is_certain_submission_failure(error: &AppError) -> bool {
    is_confirmed_submission_rejection(error)
        || matches!(
            error,
            AppError::AuthFailure(
                crate::api::response::AuthFailureKind::SessionExpired
                    | crate::api::response::AuthFailureKind::Timestamp
                    | crate::api::response::AuthFailureKind::Signature
                    | crate::api::response::AuthFailureKind::AccessDenied
                    | crate::api::response::AuthFailureKind::RateLimited
            )
        )
        || matches!(
            error,
            AppError::Notified {
                cause: Some(crate::error::NotificationCause::AuthFailure(
                    crate::api::response::AuthFailureKind::SessionExpired
                )),
                ..
            }
        )
}

fn is_confirmed_submission_rejection(error: &AppError) -> bool {
    matches!(
        error,
        AppError::TradingFailure(failure)
            if failure.kind == crate::models::trading::TradingFailureKind::Rejected
    )
}

async fn handle_failed_submission(
    risk: &Arc<tokio::sync::RwLock<RiskService>>,
    observer: &OrderNotificationObserver,
    reservation: &RiskReservation,
    context: &SubmissionContext,
    transmitted_order_link_id: &str,
    submit_error: AppError,
    now_ms: u64,
) -> AppError {
    let rollback_failed = is_certain_submission_failure(&submit_error)
        && risk
            .read()
            .await
            .release_reservation(reservation, now_ms)
            .is_err();
    if rollback_failed {
        tracing::warn!(
            code = "RISK_RESERVATION_ROLLBACK_FAILED",
            "risk reservation rollback failed after order submission"
        );
    }
    let notification_id = if is_confirmed_submission_rejection(&submit_error) {
        observer
            .observe_rejection(context, Some(transmitted_order_link_id), now_ms)
            .await
    } else {
        None
    };
    finalize_submission_failure(submit_error, rollback_failed, notification_id)
}

fn finalize_submission_failure(
    submit_error: AppError,
    rollback_failed: bool,
    notification_id: Option<String>,
) -> AppError {
    if matches!(&submit_error, AppError::Notified { cause: Some(_), .. }) {
        return submit_error;
    }
    if let Some(notification_id) = notification_id {
        return AppError::Notified {
            code: "ORDER_REJECTED",
            message: "订单请求被交易端拒绝",
            notification_id,
            cause: None,
        };
    }
    // A failed local rollback cannot make a confirmed API rejection uncertain.
    // The caller already records the rollback failure separately.
    if rollback_failed && !is_certain_submission_failure(&submit_error) {
        AppError::Internal("订单提交失败且风控预占回滚失败".into())
    } else {
        submit_error
    }
}

#[cfg(test)]
mod tests {
    mod notification_observer;

    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

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

    fn limit_order(price: &str) -> PlaceOrderRequest {
        PlaceOrderRequest {
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Limit".into(),
            qty: "1".into(),
            position_idx: 0,
            price: Some(price.into()),
            time_in_force: Some("GTC".into()),
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
    async fn generic_trading_error_keeps_reservation_in_memory_and_on_disk() {
        let path = test_path("ambiguous-trading");
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
            .is_err());
        assert_eq!(
            RiskUsageStore::with_path(path.clone())
                .load()
                .unwrap()
                .unwrap()
                .occupied_orders,
            1
        );
        cleanup(&path);
    }

    #[tokio::test]
    async fn typed_rejection_releases_reservation_in_memory_and_on_disk() {
        let path = test_path("typed-rejection-release");
        let risk = risk_with_limit(&path, 1);
        let request = market_order();

        let result = execute_reserved_order(&risk, &request, None, NOW_MS, || {
            std::future::ready(Err::<Order, AppError>(AppError::TradingFailure(
                crate::models::trading::TradingFailure::rejected(),
            )))
        })
        .await;

        assert!(matches!(result, Err(AppError::TradingFailure(_))));
        assert!(risk
            .read()
            .await
            .reserve_order(&request, None, NOW_MS)
            .is_ok());
        assert_eq!(
            RiskUsageStore::with_path(path.clone())
                .load()
                .unwrap()
                .unwrap()
                .occupied_orders,
            1,
            "the second reservation proves both memory and disk were released"
        );
        cleanup(&path);
    }

    #[test]
    fn only_closed_typed_no_submit_failures_are_certain() {
        let ambiguous = [
            AppError::Auth("safe".into()),
            AppError::Connection("safe".into()),
            AppError::Trading("safe".into()),
            AppError::Risk("safe".into()),
            AppError::Config("safe".into()),
            AppError::Storage("safe".into()),
            AppError::NotConnected,
            AppError::Internal("safe".into()),
            AppError::Notified {
                code: "ORDER_REJECTED",
                message: "订单请求被交易端拒绝",
                notification_id: uuid::Uuid::new_v4().to_string(),
                cause: None,
            },
            AppError::AuthFailure(crate::api::response::AuthFailureKind::MissingCredential),
            AppError::AuthFailure(crate::api::response::AuthFailureKind::CredentialStorage),
            AppError::AuthFailure(crate::api::response::AuthFailureKind::SigningConfiguration),
            AppError::AuthFailure(crate::api::response::AuthFailureKind::Other),
        ];
        assert!(ambiguous
            .iter()
            .all(|error| !is_certain_submission_failure(error)));
        for certain in [
            AppError::TradingFailure(crate::models::trading::TradingFailure::rejected()),
            AppError::AuthFailure(crate::api::response::AuthFailureKind::SessionExpired),
            AppError::AuthFailure(crate::api::response::AuthFailureKind::Timestamp),
            AppError::AuthFailure(crate::api::response::AuthFailureKind::Signature),
            AppError::AuthFailure(crate::api::response::AuthFailureKind::AccessDenied),
            AppError::AuthFailure(crate::api::response::AuthFailureKind::RateLimited),
            AppError::Notified {
                code: "AUTH_SESSION_EXPIRED",
                message: "账户会话已失效",
                notification_id: uuid::Uuid::new_v4().to_string(),
                cause: Some(crate::error::NotificationCause::AuthFailure(
                    crate::api::response::AuthFailureKind::SessionExpired,
                )),
            },
        ] {
            assert!(is_certain_submission_failure(&certain), "{certain:?}");
        }
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

    #[test]
    fn committed_rejection_notification_wins_over_secondary_rollback_failure() {
        let notification_id = uuid::Uuid::new_v4().to_string();
        let error = finalize_submission_failure(
            AppError::TradingFailure(crate::models::trading::TradingFailure::rejected()),
            true,
            Some(notification_id.clone()),
        );

        assert!(matches!(
            error,
            AppError::Notified {
                code: "ORDER_REJECTED",
                notification_id: id,
                ..
            } if id == notification_id
        ));
    }

    #[tokio::test]
    async fn confirmed_rejection_does_not_remain_pending_when_risk_rollback_fails() {
        for submit_error in [
            AppError::TradingFailure(crate::models::trading::TradingFailure::rejected()),
            AppError::AuthFailure(crate::api::response::AuthFailureKind::Signature),
        ] {
            let path = test_path("rejected-rollback");
            let blocked_temp = std::path::PathBuf::from(format!("{}.tmp", path.display()));
            let journal_dir = path.with_extension("submissions");
            let journal = crate::storage::OrderSubmissionStore::with_dir(journal_dir.clone());
            let risk = risk_with_limit(&path, 1);
            let mut request = market_order();
            request.order_link_id = Some("confirmed-rejection".into());

            let result = crate::services::order_submission::submit_once(
                &journal,
                "rollback-account",
                request,
                NOW_MS,
                |request| {
                    let risk = &risk;
                    let blocked_temp = &blocked_temp;
                    async move {
                        execute_reserved_order(risk, &request, None, NOW_MS, || async {
                            // Reservation already reached disk; fail only its rollback write.
                            std::fs::create_dir(blocked_temp).unwrap();
                            Err(submit_error)
                        })
                        .await
                    }
                },
            )
            .await;
            let pending = journal.list_pending("rollback-account").unwrap();
            let original = journal
                .get("rollback-account", "confirmed-rejection")
                .unwrap()
                .unwrap();
            std::fs::remove_dir(&blocked_temp).unwrap();
            std::fs::remove_dir_all(&journal_dir).unwrap();
            cleanup(&path);

            assert!(matches!(result, Err(AppError::OrderSubmissionRejected(_))));
            assert!(pending.is_empty());
            assert!(matches!(
                original.outcome,
                crate::models::order_submission::SubmissionOutcome::Rejected
            ));
        }
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

    #[tokio::test]
    async fn coordinated_order_samples_trading_day_after_waiting_for_guard() {
        let path = test_path("coordinated-time");
        let risk = risk_with_limit(&path, 1);
        let request = market_order();
        let coordinator = AccountLifecycleCoordinator::new();
        let clock = AtomicU64::new(NOW_MS);
        let next_day_ms = NOW_MS + 86_400_000;
        let held_guard = coordinator.mutation_guard().await;

        let order = execute_coordinated_reserved_order(
            &coordinator,
            &risk,
            &request,
            || None,
            || clock.load(Ordering::SeqCst),
            || {
                std::future::ready(Err::<Order, AppError>(AppError::Connection(
                    "request timed out".into(),
                )))
            },
            |_| async {},
        );
        tokio::pin!(order);
        assert!(matches!(
            futures_util::poll!(&mut order),
            std::task::Poll::Pending
        ));

        clock.store(next_day_ms, Ordering::SeqCst);
        drop(held_guard);
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), order.as_mut())
            .await
            .expect("order should acquire the coordinator after it is released");
        assert!(matches!(result, Err(AppError::Connection(_))));

        let usage = RiskUsageStore::with_path(path.clone())
            .load()
            .unwrap()
            .expect("ambiguous submission should retain its reservation");
        assert_eq!(
            usage.trading_day,
            crate::services::time::trading_day_key(
                next_day_ms,
                &RiskConfig::default().trading_day_timezone,
            )
        );
        cleanup(&path);
    }

    #[tokio::test]
    async fn coordinated_order_samples_reference_price_after_waiting_for_guard() {
        let path = test_path("coordinated-price");
        let risk = risk_with_limit(&path, 1);
        let request = limit_order("100");
        let coordinator = AccountLifecycleCoordinator::new();
        let reference_price = std::sync::Mutex::new("100".to_string());
        let submitted = AtomicBool::new(false);
        let held_guard = coordinator.mutation_guard().await;

        let order = execute_coordinated_reserved_order(
            &coordinator,
            &risk,
            &request,
            || Some(reference_price.lock().unwrap().clone()),
            || NOW_MS,
            || {
                submitted.store(true, Ordering::SeqCst);
                std::future::ready(Err::<Order, AppError>(AppError::Trading(
                    "submit should not run".into(),
                )))
            },
            |_| async {},
        );
        tokio::pin!(order);
        assert!(matches!(
            futures_util::poll!(&mut order),
            std::task::Poll::Pending
        ));

        *reference_price.lock().unwrap() = "200".to_string();
        drop(held_guard);
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), order.as_mut())
            .await
            .expect("order should acquire the coordinator after it is released");

        assert!(matches!(result, Err(AppError::Risk(_))));
        assert!(!submitted.load(Ordering::SeqCst));
        cleanup(&path);
    }

    #[tokio::test]
    async fn private_api_and_account_bound_side_effects_finish_before_switch() {
        let coordinator = Arc::new(AccountLifecycleCoordinator::new());
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let started = Arc::new(tokio::sync::Barrier::new(3));
        let release = Arc::new(tokio::sync::Notify::new());

        let mutation_coordinator = Arc::clone(&coordinator);
        let mutation_events = Arc::clone(&events);
        let mutation_started = Arc::clone(&started);
        let mutation_release = Arc::clone(&release);
        let side_effect_events = Arc::clone(&events);
        let mutation = async move {
            execute_coordinated_private_mutation(
                mutation_coordinator.as_ref(),
                || async move {
                    mutation_events.lock().unwrap().push("api");
                    mutation_started.wait().await;
                    mutation_release.notified().await;
                    Ok::<_, AppError>("order")
                },
                |_| async move {
                    side_effect_events.lock().unwrap().push("trade-log");
                    side_effect_events.lock().unwrap().push("emit");
                },
            )
            .await
            .unwrap();
        };

        let switch_coordinator = Arc::clone(&coordinator);
        let switch_events = Arc::clone(&events);
        let switch_started = Arc::clone(&started);
        let account_switch = async move {
            switch_started.wait().await;
            let _guard = switch_coordinator.mutation_guard().await;
            switch_events.lock().unwrap().push("switch");
        };

        let observed_events = Arc::clone(&events);
        let observer_started = Arc::clone(&started);
        let observer_release = Arc::clone(&release);
        let observer = async move {
            observer_started.wait().await;
            assert_eq!(*observed_events.lock().unwrap(), ["api"]);
            observer_release.notify_one();
        };

        let ((), (), ()) = tokio::join!(mutation, account_switch, observer);
        assert_eq!(
            *events.lock().unwrap(),
            ["api", "trade-log", "emit", "switch"]
        );
    }

    #[tokio::test]
    async fn async_analytics_side_effect_is_awaited_once_before_guard_release() {
        let coordinator = AccountLifecycleCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(tokio::sync::Notify::new());
        let side_effect_calls = Arc::clone(&calls);
        let side_effect_release = Arc::clone(&release);

        let mutation = execute_coordinated_private_mutation(
            &coordinator,
            || std::future::ready(Ok::<_, AppError>("order".to_string())),
            move |_| async move {
                side_effect_calls.fetch_add(1, Ordering::SeqCst);
                side_effect_release.notified().await;
            },
        );
        tokio::pin!(mutation);

        assert!(matches!(
            futures_util::poll!(&mut mutation),
            std::task::Poll::Pending
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let later_switch = coordinator.mutation_guard();
        tokio::pin!(later_switch);
        assert!(matches!(
            futures_util::poll!(&mut later_switch),
            std::task::Poll::Pending
        ));

        release.notify_one();
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), mutation.as_mut())
            .await
            .expect("mutation should finish after analytics is released")
            .unwrap();
        assert_eq!(result, "order");
        let _guard = tokio::time::timeout(std::time::Duration::from_secs(1), later_switch.as_mut())
            .await
            .expect("switch should acquire the guard after analytics finishes");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
