use super::*;
use crate::models::order_submission::SubmissionOutcome;
use crate::plugin::strategy::AmendProposal;
use std::future::Future;

pub(super) async fn query_amend_target<F, Fut>(
    request: &AmendProposal,
    original: &PlaceOrderRequest,
    mut query: F,
) -> AppResult<Order>
where
    F: FnMut(&'static str, Vec<(String, String)>) -> Fut,
    Fut: Future<Output = AppResult<Value>>,
{
    if request.symbol != original.symbol
        || original.order_type != "Limit"
        || original.order_link_id.is_none()
        || crate::plugin::strategy::validate_amend_entry(original, &request.price).is_err()
    {
        return Err(unavailable());
    }
    let client = original.order_link_id.as_deref().ok_or_else(unavailable)?;
    for endpoint in [endpoints::OPEN_ORDERS, endpoints::ORDERS] {
        let params = build_order_query_params(
            Some(&request.symbol),
            None,
            Some(&request.order_id),
            None,
            (endpoint == endpoints::OPEN_ORDERS).then_some("Normal"),
            Some(100),
            None,
            None,
            None,
            None,
        );
        let payload = query(endpoint, params).await.map_err(|_| unavailable())?;
        let rows = list(&payload)?;
        if rows.len() > 1 {
            return Err(unavailable());
        }
        let Some(raw) = rows.first() else {
            continue;
        };
        strategy::reject_conflicting_aliases(raw)?;
        let parsed = parse_orders(&payload)?;
        let [order] = parsed.as_slice() else {
            return Err(unavailable());
        };
        if order.order_id != request.order_id
            || order.order_link_id.as_deref() != Some(client)
            || order.symbol != original.symbol
            || order.side != original.side
            || order.order_type != "Limit"
            || matches!(order.status, OrderStatus::Unknown | OrderStatus::Rejected)
            || strict_text(raw, &["order_id", "orderId", "id"])? != request.order_id
            || strict_text(raw, &["order_link_id", "orderLinkId"])? != client
            || strict_text(raw, &["symbol", "s"])? != original.symbol
            || strict_text(raw, &["side"])? != original.side
            || strict_text(raw, &["order_type", "orderType", "type"])? != "Limit"
            || normalized_tif(&strict_text(raw, &["time_in_force", "timeInForce"])?)
                != original.time_in_force.as_deref().ok_or_else(unavailable)?
            || strict_position_idx(raw)? != original.position_idx
            || strict_reduce_only(raw)? != original.reduce_only.ok_or_else(unavailable)?
            || optional_raw_text(raw, "stop_order_type", "stopOrderType")?
                .is_some_and(|value| !value.is_empty() && value != "UNKNOWN")
        {
            return Err(unavailable());
        }
        strict_protection_raw(raw, original)?;
        let qty = Decimal::from_str_exact(&order.qty).map_err(|_| unavailable())?;
        let filled = Decimal::from_str_exact(&order.filled_qty).map_err(|_| unavailable())?;
        let price = Decimal::from_str_exact(&order.price).map_err(|_| unavailable())?;
        if qty <= Decimal::ZERO || price <= Decimal::ZERO || filled < Decimal::ZERO || filled > qty
        {
            return Err(unavailable());
        }
        return Ok(order.clone());
    }
    Err(unavailable())
}

fn strict_text(raw: &Value, aliases: &[&str]) -> AppResult<String> {
    let mut found: Option<&Value> = None;
    for alias in aliases {
        if let Some(value) = raw.get(*alias) {
            if found.is_some_and(|old| old != value) {
                return Err(unavailable());
            }
            found = Some(value);
        }
    }
    let value = found.and_then(Value::as_str).ok_or_else(unavailable)?;
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(unavailable());
    }
    Ok(value.into())
}
fn normalized_tif(value: &str) -> &str {
    match value {
        "GTC" => "GoodTillCancel",
        "IOC" => "ImmediateOrCancel",
        "FOK" => "FillOrKill",
        other => other,
    }
}
fn strict_position_idx(raw: &Value) -> AppResult<i32> {
    let value = raw
        .get("position_idx")
        .or_else(|| raw.get("positionIdx"))
        .ok_or_else(unavailable)?;
    if raw
        .get("position_idx")
        .zip(raw.get("positionIdx"))
        .is_some_and(|(a, b)| a != b)
    {
        return Err(unavailable());
    }
    value
        .as_i64()
        .or_else(|| {
            value
                .as_str()
                .and_then(|s| s.parse::<i64>().ok().filter(|n| n.to_string() == s))
        })
        .and_then(|n| i32::try_from(n).ok())
        .ok_or_else(unavailable)
}
fn strict_reduce_only(raw: &Value) -> AppResult<bool> {
    let value = raw
        .get("reduce_only")
        .or_else(|| raw.get("reduceOnly"))
        .ok_or_else(unavailable)?;
    if raw
        .get("reduce_only")
        .zip(raw.get("reduceOnly"))
        .is_some_and(|(a, b)| a != b)
    {
        return Err(unavailable());
    }
    value.as_bool().ok_or_else(unavailable)
}
fn optional_raw_text<'a>(raw: &'a Value, snake: &str, camel: &str) -> AppResult<Option<&'a str>> {
    let a = raw.get(snake);
    let b = raw.get(camel);
    if a.zip(b).is_some_and(|(a, b)| a != b) {
        return Err(unavailable());
    }
    a.or(b)
        .map(|v| v.as_str().ok_or_else(unavailable))
        .transpose()
}
pub(super) fn strict_protection_raw(raw: &Value, request: &PlaceOrderRequest) -> AppResult<()> {
    let protection = request.protection.as_ref();
    if let Some(protection) = protection {
        protection.validate_shape().map_err(|_| unavailable())?;
    }
    for (expected, price_keys, trigger_keys) in [
        (
            protection.and_then(|p| p.take_profit.as_ref()),
            ("take_profit", "takeProfit"),
            ("tp_trigger_by", "tpTriggerBy"),
        ),
        (
            protection.and_then(|p| p.stop_loss.as_ref()),
            ("stop_loss", "stopLoss"),
            ("sl_trigger_by", "slTriggerBy"),
        ),
    ] {
        let raw_price = optional_raw_text(raw, price_keys.0, price_keys.1)?;
        let raw_trigger = optional_raw_text(raw, trigger_keys.0, trigger_keys.1)?;
        if let Some(expected) = expected {
            let observed = raw_price.ok_or_else(unavailable)?;
            if Decimal::from_str_exact(observed).ok() != Decimal::from_str_exact(expected).ok()
                || raw_trigger != protection.map(|p| p.trigger_by.as_str())
            {
                return Err(unavailable());
            }
        } else {
            // Official ordinary/unused-leg rows use 0.0 and UNKNOWN sentinels.
            // Only the absent state gets this tolerance; requested legs remain exact.
            if raw_price.is_some_and(|v| {
                !v.is_empty() && Decimal::from_str_exact(v).ok() != Some(Decimal::ZERO)
            }) || raw_trigger.is_some_and(|v| !v.is_empty() && v != "UNKNOWN")
            {
                return Err(unavailable());
            }
        }
    }
    Ok(())
}

