use std::collections::HashMap;

use serde_json::Value;

use crate::models::api_requests::{ApiCancelOrderRequest, ApiOrderRequest};
use crate::models::account::Balance;
use crate::models::market::{Depth, DepthLevel, Kline, Ticker};
use crate::models::trading::{Order, OrderStatus, Position};

use super::response::{extract_data, extract_list, get_str};

fn normalize_open_time_ms(value: i64) -> i64 {
    if value > 0 && value < 10_000_000_000 {
        value * 1000
    } else {
        value
    }
}

fn parse_i64_value(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().map(|n| n as i64))
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

const TICKER_LAST_KEYS: &[&str] = &["lastPrice", "last_price", "last", "price", "lp"];
const TICKER_BID_KEYS: &[&str] = &["bidPrice", "bid_price", "bid", "bid1Price", "bp"];
const TICKER_ASK_KEYS: &[&str] = &["askPrice", "ask_price", "ask", "ask1Price", "ap"];
const TICKER_VOLUME_KEYS: &[&str] = &["volume24h", "volume_24h", "volume", "v"];
const TICKER_CHANGE_KEYS: &[&str] = &[
    "change24hPct",
    "change_24h_pct",
    "price24hPcnt",
    "price_24h_pcnt",
];

fn pick_ticker_field(base: &str, parsed: &str, value: &Value, keys: &[&str]) -> String {
    if get_str(value, keys).is_some() {
        parsed.to_string()
    } else {
        base.to_string()
    }
}

pub fn parse_ticker(value: &Value, symbol: &str) -> Ticker {
    Ticker {
        symbol: get_str(value, &["symbol", "s"]).unwrap_or_else(|| symbol.to_string()),
        last_price: get_str(value, TICKER_LAST_KEYS).unwrap_or_else(|| "0".into()),
        bid_price: get_str(value, TICKER_BID_KEYS).unwrap_or_else(|| "0".into()),
        ask_price: get_str(value, TICKER_ASK_KEYS).unwrap_or_else(|| "0".into()),
        volume_24h: get_str(value, TICKER_VOLUME_KEYS).unwrap_or_else(|| "0".into()),
        change_24h_pct: get_str(value, TICKER_CHANGE_KEYS).unwrap_or_else(|| "0".into()),
    }
}

/// Merge a WebSocket ticker delta into an existing snapshot; missing fields keep prior values.
pub fn merge_ticker(existing: Option<&Ticker>, value: &Value, symbol: &str) -> Ticker {
    let delta = parse_ticker(value, symbol);
    let Some(base) = existing else {
        return delta;
    };
    let sym = if get_str(value, &["symbol", "s"]).is_some() {
        delta.symbol
    } else {
        base.symbol.clone()
    };
    Ticker {
        symbol: sym,
        last_price: pick_ticker_field(&base.last_price, &delta.last_price, value, TICKER_LAST_KEYS),
        bid_price: pick_ticker_field(&base.bid_price, &delta.bid_price, value, TICKER_BID_KEYS),
        ask_price: pick_ticker_field(&base.ask_price, &delta.ask_price, value, TICKER_ASK_KEYS),
        volume_24h: pick_ticker_field(&base.volume_24h, &delta.volume_24h, value, TICKER_VOLUME_KEYS),
        change_24h_pct: pick_ticker_field(
            &base.change_24h_pct,
            &delta.change_24h_pct,
            value,
            TICKER_CHANGE_KEYS,
        ),
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
        symbol: get_str(data, &["symbol", "s"]).unwrap_or_else(|| symbol.to_string()),
        bids: {
            let bids = parse_levels("bids");
            if bids.is_empty() {
                parse_levels("b")
            } else {
                bids
            }
        },
        asks: {
            let asks = parse_levels("asks");
            if asks.is_empty() {
                parse_levels("a")
            } else {
                asks
            }
        },
    }
}

