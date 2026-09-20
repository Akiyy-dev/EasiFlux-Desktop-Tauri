//! Narrow production adapter. Snapshot reads never emit refresh events or update analytics.
use super::*;
use crate::{
    api::{endpoints, mapper::build_order_query_params, ApiClient},
    error::AppResult,
    models::{
        account::Balance,
        config::{normalize_account_id, AppConfig, ConnectionStatus},
        trading::{
            CancelOrderRequest, Order, OrderStatus, PlaceOrderRequest, Position, SessionContext,
            SubmissionContext,
        },
    },
    services::{AccountLifecycleCoordinator, ConnectionService, TimeService, TradingService},
    storage::OrderSubmissionStore,
};
use rust_decimal::Decimal;
use serde_json::Value;
use std::sync::Arc;

pub(crate) struct ProductionWorkflowHost {
    api: Arc<ApiClient>,
    config: Arc<tokio::sync::RwLock<AppConfig>>,
    connection: Arc<ConnectionService>,
    lifecycle: Arc<AccountLifecycleCoordinator>,
    trading: Arc<TradingService>,
    submissions: OrderSubmissionStore,
    time: Arc<TimeService>,
}
impl ProductionWorkflowHost {
    pub(crate) fn from_state(state: &crate::state::AppState) -> Self {
        Self {
            api: state.api.clone(),
            config: state.config.clone(),
            connection: state.connection.clone(),
            lifecycle: state.account_lifecycle.clone(),
            trading: state.trading.clone(),
            submissions: state.order_submissions.clone(),
            time: state.time.clone(),
        }
    }
    async fn private_list(&self, endpoint: &str, symbol: Option<&str>) -> AppResult<Value> {
        self.api
            .private_get(
                endpoint,
                build_order_query_params(
                    symbol,
                    None,
                    None,
                    None,
                    (endpoint == endpoints::OPEN_ORDERS).then_some("Normal"),
                    Some(100),
                    None,
                    None,
                    None,
                    None,
                ),
            )
            .await
    }
}
impl WorkflowHost for ProductionWorkflowHost {
    fn lifecycle(&self) -> &AccountLifecycleCoordinator {
        &self.lifecycle
    }
    fn now_ms(&self) -> u64 {
        self.time.local_now_ms()
    }
    fn authority_locked(&self) -> HostFuture<'_, AccountAuthority> {
        Box::pin(async {
            if self.connection.status().await != ConnectionStatus::Connected {
                return Err(error("plugin_workflow_account_unavailable"));
            }
            let (session, private_session, environment) =
                self.api.workflow_session_identity().await?;
            if session.account_id
                != normalize_account_id(&self.config.read().await.active_account_id)
                || session.session_epoch != self.lifecycle.current_session_epoch()
            {
                return Err(error("plugin_workflow_account_unavailable"));
            }
            Ok(AccountAuthority {
                account: Account {
                    account_id: session.account_id,
                    session_epoch: session.session_epoch.to_string(),
                    environment,
                },
                private_session,
            })
        })
    }
    fn balances_locked(&self) -> HostFuture<'_, Vec<Balance>> {
        Box::pin(async { parse_balances(&self.private_list(endpoints::BALANCES, None).await?) })
    }
    fn positions_locked<'a>(&'a self, symbol: &'a str) -> HostFuture<'a, Vec<Position>> {
        Box::pin(async move {
            parse_positions(
                &self
                    .private_list(endpoints::POSITIONS, Some(symbol))
                    .await?,
            )
        })
    }
    fn orders_locked<'a>(&'a self, symbol: &'a str) -> HostFuture<'a, Vec<Order>> {
        Box::pin(async move {
            parse_orders(
                &self
                    .private_list(endpoints::OPEN_ORDERS, Some(symbol))
                    .await?,
            )
        })
    }
    fn market_locked<'a>(&'a self, symbol: &'a str) -> HostFuture<'a, Quote> {
        Box::pin(async move {
            let payload = self
                .api
                .public_get(
                    endpoints::TICKER,
                    std::collections::HashMap::from([("symbol".into(), symbol.into())]),
                )
                .await?;
            parse_quote(&payload, symbol)
        })
    }
    fn place_locked(
        &self,
        context: SubmissionContext,
        mut request: PlaceOrderRequest,
    ) -> HostFuture<'_, Order> {
        Box::pin(async move {
            request.time_in_force =
                Some(exchange_time_in_force(request.time_in_force.as_deref())?.into());
            let scope = self.api.order_submission_scope(&context.account_id).await?;
            crate::services::order_submission::submit_once(
                &self.submissions,
                &scope,
                request,
                self.time.local_now_ms(),
                |request| self.trading.place_order(context, request),
            )
            .await
        })
    }
    fn cancel_locked(
        &self,
        context: SessionContext,
        request: CancelOrderRequest,
    ) -> HostFuture<'_, Order> {
        Box::pin(async move { self.trading.cancel_order(context, request).await })
    }
}