pub(super) async fn acknowledge_protected<F, Fut>(
    store: &OrderSubmissionStore,
    current: &str,
    scope: &str,
    id: &str,
    expected: &Order,
    request: &PlaceOrderRequest,
    mut query: F,
) -> AppResult<()>
where
    F: FnMut(&'static str, Vec<(String, String)>) -> Fut,
    Fut: Future<Output = AppResult<Value>>,
{
    if current.is_empty() || current != scope {
        return Err(unavailable());
    }
    strategy::validate_request(id, request)?;
    if request.protection.is_none() {
        return Err(unavailable());
    }
    let record = store
        .get(scope, id)
        .map_err(|_| unavailable())?
        .ok_or_else(unavailable)?;
    if record.request != *request {
        return Err(unavailable());
    }
    strategy::matches_request(expected, request)?;
    let accepted = match record.outcome {
        SubmissionOutcome::Accepted(order) => *order,
        SubmissionOutcome::Pending => {
            let found =
                strategy::query_exact(&request.symbol, Some(id), None, Some(request), &mut query)
                    .await?
                    .ok_or_else(unavailable)?;
            if found.order_id != expected.order_id || found.status == OrderStatus::Rejected {
                return Err(unavailable());
            }
            store
                .finish(
                    scope,
                    id,
                    SubmissionOutcome::Accepted(Box::new(found.clone())),
                )
                .map_err(|_| unavailable())?;
            found
        }
        SubmissionOutcome::Rejected => return Err(unavailable()),
    };
    strategy::matches_request(&accepted, request)?;
    if accepted.order_id != expected.order_id || accepted.status == OrderStatus::Rejected {
        return Err(unavailable());
    }
    store.acknowledge(scope, id).map_err(|_| unavailable())
}

pub(super) async fn reconcile_protected<F, Fut>(
    store: &OrderSubmissionStore,
    current: &str,
    scope: &str,
    request: &PlaceOrderRequest,
    mut query: F,
) -> AppResult<Option<Order>>
where
    F: FnMut(&'static str, Vec<(String, String)>) -> Fut,
    Fut: Future<Output = AppResult<Value>>,
{
    if current.is_empty() || current != scope || request.protection.is_none() {
        return Err(unavailable());
    }
    let id = request.order_link_id.as_deref().ok_or_else(unavailable)?;
    strategy::validate_request(id, request)?;
    let record = store
        .get(scope, id)
        .map_err(|_| unavailable())?
        .ok_or_else(unavailable)?;
    if record.request != *request || matches!(record.outcome, SubmissionOutcome::Rejected) {
        return Err(unavailable());
    }
    let found =
        strategy::query_exact(&request.symbol, Some(id), None, Some(request), &mut query).await?;
    if let Some(order) = &found {
        if order.status == OrderStatus::Rejected
            || matches!(&record.outcome, SubmissionOutcome::Accepted(known) if known.order_id != order.order_id)
        {
            return Err(unavailable());
        }
        if matches!(record.outcome, SubmissionOutcome::Pending) {
            store
                .finish(
                    scope,
                    id,
                    SubmissionOutcome::Accepted(Box::new(order.clone())),
                )
                .map_err(|_| unavailable())?;
        }
    }
    Ok(found)
}

pub(super) async fn reconcile_amend<F, Fut>(
    request: &AmendProposal,
    original: &PlaceOrderRequest,
    query: F,
) -> AppResult<Option<Order>>
where
    F: FnMut(&'static str, Vec<(String, String)>) -> Fut,
    Fut: Future<Output = AppResult<Value>>,
{
    let found = query_amend_target(request, original, query).await?;
    if Decimal::from_str_exact(&found.price).ok() == Decimal::from_str_exact(&request.price).ok()
        && Decimal::from_str_exact(&found.qty).ok() == Decimal::from_str_exact(&request.qty).ok()
    {
        Ok(Some(found))
    } else {
        Ok(None)
    }
}

pub(super) fn validate_amend_change(request: &AmendProposal, target: &Order) -> AppResult<()> {
    if !matches!(
        target.status,
        OrderStatus::New | OrderStatus::PartiallyFilled
    ) {
        return Err(unavailable());
    }
    let qty = Decimal::from_str_exact(&request.qty).map_err(|_| unavailable())?;
    let price = Decimal::from_str_exact(&request.price).map_err(|_| unavailable())?;
    let current_qty = Decimal::from_str_exact(&target.qty).map_err(|_| unavailable())?;
    let current_price = Decimal::from_str_exact(&target.price).map_err(|_| unavailable())?;
    let filled = Decimal::from_str_exact(&target.filled_qty).map_err(|_| unavailable())?;
    if filled >= current_qty
        || qty <= filled
        || qty > current_qty
        || (qty == current_qty && price == current_price)
    {
        return Err(unavailable());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    struct TestDir(std::path::PathBuf);
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn original() -> PlaceOrderRequest {
        PlaceOrderRequest {
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Limit".into(),
            qty: "0.001".into(),
            position_idx: 1,
            price: Some("50000".into()),
            time_in_force: Some("GoodTillCancel".into()),
            order_link_id: Some("client-1".into()),
            reduce_only: Some(false),
            protection: None,
        }
    }
    fn raw() -> Value {
        json!({"data":[{"order_id":"exchange-1","order_link_id":"client-1","symbol":"BTCUSDT",
            "side":"Buy","order_type":"Limit","qty":"0.001","price":"50000",
            "order_status":"New","cum_exec_qty":"0","avg_price":"0",
            "time_in_force":"GoodTillCancel","position_idx":1,"reduce_only":false}]})
    }
    fn documented_no_protection_raw() -> Value {
        let mut payload = raw();
        payload["data"][0]["stop_order_type"] = json!("UNKNOWN");
        payload["data"][0]["take_profit"] = json!("0.0");
        payload["data"][0]["stop_loss"] = json!("0.0");
        payload["data"][0]["tp_trigger_by"] = json!("UNKNOWN");
        payload["data"][0]["sl_trigger_by"] = json!("UNKNOWN");
        payload
    }
    #[tokio::test]
    async fn documented_raw_no_condition_sentinels_allow_ordinary_amend_but_not_conflicts() {
        let amend = AmendProposal {
            symbol: "BTCUSDT".into(),
            order_id: "exchange-1".into(),
            price: "50010".into(),
            qty: "0.0008".into(),
        };
        let documented = documented_no_protection_raw();
        assert!(query_amend_target(&amend, &original(), |_, _| {
            std::future::ready(Ok(documented.clone()))
        })
        .await
        .is_ok());
        let mut conflicting = documented.clone();
        conflicting["data"][0]["stopOrderType"] = json!("Stop");
        assert!(query_amend_target(&amend, &original(), |_, _| {
            std::future::ready(Ok(conflicting.clone()))
        })
        .await
        .is_err());
        let mut active = documented;
        active["data"][0]["stop_loss"] = json!("45000");
        assert!(query_amend_target(&amend, &original(), |_, _| {
            std::future::ready(Ok(active.clone()))
        })
        .await
        .is_err());
    }
    #[test]
    fn documented_unused_leg_sentinels_allow_one_leg_protected_identity() {
        let mut request = original();
        request.protection = Some(crate::models::trading::OrderProtection {
            take_profit: Some("55000".into()),
            stop_loss: None,
            trigger_by: "LastPrice".into(),
        });
        let mut documented = documented_no_protection_raw();
        documented["data"][0]["take_profit"] = json!("55000");
        documented["data"][0]["tp_trigger_by"] = json!("LastPrice");
        assert!(strict_protection_raw(&documented["data"][0], &request).is_ok());
        documented["data"][0]["sl_trigger_by"] = json!("LastPrice");
        assert!(strict_protection_raw(&documented["data"][0], &request).is_err());
    }
    #[tokio::test]
    async fn protected_market_id_only_acknowledges_production_journal_without_observed_price() {
        let id = uuid::Uuid::new_v4().to_string();
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/strategy-production-tests")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();
        let temp = TestDir(root);
        let store = OrderSubmissionStore::with_dir(temp.0.clone());
        let mut request = original();
        request.order_link_id = Some(id.clone());
        request.order_type = "Market".into();
        request.price = None;
        request.time_in_force = Some("ImmediateOrCancel".into());
        request.protection = Some(crate::models::trading::OrderProtection {
            take_profit: Some("55000".into()),
            stop_loss: Some("45000".into()),
            trigger_by: "LastPrice".into(),
        });
        let accepted = crate::services::order_submission::submit_once(
            &store,
            "scope",
            request.clone(),
            1,
            |actual| async move {
                crate::api::PrivateApi::parse_protected_create_ack(
                    &json!({"code":0,"data":{"order_id":"exchange-market","order_link_id":""}}),
                    &actual,
                )
            },
        )
        .await
        .unwrap();
        assert_eq!(accepted.price, "");
        assert_eq!(accepted.status, OrderStatus::Unknown);
        acknowledge_protected(
            &store,
            "scope",
            "scope",
            &id,
            &accepted,
            &request,
            |_, _| -> std::future::Ready<AppResult<Value>> { panic!("known ack must not query") },
        )
        .await
        .unwrap();
        assert!(store.get("scope", &id).unwrap().unwrap().acknowledged);
    }
    #[tokio::test]
    async fn amend_target_requires_exact_raw_immutable_identity() {
        let amend = AmendProposal {
            symbol: "BTCUSDT".into(),
            order_id: "exchange-1".into(),
            price: "50010".into(),
            qty: "0.0008".into(),
        };
        let valid =
            query_amend_target(&amend, &original(), |_, _| std::future::ready(Ok(raw()))).await;
        assert_eq!(valid.unwrap().order_id, "exchange-1");
        for key in [
            "order_link_id",
            "side",
            "order_type",
            "time_in_force",
            "position_idx",
            "reduce_only",
        ] {
            let mut missing = raw();
            missing["data"][0].as_object_mut().unwrap().remove(key);
            assert!(
                query_amend_target(&amend, &original(), |_, _| std::future::ready(Ok(
                    missing.clone()
                )))
                .await
                .is_err(),
                "missing {key}"
            );
        }
    }
    #[tokio::test]
    async fn protected_owned_amend_requires_unchanged_raw_protection() {
        let amend = AmendProposal {
            symbol: "BTCUSDT".into(),
            order_id: "exchange-1".into(),
            price: "50010".into(),
            qty: "0.0008".into(),
        };
        let mut protected = original();
        protected.protection = Some(crate::models::trading::OrderProtection {
            take_profit: Some("55000".into()),
            stop_loss: Some("45000".into()),
            trigger_by: "LastPrice".into(),
        });
        let mut observed = raw();
        observed["data"][0]["take_profit"] = json!("55000");
        observed["data"][0]["stop_loss"] = json!("45000");
        observed["data"][0]["tp_trigger_by"] = json!("LastPrice");
        observed["data"][0]["sl_trigger_by"] = json!("LastPrice");
        let target = query_amend_target(&amend, &protected, |_, _| {
            std::future::ready(Ok(observed.clone()))
        })
        .await
        .unwrap();
        assert!(validate_amend_change(&amend, &target).is_ok());
        for key in ["take_profit", "stop_loss", "tp_trigger_by", "sl_trigger_by"] {
            let mut missing = observed.clone();
            missing["data"][0].as_object_mut().unwrap().remove(key);
            assert!(
                query_amend_target(&amend, &protected, |_, _| {
                    std::future::ready(Ok(missing.clone()))
                })
                .await
                .is_err(),
                "missing {key}"
            );
        }
        let mut changed = observed.clone();
        changed["data"][0]["take_profit"] = json!("56000");
        assert!(query_amend_target(&amend, &protected, |_, _| {
            std::future::ready(Ok(changed.clone()))
        })
        .await
        .is_err());
        assert!(
            query_amend_target(&amend, &original(), |_, _| {
                std::future::ready(Ok(observed.clone()))
            })
            .await
            .is_err(),
            "unprotected ownership cannot inherit raw exchange protection"
        );
    }
    #[tokio::test]
    async fn protected_owned_amend_preflight_rejects_entry_beyond_immutable_legs() {
        let mut protected = original();
        protected.protection = Some(crate::models::trading::OrderProtection {
            take_profit: Some("55000".into()),
            stop_loss: Some("45000".into()),
            trigger_by: "LastPrice".into(),
        });
        let mut observed = raw();
        observed["data"][0]["take_profit"] = json!("55000");
        observed["data"][0]["stop_loss"] = json!("45000");
        observed["data"][0]["tp_trigger_by"] = json!("LastPrice");
        observed["data"][0]["sl_trigger_by"] = json!("LastPrice");
        for crossed_price in ["55000", "56000", "45000", "44000"] {
            let amend = AmendProposal {
                symbol: "BTCUSDT".into(),
                order_id: "exchange-1".into(),
                price: crossed_price.into(),
                qty: "0.0008".into(),
            };
            assert!(
                query_amend_target(&amend, &protected, |_, _| {
                    std::future::ready(Ok(observed.clone()))
                })
                .await
                .is_err(),
                "crossed price {crossed_price}"
            );
        }
    }
    #[test]
    fn protected_raw_recovery_requires_every_requested_leg_and_selector() {
        let mut request = original();
        request.protection = Some(crate::models::trading::OrderProtection {
            take_profit: Some("55000".into()),
            stop_loss: Some("45000".into()),
            trigger_by: "LastPrice".into(),
        });
        let raw = json!({"take_profit":"55000","stop_loss":"45000",
            "tp_trigger_by":"LastPrice","sl_trigger_by":"LastPrice"});
        assert!(strict_protection_raw(&raw, &request).is_ok());
        for key in ["take_profit", "stop_loss", "tp_trigger_by", "sl_trigger_by"] {
            let mut invalid = raw.clone();
            invalid.as_object_mut().unwrap().remove(key);
            assert!(
                strict_protection_raw(&invalid, &request).is_err(),
                "missing {key}"
            );
        }
        let mut conflict = raw;
        conflict["takeProfit"] = json!("56000");
        assert!(strict_protection_raw(&conflict, &request).is_err());
    }
    #[tokio::test]
    async fn protected_exact_query_cannot_recover_from_unprotected_normalized_order() {
        let mut request = original();
        request.protection = Some(crate::models::trading::OrderProtection {
            take_profit: Some("55000".into()),
            stop_loss: Some("45000".into()),
            trigger_by: "LastPrice".into(),
        });
        let mut raw = raw();
        raw["data"][0]["take_profit"] = json!("55000");
        raw["data"][0]["stop_loss"] = json!("45000");
        raw["data"][0]["tp_trigger_by"] = json!("LastPrice");
        raw["data"][0]["sl_trigger_by"] = json!("LastPrice");
        let found = strategy::query_exact(
            "BTCUSDT",
            Some("client-1"),
            None,
            Some(&request),
            &mut |_, _| std::future::ready(Ok(raw.clone())),
        )
        .await
        .unwrap();
        assert_eq!(found.unwrap().order_id, "exchange-1");
        raw["data"][0].as_object_mut().unwrap().remove("stop_loss");
        assert!(strategy::query_exact(
            "BTCUSDT",
            Some("client-1"),
            None,
            Some(&request),
            &mut |_, _| std::future::ready(Ok(raw.clone()))
        )
        .await
        .is_err());
    }
    #[tokio::test]
    async fn amendment_reconciliation_requires_desired_price_and_total_quantity() {
        let amend = AmendProposal {
            symbol: "BTCUSDT".into(),
            order_id: "exchange-1".into(),
            price: "50010".into(),
            qty: "0.0008".into(),
        };
        let old = reconcile_amend(&amend, &original(), |_, _| std::future::ready(Ok(raw())))
            .await
            .unwrap();
        assert!(old.is_none());
        let mut updated = raw();
        updated["data"][0]["price"] = json!("50010");
        updated["data"][0]["qty"] = json!("0.0008");
        let found = reconcile_amend(&amend, &original(), |_, _| {
            std::future::ready(Ok(updated.clone()))
        })
        .await
        .unwrap();
        assert_eq!(found.unwrap().qty, "0.0008");
        let mut filled = updated.clone();
        filled["data"][0]["order_status"] = json!("Filled");
        filled["data"][0]["cum_exec_qty"] = json!("0.0008");
        assert!(
            reconcile_amend(&amend, &original(), |_, _| std::future::ready(Ok(
                filled.clone()
            )))
            .await
            .unwrap()
            .is_some()
        );
        updated["data"][0]["order_link_id"] = json!("foreign");
        assert!(
            reconcile_amend(&amend, &original(), |_, _| std::future::ready(Ok(
                updated.clone()
            )))
            .await
            .is_err()
        );
    }
}