pub fn parse_klines(payload: &Value, symbol: &str, interval: &str) -> Vec<Kline> {
    let mut klines: Vec<Kline> = extract_list(payload)
        .iter()
        .filter_map(|item| {
            if let Some(arr) = item.as_array() {
                if arr.len() >= 6 {
                    return Some(Kline {
                        symbol: symbol.to_string(),
                        interval: interval.to_string(),
                        open_time: normalize_open_time_ms(parse_i64_value(&arr[0]).unwrap_or(0)),
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
                    open_time: normalize_open_time_ms(
                        get_str(&v, &["openTime", "open_time", "t", "timestamp", "start", "time"])
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0),
                    ),
                    open: get_str(&v, &["open", "o"]).unwrap_or_else(|| "0".into()),
                    high: get_str(&v, &["high", "h"]).unwrap_or_else(|| "0".into()),
                    low: get_str(&v, &["low", "l"]).unwrap_or_else(|| "0".into()),
                    close: get_str(&v, &["close", "c"]).unwrap_or_else(|| "0".into()),
                    volume: get_str(&v, &["volume", "v"]).unwrap_or_else(|| "0".into()),
                });
            }
            None
        })
        .collect();
    klines.retain(|k| k.open_time > 0);
    klines.sort_by_key(|k| k.open_time);
    klines
}

pub fn parse_order(value: &Value) -> Order {
    Order {
        order_id: get_str(value, &["orderId", "order_id", "id"]).unwrap_or_default(),
        symbol: get_str(value, &["symbol", "s"]).unwrap_or_default(),
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
        symbol: get_str(value, &["symbol", "s"]).unwrap_or_default(),
        side: get_str(value, &["side", "positionSide", "position_side"]).unwrap_or_default(),
        size: get_str(value, &["size", "qty", "positionAmt", "position_amt"]).unwrap_or_else(|| "0".into()),
        entry_price: get_str(value, &["entryPrice", "entry_price", "avgPrice"]).unwrap_or_else(|| "0".into()),
        leverage: get_str(value, &["leverage"]).unwrap_or_else(|| "1".into()),
        unrealised_pnl: get_str(value, &[
            "unrealisedPnl",
            "unrealised_pnl",
            "unrealizedPnl",
            "unrealized_pnl",
            "pnl",
        ])
        .unwrap_or_else(|| "0".into()),
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
    let available = get_str(value, &["available", "availableBalance", "available_balance"])
        .unwrap_or_else(|| "0".into());
    let frozen = get_str(value, &["frozen", "locked", "frozenBalance"]).unwrap_or_else(|| "0".into());
    let total = get_str(value, &["total", "balance", "equity", "walletBalance", "wallet_balance"])
        .unwrap_or_else(|| available.clone());
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

pub fn build_kline_params(
    symbol: &str,
    interval: &str,
    limit: Option<u32>,
    start: Option<i64>,
    end: Option<i64>,
) -> HashMap<String, String> {
    let mut params = HashMap::new();
    params.insert("symbol".into(), symbol.into());
    params.insert("interval".into(), interval.into());
    if let Some(l) = limit {
        params.insert("limit".into(), l.to_string());
    }
    if let Some(s) = start {
        params.insert("start".into(), s.to_string());
    }
    if let Some(e) = end {
        params.insert("end".into(), e.to_string());
    }
    params
}

pub fn build_depth_params(symbol: &str, depth: u32) -> HashMap<String, String> {
    let mut params = HashMap::new();
    params.insert("symbol".into(), symbol.into());
    params.insert("depth".into(), depth.to_string());
    params
}

pub fn build_public_trades_params(symbol: &str, limit: Option<u32>) -> HashMap<String, String> {
    let mut params = HashMap::new();
    params.insert("symbol".into(), symbol.into());
    if let Some(l) = limit {
        params.insert("limit".into(), l.to_string());
    }
    params
}

pub fn build_funding_rate_history_params(
    symbol: &str,
    from_time: Option<i64>,
    to_time: Option<i64>,
    limit: Option<u32>,
) -> HashMap<String, String> {
    let mut params = HashMap::new();
    params.insert("symbol".into(), symbol.into());
    if let Some(f) = from_time {
        params.insert("from".into(), f.to_string());
    }
    if let Some(t) = to_time {
        params.insert("to".into(), t.to_string());
    }
    if let Some(l) = limit {
        params.insert("limit".into(), l.to_string());
    }
    params
}

pub fn build_instruments_params(symbol: Option<&str>) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(s) = symbol {
        params.insert("symbol".into(), s.into());
    }
    params
}

pub fn build_fiat_rate_params(symbol_list: Option<&str>) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(s) = symbol_list {
        params.insert("symbol_list".into(), s.into());
    }
    params
}

