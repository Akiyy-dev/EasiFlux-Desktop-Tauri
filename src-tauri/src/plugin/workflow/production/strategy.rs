//! Native-only exact-identity recovery; never submits or cancels an order.
use super::*;
use crate::models::order_submission::SubmissionOutcome;
use std::future::Future;

pub(super) async fn acknowledge<F, Fut>(
    store: &OrderSubmissionStore,
    current: &str,
    scope: &str,
    id: &str,
    expected: &Order,
    proposal: &PlaceProposal,
    mut query: F,
) -> AppResult<()>
where
    F: FnMut(&'static str, Vec<(String, String)>) -> Fut,
    Fut: Future<Output = AppResult<Value>>,
{
    check_scope(current, scope)?;
    let canonical = canonical_request(id, proposal)?;
    let record = store
        .get(scope, id)
        .map_err(|_| unavailable())?
        .ok_or_else(unavailable)?;
    if serde_json::to_value(&canonical).map_err(|_| unavailable())?
        != serde_json::to_value(&record.request).map_err(|_| unavailable())?
    {
        return Err(unavailable());
    }
    matches_request(expected, &canonical)?;
    let accepted = match record.outcome {
        SubmissionOutcome::Accepted(order) => *order,
        SubmissionOutcome::Pending => {
            let order = query_exact(
                &canonical.symbol,
                Some(id),
                None,
                Some(&canonical),
                &mut query,
            )
            .await?
            .ok_or_else(unavailable)?;
            // Never publish a different exchange identity into the journal, even when
            // the exchange echoed the original client identity.
            if order.order_id != expected.order_id || order.status == OrderStatus::Rejected {
                return Err(unavailable());
            }
            store
                .finish(
                    scope,
                    id,
                    SubmissionOutcome::Accepted(Box::new(order.clone())),
                )
                .map_err(|_| unavailable())?;
            order
        }
        SubmissionOutcome::Rejected => return Err(unavailable()),
    };
    matches_request(&accepted, &canonical)?;
    if accepted.order_id != expected.order_id || accepted.status == OrderStatus::Rejected {
        return Err(unavailable());
    }
    store.acknowledge(scope, id).map_err(|_| unavailable())
}

pub(super) async fn reconcile<F, Fut>(
    store: &OrderSubmissionStore,
    current: &str,
    scope: &str,
    symbol_value: &str,
    submission: Option<&str>,
    exchange: Option<&str>,
    mut query: F,
) -> AppResult<Option<Order>>
where
    F: FnMut(&'static str, Vec<(String, String)>) -> Fut,
    Fut: Future<Output = AppResult<Value>>,
{
    check_scope(current, scope)?;
    symbol(symbol_value).map_err(|_| unavailable())?;
    match (submission, exchange) {
        (Some(id), None) => {
            let Some(record) = store.get(scope, id).map_err(|_| unavailable())? else {
                return Ok(None);
            };
            validate_request(id, &record.request)?;
            if record.request.symbol != symbol_value {
                return Err(unavailable());
            }
            match record.outcome {
                SubmissionOutcome::Accepted(order) => {
                    matches_request(&order, &record.request)?;
                    if order.status == OrderStatus::Rejected {
                        return Err(unavailable());
                    }
                    return Ok(Some(*order));
                }
                SubmissionOutcome::Rejected => return Err(rejected()),
                SubmissionOutcome::Pending => {}
            }
            let found = query_exact(
                symbol_value,
                Some(id),
                None,
                Some(&record.request),
                &mut query,
            )
            .await?;
            if let Some(order) = &found {
                let outcome = if order.status == OrderStatus::Rejected {
                    SubmissionOutcome::Rejected
                } else {
                    SubmissionOutcome::Accepted(Box::new(order.clone()))
                };
                store
                    .finish(scope, id, outcome)
                    .map_err(|_| unavailable())?;
                if order.status == OrderStatus::Rejected {
                    return Err(rejected());
                }
            }
            Ok(found)
        }
        (None, Some(id)) if bounded_id(id) => {
            query_exact(symbol_value, None, Some(id), None, &mut query).await
        }
        _ => Err(unavailable()),
    }
}

fn check_scope(current: &str, scope: &str) -> AppResult<()> {
    if scope.is_empty() || current != scope {
        return Err(unavailable());
    }
    Ok(())
}
fn rejected() -> crate::error::AppError {
    crate::error::AppError::OrderSubmissionRejected("plugin_strategy_rejected".into())
}
fn canonical_request(id: &str, proposal: &PlaceProposal) -> AppResult<PlaceOrderRequest> {
    if uuid::Uuid::parse_str(id).is_err() || id.len() != 36 {
        return Err(unavailable());
    }
    let validated = WorkflowOutput::PlaceOrder {
        order: proposal.clone(),
    }
    .validate(&proposal.symbol, &["trade.place".into()], None)
    .map_err(|_| unavailable())?;
    let WorkflowOutput::PlaceOrder { order } = validated else {
        return Err(unavailable());
    };
    Ok(PlaceOrderRequest {
        symbol: order.symbol,
        side: order.side,
        order_type: order.order_type,
        qty: order.qty,
        price: order.price,
        time_in_force: Some(exchange_time_in_force(Some(&order.time_in_force))?.into()),
        position_idx: order.position_idx,
        reduce_only: Some(order.reduce_only),
        order_link_id: Some(id.into()),
    })
}
fn validate_request(id: &str, request: &PlaceOrderRequest) -> AppResult<()> {
    let tif = match request.time_in_force.as_deref() {
        Some("GoodTillCancel") => "GTC",
        Some("ImmediateOrCancel") => "IOC",
        Some("FillOrKill") => "FOK",
        _ => return Err(unavailable()),
    };
    let canonical = canonical_request(
        id,
        &PlaceProposal {
            symbol: request.symbol.clone(),
            side: request.side.clone(),
            order_type: request.order_type.clone(),
            qty: request.qty.clone(),
            price: request.price.clone(),
            time_in_force: tif.into(),
            position_idx: request.position_idx,
            reduce_only: request.reduce_only.ok_or_else(unavailable)?,
        },
    )?;
    if serde_json::to_value(canonical).map_err(|_| unavailable())?
        != serde_json::to_value(request).map_err(|_| unavailable())?
    {
        return Err(unavailable());
    }
    Ok(())
}
fn matches_request(order: &Order, request: &PlaceOrderRequest) -> AppResult<()> {
    validate_order(order, &request.symbol).map_err(|_| unavailable())?;
    if order.order_link_id != request.order_link_id
        || order.side != request.side
        || order.order_type != request.order_type
        || decimal(&order.qty, true).map_err(|_| unavailable())?
            != decimal(&request.qty, true).map_err(|_| unavailable())?
        || request
            .price
            .as_ref()
            .is_some_and(|price| decimal(&order.price, true).ok() != decimal(price, true).ok())
    {
        return Err(unavailable());
    }
    Ok(())
}

async fn query_exact<F, Fut>(
    symbol: &str,
    submission: Option<&str>,
    exchange: Option<&str>,
    request: Option<&PlaceOrderRequest>,
    query: &mut F,
) -> AppResult<Option<Order>>
where
    F: FnMut(&'static str, Vec<(String, String)>) -> Fut,
    Fut: Future<Output = AppResult<Value>>,
{
    for endpoint in [endpoints::OPEN_ORDERS, endpoints::ORDERS] {
        let params = build_order_query_params(
            Some(symbol),
            None,
            exchange,
            submission,
            (endpoint == endpoints::OPEN_ORDERS).then_some("Normal"),
            Some(100),
            None,
            None,
            None,
            None,
        );
        let payload = query(endpoint, params).await.map_err(|_| unavailable())?;
        let items = list(&payload)?;
        if items.len() > 1 {
            return Err(unavailable());
        }
        let Some(raw) = items.first() else {
            continue;
        };
        reject_conflicting_aliases(raw)?;
        let order = parse_orders(&payload)?
            .into_iter()
            .next()
            .ok_or_else(unavailable)?;
        validate_order(&order, symbol).map_err(|_| unavailable())?;
        if submission.is_some_and(|id| order.order_link_id.as_deref() != Some(id))
            || exchange.is_some_and(|id| order.order_id != id)
        {
            return Err(unavailable());
        }
        for value in [
            &order.price,
            &order.qty,
            &order.filled_qty,
            &order.avg_price,
        ] {
            if Decimal::from_str_exact(value).map_err(|_| unavailable())? < Decimal::ZERO {
                return Err(unavailable());
            }
        }
        if Decimal::from_str_exact(&order.qty).map_err(|_| unavailable())? <= Decimal::ZERO
            || Decimal::from_str_exact(&order.filled_qty).map_err(|_| unavailable())?
                > Decimal::from_str_exact(&order.qty).map_err(|_| unavailable())?
        {
            return Err(unavailable());
        }
        if let Some(request) = request {
            matches_request(&order, request)?;
            let tif = text(raw, &["timeInForce", "time_in_force"])?;
            let tif = match tif.as_str() {
                "GTC" => "GoodTillCancel",
                "IOC" => "ImmediateOrCancel",
                "FOK" => "FillOrKill",
                other => other,
            };
            let idx = raw
                .get("positionIdx")
                .or_else(|| raw.get("position_idx"))
                .ok_or_else(unavailable)?;
            let idx = idx
                .as_i64()
                .or_else(|| {
                    idx.as_str()
                        .and_then(|s| s.parse::<i64>().ok().filter(|n| n.to_string() == s))
                })
                .ok_or_else(unavailable)?;
            let reduce = raw
                .get("reduceOnly")
                .or_else(|| raw.get("reduce_only"))
                .and_then(Value::as_bool)
                .ok_or_else(unavailable)?;
            if request.time_in_force.as_deref() != Some(tif)
                || i64::from(request.position_idx) != idx
                || request.reduce_only != Some(reduce)
            {
                return Err(unavailable());
            }
        }
        return Ok(Some(order));
    }
    Ok(None)
}

fn reject_conflicting_aliases(raw: &Value) -> AppResult<()> {
    // The snapshot parser supports several documented spelling variants. Recovery
    // cannot use first-alias-wins semantics when a payload supplies contradictions.
    for aliases in [
        &["orderId", "order_id", "id"][..],
        &["orderLinkId", "order_link_id"],
        &["symbol", "s"],
        &["orderType", "order_type", "type"],
        &["status", "orderStatus", "order_status"],
        &["qty", "quantity", "size"],
        &["cumExecQty", "cum_exec_qty", "filled_qty", "filledQty"],
        &["avgPrice", "avg_price"],
        &["cumExecValue", "cum_exec_value"],
        &["timeInForce", "time_in_force"],
        &["positionIdx", "position_idx"],
        &["reduceOnly", "reduce_only"],
    ] {
        let mut values = aliases.iter().filter_map(|key| raw.get(*key));
        if let Some(first) = values.next() {
            if values.any(|value| value != first) {
                return Err(unavailable());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