fn unavailable() -> crate::error::AppError {
    error("plugin_workflow_data_unavailable")
}
fn exchange_time_in_force(value: Option<&str>) -> AppResult<&'static str> {
    match value {
        Some("GTC") => Ok("GoodTillCancel"),
        Some("IOC") => Ok("ImmediateOrCancel"),
        Some("FOK") => Ok("FillOrKill"),
        _ => Err(error("plugin_workflow_invalid_output")),
    }
}
fn exact_number(value: &Value, keys: &[&str]) -> AppResult<Decimal> {
    Decimal::from_str_exact(&number(value, keys)?).map_err(|_| unavailable())
}
// EasiCoin get-wallet-list documents reserved position/order margin, not a frozen field.
fn reserved_margin(value: &Value) -> AppResult<String> {
    if ["frozen", "locked", "frozenBalance"]
        .iter()
        .any(|key| value.get(*key).is_some())
    {
        return number(value, &["frozen", "locked", "frozenBalance"]);
    }
    let position = exact_number(value, &["position_margin"])?;
    let orders = exact_number(value, &["order_margin"])?;
    if position < Decimal::ZERO || orders < Decimal::ZERO {
        return Err(unavailable());
    }
    position
        .checked_add(orders)
        .map(|v| v.normalize().to_string())
        .ok_or_else(unavailable)
}
// Only authoritative zero fills imply zero average. Missing/malformed execution data fails closed.
fn average_fill(value: &Value) -> AppResult<String> {
    if ["avgPrice", "avg_price"]
        .iter()
        .any(|key| value.get(*key).is_some())
    {
        return number(value, &["avgPrice", "avg_price"]);
    }
    let qty = exact_number(
        value,
        &["cumExecQty", "cum_exec_qty", "filled_qty", "filledQty"],
    )?;
    if qty == Decimal::ZERO {
        return Ok("0".into());
    }
    let total = exact_number(value, &["cumExecValue", "cum_exec_value"])?;
    if qty < Decimal::ZERO || total < Decimal::ZERO {
        return Err(unavailable());
    }
    total
        .checked_div(qty)
        .map(|v| v.normalize().to_string())
        .ok_or_else(unavailable)
}
/// Accept only recognized list envelopes. Missing data is not an empty account.
fn list(payload: &Value) -> AppResult<&Vec<Value>> {
    let mut value = payload;
    for _ in 0..4 {
        if let Some(array) = value.as_array() {
            return Ok(array);
        }
        let object = value.as_object().ok_or_else(unavailable)?;
        if let Some(next) = [
            "list",
            "items",
            "records",
            "orders",
            "positions",
            "balances",
            "tickers",
            "rows",
            "dataList",
            "orderList",
            "positionList",
            "order_list",
            "position_list",
            "data",
            "result",
        ]
        .iter()
        .find_map(|key| object.get(*key))
        {
            value = next;
        } else {
            return Err(unavailable());
        }
    }
    Err(unavailable())
}
fn text(value: &Value, keys: &[&str]) -> AppResult<String> {
    let value = keys
        .iter()
        .find_map(|key| value.get(*key))
        .and_then(Value::as_str)
        .ok_or_else(unavailable)?;
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(unavailable());
    }
    Ok(value.into())
}
fn number(value: &Value, keys: &[&str]) -> AppResult<String> {
    let value = text(value, keys)?;
    decimal(&value, false).map_err(|_| unavailable())?;
    Ok(value)
}
fn parse_balances(payload: &Value) -> AppResult<Vec<Balance>> {
    list(payload)?
        .iter()
        .take(100)
        .map(|v| {
            Ok(Balance {
                asset: text(v, &["asset", "coin", "currency"])?,
                available: number(v, &["available", "availableBalance", "available_balance"])?,
                frozen: reserved_margin(v)?,
                total: number(
                    v,
                    &[
                        "equity",
                        "total",
                        "balance",
                        "walletBalance",
                        "wallet_balance",
                    ],
                )?,
            })
        })
        .collect()
}
fn parse_positions(payload: &Value) -> AppResult<Vec<Position>> {
    list(payload)?
        .iter()
        .take(100)
        .map(|v| {
            let idx = v
                .get("positionIdx")
                .or_else(|| v.get("position_idx"))
                .ok_or_else(unavailable)?;
            let position_idx = idx
                .as_i64()
                .and_then(|i| i32::try_from(i).ok())
                .or_else(|| {
                    idx.as_str()
                        .and_then(|s| s.parse::<i32>().ok().filter(|i| i.to_string() == s))
                })
                .ok_or_else(unavailable)?;
            Ok(Position {
                symbol: text(v, &["symbol", "s"])?,
                side: text(v, &["side", "positionSide", "position_side", "direction"])?,
                size: number(
                    v,
                    &[
                        "size",
                        "qty",
                        "quantity",
                        "positionAmt",
                        "position_amt",
                        "positionQty",
                        "position_qty",
                        "holdQty",
                        "hold_qty",
                        "openQty",
                        "open_qty",
                        "currentPiece",
                        "current_piece",
                        "totalPiece",
                        "total_piece",
                    ],
                )?,
                entry_price: number(
                    v,
                    &[
                        "entryPrice",
                        "entry_price",
                        "avgPrice",
                        "avg_price",
                        "openPrice",
                        "open_price",
                    ],
                )?,
                leverage: number(v, &["leverage"])?,
                unrealised_pnl: number(
                    v,
                    &[
                        "unrealisedPnl",
                        "unrealised_pnl",
                        "unrealizedPnl",
                        "unrealized_pnl",
                        "profitUnreal",
                        "profit_unreal",
                        "pnl",
                    ],
                )?,
                position_idx,
            })
        })
        .collect()
}
fn parse_orders(payload: &Value) -> AppResult<Vec<Order>> {
    list(payload)?
        .iter()
        .take(100)
        .map(|v| {
            let status =
                OrderStatus::from_raw(&text(v, &["status", "orderStatus", "order_status"])?);
            let order_link_id = match v.get("orderLinkId").or_else(|| v.get("order_link_id")) {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) if s.is_empty() => None,
                Some(Value::String(s)) if bounded_id(s) => Some(s.clone()),
                _ => return Err(unavailable()),
            };
            Ok(Order {
                order_id: text(v, &["orderId", "order_id", "id"])?,
                symbol: text(v, &["symbol", "s"])?,
                side: text(v, &["side"])?,
                order_type: text(v, &["orderType", "order_type", "type"])?,
                price: number(v, &["price"])?,
                qty: number(v, &["qty", "quantity", "size"])?,
                status,
                order_link_id,
                filled_qty: number(
                    v,
                    &["cumExecQty", "cum_exec_qty", "filled_qty", "filledQty"],
                )?,
                avg_price: average_fill(v)?,
            })
        })
        .collect()
}
fn parse_quote(payload: &Value, symbol: &str) -> AppResult<Quote> {
    let values = list(payload);
    let direct = payload.get("data").unwrap_or(payload);
    let value = match &values {
        Ok(items) => items
            .iter()
            .find(|v| text(v, &["symbol", "s"]).is_ok_and(|s| s == symbol))
            .ok_or_else(unavailable)?,
        Err(_) => direct,
    };
    let actual = text(value, &["symbol", "s"])?;
    if actual != symbol {
        return Err(unavailable());
    }
    Ok(Quote {
        symbol: actual,
        last_price: number(value, &["lastPrice", "last_price", "price", "last"])?,
        bid_price: number(value, &["bidPrice", "bid_price", "bid1Price", "bid"])?,
        ask_price: number(value, &["askPrice", "ask_price", "ask1Price", "ask"])?,
        mark_price: number(value, &["markPrice", "mark_price"])?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn documented_exchange_shapes_derive_reserved_margin_and_fill_average_without_defaults() {
        let balances=parse_balances(&json!({"data":[{"coin":"USDT","equity":"9007199254740993.3","wallet_balance":"9007199254740993.2","position_margin":"0.1","order_margin":"0.2","available_balance":"9007199254740993"}]})).unwrap();
        assert_eq!(balances[0].frozen, "0.3");
        assert_eq!(balances[0].total, "9007199254740993.3");
        let fixture = |qty: &str, value: &str| json!({"data":[{"order_id":"fixture-order","symbol":"BTCUSDT","side":"Buy","order_type":"Limit","price":"100","qty":"2","order_status":"PartiallyFilled","cum_exec_qty":qty,"cum_exec_value":value}]});
        assert_eq!(
            parse_orders(&fixture("0.5", "50.25")).unwrap()[0].avg_price,
            "100.5"
        );
        assert_eq!(parse_orders(&fixture("0", "0")).unwrap()[0].avg_price, "0");
        let mut missing = fixture("0.5", "50.25");
        missing["data"][0]
            .as_object_mut()
            .unwrap()
            .remove("cum_exec_value");
        assert!(parse_orders(&missing).is_err());
    }
    #[test]
    fn native_production_tif_maps_only_exact_allowlisted_shorthand() {
        for (wire, exchange) in [
            ("GTC", "GoodTillCancel"),
            ("IOC", "ImmediateOrCancel"),
            ("FOK", "FillOrKill"),
        ] {
            assert_eq!(exchange_time_in_force(Some(wire)).unwrap(), exchange);
        }
        assert!(exchange_time_in_force(None).is_err());
        assert!(exchange_time_in_force(Some("PostOnly")).is_err());
    }
    #[test]
    fn pending_cancel_and_raced_terminal_statuses_remain_visible_but_not_new() {
        for (status, expected) in [
            ("PendingCancel", OrderStatus::Unknown),
            ("Cancelled", OrderStatus::Cancelled),
            ("Filled", OrderStatus::Filled),
            ("UnexpectedFutureStatus", OrderStatus::Unknown),
        ] {
            let orders=parse_orders(&json!({"data":[{"orderId":"fixture-order","symbol":"BTCUSDT","side":"Buy","orderType":"Limit","price":"1","qty":"1","status":status,"cumExecQty":"0"}]})).unwrap();
            assert_eq!(orders[0].status, expected);
        }
    }
    #[test]
    fn missing_data_does_not_fabricate_balances_or_hide_invalid_positions() {
        assert!(parse_balances(&json!({"data":[{}]})).is_err());
        assert!(parse_balances(&json!({"data":null})).is_err());
        assert!(parse_balances(&json!({})).is_err());
        let b = json!({"data":[{"asset":"USDT","available":"9007199254740993.1","frozen":"0","total":"9007199254740993.1"}]});
        assert_eq!(parse_balances(&b).unwrap()[0].total, "9007199254740993.1");
        assert!(parse_positions(&json!({"data":[{"symbol":"BTCUSDT","size":"oops"}]})).is_err());
        assert!(parse_balances(
            &json!({"data":[{"asset":"USDT","available":0,"frozen":"0","total":"0"}]})
        )
        .is_err());
        assert!(parse_balances(&json!({"data":[]})).unwrap().is_empty());
    }
    #[test]
    fn quote_requires_explicit_matching_symbol_and_all_prices_without_diagnostics() {
        let q = json!({"data":{"symbol":"BTCUSDT","lastPrice":"50000","bidPrice":"49999","askPrice":"50001","markPrice":"50000","diagnostic":"secret"}});
        let quote = parse_quote(&q, "BTCUSDT").unwrap();
        assert!(!serde_json::to_string(&quote).unwrap().contains("secret"));
        assert!(parse_quote(&q, "ETHUSDT").is_err());
        let mut q = q;
        q["data"].as_object_mut().unwrap().remove("bidPrice");
        assert!(parse_quote(&q, "BTCUSDT").is_err());
    }
    #[test]
    fn strict_nonempty_order_and_position_fixtures_preserve_decimal_strings_and_require_fields() {
        let order = serde_json::json!({"data":{"list":[{"orderId":"exchange-1","symbol":"BTCUSDT","side":"Buy","orderType":"Limit","price":"50000","qty":"0.01","status":"New","cumExecQty":"0","avgPrice":"0","rawSecret":"must-not-escape"}]}});
        let orders = parse_orders(&order).unwrap();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].qty, "0.01");
        assert!(validate_order(&orders[0], "BTCUSDT").is_ok());
        assert!(validate_order(&orders[0], "ETHUSDT").is_err());
        assert!(!serde_json::to_string(&orders)
            .unwrap()
            .contains("must-not-escape"));
        let mut missing = order;
        missing["data"]["list"][0]
            .as_object_mut()
            .unwrap()
            .remove("cumExecQty");
        assert!(parse_orders(&missing).is_err());
        let position = serde_json::json!({"data":[{"symbol":"BTCUSDT","side":"Buy","size":"0.01","entryPrice":"50000","leverage":"2","unrealisedPnl":"-0.00000001","positionIdx":1}]});
        let positions = parse_positions(&position).unwrap();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].unrealised_pnl, "-0.00000001");
        assert_eq!(positions[0].position_idx, 1);
        let mut missing = position;
        missing["data"][0]
            .as_object_mut()
            .unwrap()
            .remove("leverage");
        assert!(parse_positions(&missing).is_err());
    }
}