pub fn build_order_query_params(
    symbol: Option<&str>,
    coin: Option<&str>,
    order_id: Option<&str>,
    order_link_id: Option<&str>,
    order_filter: Option<&str>,
    limit: Option<u32>,
    cursor: Option<&str>,
    start_time: Option<i64>,
    end_time: Option<i64>,
    exec_type: Option<&str>,
) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(s) = symbol {
        params.insert("symbol".into(), s.into());
    }
    if let Some(c) = coin {
        params.insert("coin".into(), c.into());
    }
    if let Some(id) = order_id {
        params.insert("order_id".into(), id.into());
    }
    if let Some(id) = order_link_id {
        params.insert("order_link_id".into(), id.into());
    }
    if let Some(f) = order_filter {
        params.insert("order_filter".into(), f.into());
    }
    if let Some(l) = limit {
        params.insert("limit".into(), l.to_string());
    }
    if let Some(c) = cursor {
        params.insert("cursor".into(), c.into());
    }
    if let Some(s) = start_time {
        params.insert("start_time".into(), s.to_string());
    }
    if let Some(e) = end_time {
        params.insert("end_time".into(), e.to_string());
    }
    if let Some(t) = exec_type {
        params.insert("exec_type".into(), t.into());
    }
    params
}

pub fn build_transfer_history_params(
    start_time: i64,
    end_time: i64,
    coin: Option<&str>,
    page_num: Option<u32>,
    page_size: Option<u32>,
) -> HashMap<String, String> {
    let mut params = HashMap::new();
    params.insert("start_time".into(), start_time.to_string());
    params.insert("end_time".into(), end_time.to_string());
    if let Some(c) = coin {
        params.insert("coin".into(), c.into());
    }
    if let Some(p) = page_num {
        params.insert("page_num".into(), p.to_string());
    }
    if let Some(p) = page_size {
        params.insert("page_size".into(), p.to_string());
    }
    params
}

pub fn build_place_order_body(req: &crate::models::trading::PlaceOrderRequest) -> Value {
    ApiOrderRequest {
        symbol: req.symbol.clone(),
        side: req.side.clone(),
        qty: req.qty.clone(),
        position_idx: req.position_idx,
        order_type: Some(req.order_type.clone()),
        price: req.price.clone(),
        time_in_force: req.time_in_force.clone(),
        order_link_id: req.order_link_id.clone(),
        reduce_only: req.reduce_only,
    }
    .to_value()
}

