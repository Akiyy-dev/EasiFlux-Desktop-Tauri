use std::collections::HashMap;

use serde_json::Value;

use crate::models::account::Balance;
use crate::models::market::{Depth, DepthLevel, Kline, Ticker};
use crate::models::trading::{Order, OrderStatus, Position};

use super::response::{extract_data, extract_list, get_str};

pub fn parse_ticker(value: &Value, symbol: &str) -> Ticker {
    Ticker {
        symbol: get_str(value, &["symbol"]).unwrap_or_else(|| symbol.to_string()),
        last_price: get_str(value, &["lastPrice", "last_price", "last"]).unwrap_or_else(|| "0".into()),
        bid_price: get_str(value, &["bidPrice", "bid_price", "bid"]).unwrap_or_else(|| "0".into()),
        ask_price: get_str(value, &["askPrice", "ask_price", "ask"]).unwrap_or_else(|| "0".into()),
        volume_24h: get_str(value, &["volume24h", "volume_24h", "volume"]).unwrap_or_else(|| "0".into()),
        change_24h_pct: get_str(value, &["change24hPct", "change_24h_pct", "change"]).unwrap_or_else(|| "0".into()),
    }
}

pub fn parse_depth(value: &Value, symbol: &str) -> Depth {
    let data = extract_data(value);
    let parse_levels = |key: &str| -> Vec<DepthLevel> {
        data.get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        if let Some(pair) = item.as_array() {
                            if pair.len() >= 2 {
                                return Some(DepthLevel {
                                    price: pair[0].to_string().trim_matches('"').to_string(),
                                    qty: pair[1].to_string().trim_matches('"').to_string(),
                                });
                            }
                        }
                        if let Some(obj) = item.as_object() {
                            return Some(DepthLevel {
                                price: get_str(&Value::Object(obj.clone()), &["price", "p"]).unwrap_or_default(),
                                qty: get_str(&Value::Object(obj.clone()), &["qty", "size", "q"]).unwrap_or_default(),
                            });
                        }
                        None
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    Depth {
        symbol: get_str(data, &["symbol"]).unwrap_or_else(|| symbol.to_string()),
        bids: parse_levels("bids"),
        asks: parse_levels("asks"),
    }
}

pub fn parse_klines(payload: &Value, symbol: &str, interval: &str) -> Vec<Kline> {
    extract_list(payload)
        .iter()
        .filter_map(|item| {
            if let Some(arr) = item.as_array() {
                if arr.len() >= 6 {
                    return Some(Kline {
                        symbol: symbol.to_string(),
                        interval: interval.to_string(),
                        open_time: arr[0].as_i64().unwrap_or(0),
                        open: arr[1].to_string().trim_matches('"').to_string(),
                        high: arr[2].to_string().trim_matches('"').to_string(),
                        low: arr[3].to_string().trim_matches('"').to_string(),
                        close: arr[4].to_string().trim_matches('"').to_string(),
                        volume: arr[5].to_string().trim_matches('"').to_string(),
                    });
                }
            }
            if let Some(obj) = item.as_object() {
                let v = Value::Object(obj.clone());
                return Some(Kline {
                    symbol: symbol.to_string(),
                    interval: interval.to_string(),
                    open_time: get_str(&v, &["openTime", "open_time", "t"])
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0),
                    open: get_str(&v, &["open", "o"]).unwrap_or_else(|| "0".into()),
                    high: get_str(&v, &["high", "h"]).unwrap_or_else(|| "0".into()),
                    low: get_str(&v, &["low", "l"]).unwrap_or_else(|| "0".into()),
                    close: get_str(&v, &["close", "c"]).unwrap_or_else(|| "0".into()),
                    volume: get_str(&v, &["volume", "v"]).unwrap_or_else(|| "0".into()),
                });
            }
            None
        })
        .collect()
}

