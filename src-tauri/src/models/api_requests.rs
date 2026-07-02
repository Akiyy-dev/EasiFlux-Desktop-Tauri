use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

fn omit_null_fields(value: Value) -> Value {
    if let Some(obj) = value.as_object() {
        let cleaned: Map<String, Value> = obj
            .iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        return Value::Object(cleaned);
    }
    value
}

/// API request payloads (snake_case) aligned with EasiFlux-SDK v0.3 models.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApiOrderRequest {
    pub symbol: String,
    pub side: String,
    pub qty: String,
    pub position_idx: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_link_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
}

impl ApiOrderRequest {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(json!({}))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApiCancelOrderRequest {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_link_id: Option<String>,
}

impl ApiCancelOrderRequest {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(json!({}))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApiCancelAllOrdersRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_filter: Option<String>,
}

impl ApiCancelAllOrdersRequest {
    pub fn to_value(&self) -> Value {
        omit_null_fields(json!({
            "symbol": self.symbol,
            "coin": self.coin,
            "order_filter": self.order_filter,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiReplaceOrderRequest {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_link_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qty: Option<String>,
}

impl ApiReplaceOrderRequest {
    pub fn to_value(&self) -> Value {
        omit_null_fields(json!({
            "symbol": self.symbol,
            "order_id": self.order_id,
            "order_link_id": self.order_link_id,
            "price": self.price,
            "qty": self.qty,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiSetLeverageRequest {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buy_leverage: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sell_leverage: Option<i32>,
}

impl ApiSetLeverageRequest {
    pub fn to_value(&self) -> Value {
        omit_null_fields(json!({
            "symbol": self.symbol,
            "buy_leverage": self.buy_leverage,
            "sell_leverage": self.sell_leverage,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiAddMarginRequest {
    pub symbol: String,
    pub position_idx: i32,
    pub margin: String,
}

impl ApiAddMarginRequest {
    pub fn to_value(&self) -> Value {
        omit_null_fields(json!({
            "symbol": self.symbol,
            "position_idx": self.position_idx,
            "margin": self.margin,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ApiCloseAllPositionsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_idx: Option<i32>,
}

impl ApiCloseAllPositionsRequest {
    pub fn to_value(&self) -> Value {
        omit_null_fields(json!({
            "symbol": self.symbol,
            "coin": self.coin,
            "position_idx": self.position_idx,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCreateTpslRequest {
    pub symbol: String,
    pub position_idx: i32,
    pub tp_sl_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_loss: Option<String>,
}

impl ApiCreateTpslRequest {
    pub fn to_value(&self) -> Value {
        omit_null_fields(json!({
            "symbol": self.symbol,
            "position_idx": self.position_idx,
            "tp_sl_mode": self.tp_sl_mode,
            "take_profit": self.take_profit,
            "stop_loss": self.stop_loss,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiReplaceTpslRequest {
    pub symbol: String,
    pub order_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_loss: Option<String>,
}

impl ApiReplaceTpslRequest {
    pub fn to_value(&self) -> Value {
        omit_null_fields(json!({
            "symbol": self.symbol,
            "order_id": self.order_id,
            "take_profit": self.take_profit,
            "stop_loss": self.stop_loss,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiSwitchMarginModeRequest {
    pub symbol: String,
    pub margin_mode: String,
}

impl ApiSwitchMarginModeRequest {
    pub fn to_value(&self) -> Value {
        omit_null_fields(json!({
            "symbol": self.symbol,
            "margin_mode": self.margin_mode,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiSwitchSeparatePositionModeRequest {
    pub coin: String,
    pub position_mode: String,
}

impl ApiSwitchSeparatePositionModeRequest {
    pub fn to_value(&self) -> Value {
        omit_null_fields(json!({
            "coin": self.coin,
            "position_mode": self.position_mode,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTransferRequest {
    pub amount: String,
    pub coin: String,
    pub from_wallet: String,
    pub to_wallet: String,
}

impl ApiTransferRequest {
    pub fn to_value(&self) -> Value {
        omit_null_fields(json!({
            "amount": self.amount,
            "coin": self.coin,
            "from_wallet": self.from_wallet,
            "to_wallet": self.to_wallet,
        }))
    }
}

/// Build query params from optional fields (snake_case keys).
pub fn build_query(params: &[(&str, Option<String>)]) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for (key, value) in params {
        if let Some(v) = value {
            map.insert((*key).to_string(), v.clone());
        }
    }
    map
}

pub fn build_query_from_json(value: &Value) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            if v.is_null() {
                continue;
            }
            if let Some(s) = v.as_str() {
                map.insert(k.clone(), s.to_string());
            } else if v.is_number() || v.is_boolean() {
                map.insert(k.clone(), v.to_string());
            }
        }
    }
    map
}

pub fn optional_i64_query(key: &str, value: Option<i64>) -> Option<(String, String)> {
    value.map(|v| (key.to_string(), v.to_string()))
}

pub fn merge_query(
    base: std::collections::HashMap<String, String>,
    extra: std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    let mut out = base;
    out.extend(extra);
    out
}

pub fn json_map(entries: &[(&str, Value)]) -> Value {
    let mut map = Map::new();
    for (k, v) in entries {
        map.insert((*k).to_string(), v.clone());
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_all_orders_api_payload_snake_case() {
        let req = ApiCancelAllOrdersRequest {
            symbol: Some("BTCUSDT".into()),
            coin: None,
            order_filter: None,
        };
        let body = req.to_value();
        assert_eq!(body["symbol"], "BTCUSDT");
        assert!(body.get("coin").is_none());
    }
}