pub fn build_cancel_order_body(req: &crate::models::trading::CancelOrderRequest) -> Value {
    ApiCancelOrderRequest {
        symbol: req.symbol.clone(),
        order_id: req.order_id.clone(),
        order_link_id: req.order_link_id.clone(),
    }
    .to_value()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trading::PlaceOrderRequest;
    use serde_json::json;

    #[test]
    fn parse_ticker_supports_legacy_aliases() {
        let ticker = parse_ticker(
            &json!({
                "s": "ETHUSDT",
                "price": "3200",
                "bid1Price": "3199",
                "ask1Price": "3201",
                "volume": "1000",
                "price24hPcnt": "1.5"
            }),
            "BTCUSDT",
        );
        assert_eq!(ticker.symbol, "ETHUSDT");
        assert_eq!(ticker.last_price, "3200");
        assert_eq!(ticker.bid_price, "3199");
        assert_eq!(ticker.ask_price, "3201");
        assert_eq!(ticker.volume_24h, "1000");
        assert_eq!(ticker.change_24h_pct, "1.5");
    }

    #[test]
    fn merge_ticker_preserves_snapshot_when_ws_delta_is_symbol_only() {
        let base = Ticker {
            symbol: "BTCUSDT".into(),
            last_price: "61700".into(),
            bid_price: "61699".into(),
            ask_price: "61701".into(),
            volume_24h: "1234".into(),
            change_24h_pct: "0.5".into(),
        };
        let merged = merge_ticker(Some(&base), &json!({"s": "BTCUSDT"}), "BTCUSDT");
        assert_eq!(merged.last_price, "61700");
        assert_eq!(merged.bid_price, "61699");
        assert_eq!(merged.ask_price, "61701");
        assert_eq!(merged.volume_24h, "1234");
        assert_eq!(merged.change_24h_pct, "0.5");
    }

    #[test]
    fn merge_ticker_ignores_absolute_change_field() {
        let base = Ticker {
            symbol: "BTCUSDT".into(),
            last_price: "61700".into(),
            bid_price: "61699".into(),
            ask_price: "61701".into(),
            volume_24h: "1234".into(),
            change_24h_pct: "0.034".into(),
        };
        let merged = merge_ticker(
            Some(&base),
            &json!({"change": "100", "price24hPcnt": "0.034"}),
            "BTCUSDT",
        );
        assert_eq!(merged.change_24h_pct, "0.034");
    }

    #[test]
    fn merge_ticker_ws_change_field_does_not_override_pct() {
        let base = Ticker {
            symbol: "BTCUSDT".into(),
            last_price: "61700".into(),
            bid_price: "61699".into(),
            ask_price: "61701".into(),
            volume_24h: "1234".into(),
            change_24h_pct: "1.5".into(),
        };
        let merged = merge_ticker(Some(&base), &json!({"change": "150.5"}), "BTCUSDT");
        assert_eq!(merged.change_24h_pct, "1.5");
    }

    #[test]
    fn merge_ticker_applies_ws_price_delta() {
        let base = Ticker {
            symbol: "BTCUSDT".into(),
            last_price: "61700".into(),
            bid_price: "61699".into(),
            ask_price: "61701".into(),
            volume_24h: "1234".into(),
            change_24h_pct: "0.5".into(),
        };
        let merged = merge_ticker(Some(&base), &json!({"lp": "61800"}), "BTCUSDT");
        assert_eq!(merged.last_price, "61800");
        assert_eq!(merged.bid_price, "61699");
    }

    #[test]
    fn parse_depth_supports_b_a_aliases() {
        let depth = parse_depth(
            &json!({
                "s": "BTCUSDT",
                "b": [["61727.0", "8.91"]],
                "a": [["61728.5", "8.79"]]
            }),
            "BTCUSDT",
        );
        assert_eq!(depth.symbol, "BTCUSDT");
        assert_eq!(depth.bids.len(), 1);
        assert_eq!(depth.asks.len(), 1);
        assert_eq!(depth.bids[0].price, "61727.0");
        assert_eq!(depth.asks[0].price, "61728.5");
    }

    #[test]
    fn parse_klines_parses_string_timestamps_in_arrays() {
        let klines = parse_klines(
            &json!([["1783006260", "61679.3", "61700", "61650", "61690", "12"]]),
            "BTCUSDT",
            "1",
        );
        assert_eq!(klines.len(), 1);
        assert_eq!(klines[0].open_time, 1_783_006_260_000);
    }

    #[test]
    fn parse_klines_normalizes_second_timestamps() {
        let klines = parse_klines(
            &json!([[1700000000, "1", "2", "0.5", "1.5", "10"]]),
            "BTCUSDT",
            "1",
        );
        assert_eq!(klines.len(), 1);
        assert_eq!(klines[0].open_time, 1_700_000_000_000);
    }

    #[test]
    fn parse_klines_keeps_millisecond_timestamps() {
        let klines = parse_klines(
            &json!([[1700000000000_i64, "1", "2", "0.5", "1.5", "10"]]),
            "BTCUSDT",
            "1",
        );
        assert_eq!(klines.len(), 1);
        assert_eq!(klines[0].open_time, 1_700_000_000_000);
    }

    #[test]
    fn parse_orders_from_list_envelope() {
        let orders = parse_orders(&json!({
            "data": {
                "list": [{
                    "orderId": "abc123",
                    "symbol": "ETHUSDT",
                    "side": "Buy",
                    "orderType": "Limit",
                    "price": "3200",
                    "qty": "0.1",
                    "status": "New"
                }]
            }
        }));
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].order_id, "abc123");
        assert_eq!(orders[0].symbol, "ETHUSDT");
    }

    #[test]
    fn parse_positions_filters_zero_size() {
        let positions = parse_positions(&json!({
            "data": {
                "list": [
                    {"symbol": "BTCUSDT", "size": "0.01", "side": "Buy"},
                    {"symbol": "ETHUSDT", "size": "0", "side": "Buy"}
                ]
            }
        }));
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].symbol, "BTCUSDT");
    }

    #[test]
    fn depth_params_use_depth_key() {
        let params = build_depth_params("BTCUSDT", 20);
        assert_eq!(params.get("depth"), Some(&"20".to_string()));
        assert!(params.get("limit").is_none());
    }

    #[test]
    fn place_order_body_snake_case_with_position_idx() {
        let req = PlaceOrderRequest {
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Limit".into(),
            qty: "0.001".into(),
            position_idx: 1,
            price: Some("50000".into()),
            time_in_force: None,
            order_link_id: None,
            reduce_only: None,
        };
        let body = build_place_order_body(&req);
        assert_eq!(body["symbol"], "BTCUSDT");
        assert_eq!(body["position_idx"], 1);
        assert_eq!(body["order_type"], "Limit");
        assert!(body.get("orderType").is_none());
    }
}