pub fn parse_order(value: &Value) -> Order {
    Order {
        order_id: get_str(value, &["orderId", "order_id", "id"]).unwrap_or_default(),
        symbol: get_str(value, &["symbol"]).unwrap_or_default(),
        side: get_str(value, &["side"]).unwrap_or_default(),
        order_type: get_str(value, &["orderType", "order_type", "type"]).unwrap_or_default(),
        price: get_str(value, &["price"]).unwrap_or_else(|| "0".into()),
        qty: get_str(value, &["qty", "quantity", "size"]).unwrap_or_else(|| "0".into()),
        status: OrderStatus::from_raw(
            &get_str(value, &["status", "orderStatus", "order_status"]).unwrap_or_default(),
        ),
        order_link_id: get_str(value, &["orderLinkId", "order_link_id"]),
        filled_qty: get_str(value, &["cumExecQty", "filled_qty", "filledQty"]).unwrap_or_else(|| "0".into()),
        avg_price: get_str(value, &["avgPrice", "avg_price"]).unwrap_or_else(|| "0".into()),
    }
}

pub fn parse_orders(payload: &Value) -> Vec<Order> {
    extract_list(payload).iter().map(|v| parse_order(v)).collect()
}

pub fn parse_position(value: &Value) -> Position {
    Position {
        symbol: get_str(value, &["symbol"]).unwrap_or_default(),
        side: get_str(value, &["side", "positionSide", "position_side"]).unwrap_or_default(),
        size: get_str(value, &["size", "qty", "positionAmt", "position_amt"]).unwrap_or_else(|| "0".into()),
        entry_price: get_str(value, &["entryPrice", "entry_price", "avgPrice"]).unwrap_or_else(|| "0".into()),
        leverage: get_str(value, &["leverage"]).unwrap_or_else(|| "1".into()),
        unrealised_pnl: get_str(value, &["unrealisedPnl", "unrealised_pnl", "unrealizedPnl"]).unwrap_or_else(|| "0".into()),
    }
}

pub fn parse_positions(payload: &Value) -> Vec<Position> {
    extract_list(payload)
        .iter()
        .map(|v| parse_position(v))
        .filter(|p| p.size != "0" && !p.size.is_empty())
        .collect()
}

pub fn parse_balance(value: &Value) -> Balance {
    let available = get_str(value, &["available", "availableBalance", "available_balance"]).unwrap_or_else(|| "0".into());
    let frozen = get_str(value, &["frozen", "locked", "frozenBalance"]).unwrap_or_else(|| "0".into());
    let total = get_str(value, &["total", "balance", "equity"]).unwrap_or_else(|| available.clone());
    Balance {
        asset: get_str(value, &["asset", "coin", "currency"]).unwrap_or_else(|| "USDT".into()),
        available,
        frozen,
        total,
    }
}

pub fn parse_balances(payload: &Value) -> Vec<Balance> {
    extract_list(payload).iter().map(|v| parse_balance(v)).collect()
}

pub fn build_kline_params(symbol: &str, interval: &str, limit: u32) -> HashMap<String, String> {
    let mut params = HashMap::new();
    params.insert("symbol".into(), symbol.into());
    params.insert("interval".into(), interval.into());
    params.insert("limit".into(), limit.to_string());
    params
}

pub fn build_depth_params(symbol: &str, limit: u32) -> HashMap<String, String> {
    let mut params = HashMap::new();
    params.insert("symbol".into(), symbol.into());
    params.insert("limit".into(), limit.to_string());
    params
}

pub fn build_order_query_params(symbol: Option<&str>) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(s) = symbol {
        params.insert("symbol".into(), s.into());
    }
    params
}

pub fn build_place_order_body(req: &crate::models::trading::PlaceOrderRequest) -> Value {
    let mut body = serde_json::Map::new();
    body.insert("symbol".into(), req.symbol.clone().into());
    body.insert("side".into(), req.side.clone().into());
    body.insert("orderType".into(), req.order_type.clone().into());
    body.insert("qty".into(), req.qty.clone().into());
    if let Some(price) = &req.price {
        body.insert("price".into(), price.clone().into());
    }
    Value::Object(body)
}

pub fn build_cancel_order_body(symbol: &str, order_id: &str) -> Value {
    serde_json::json!({
        "symbol": symbol,
        "orderId": order_id,
    })
}
