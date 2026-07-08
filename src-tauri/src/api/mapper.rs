use std::collections::HashMap;

use rust_decimal::Decimal;
use serde_json::Value;
use std::str::FromStr;

use crate::models::api_requests::{ApiCancelOrderRequest, ApiOrderRequest};
use crate::models::account::Balance;
use crate::models::market::{Depth, DepthLevel, Kline, Ticker};
use crate::models::trading::{Order, OrderStatus, Position};

use super::response::{extract_data, extract_list, extract_list_with_meta, get_str, ListEnvelopeMeta};

pub const DEFAULT_SETTLE_COIN: &str = "USDT";

pub fn list_envelope_meta(payload: &Value) -> ListEnvelopeMeta {
    extract_list_with_meta(payload).1
}

fn parse_decimal_value(raw: &str) -> Decimal {
    Decimal::from_str(raw.trim()).unwrap_or(Decimal::ZERO)
}

fn is_zero_size(raw: &str) -> bool {
    raw.trim().is_empty() || parse_decimal_value(raw) == Decimal::ZERO
}

fn parse_i32_value(value: &Value) -> i32 {
    value
        .as_i64()
        .or_else(|| value.as_u64().map(|n| n as i64))
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0) as i32
}

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

const TICKER_LAST_KEYS: &[&str] = &[
    "lastPrice", "last_price", "last", "price", "lp", "p",
];
const TICKER_BID_KEYS: &[&str] = &[
    "bidPrice", "bid_price", "bid", "bid1Price", "bp", "b1",
];
const TICKER_ASK_KEYS: &[&str] = &[
    "askPrice", "ask_price", "ask", "ask1Price", "ap", "a1",
];
const TICKER_VOLUME_KEYS: &[&str] = &["volume24h", "volume_24h", "volume", "v"];
const TICKER_MARK_KEYS: &[&str] = &["markPrice", "mark_price", "mark", "mp"];
const TICKER_HIGH_24H_KEYS: &[&str] = &["high24h", "high_24h", "highPrice24h", "high_price_24h", "h"];
const TICKER_LOW_24H_KEYS: &[&str] = &["low24h", "low_24h", "lowPrice24h", "low_price_24h", "l"];
const TICKER_FUNDING_RATE_KEYS: &[&str] = &["fundingRate", "funding_rate", "fr"];
const TICKER_NEXT_FUNDING_KEYS: &[&str] = &[
    "nextFundingTime",
    "next_funding_time",
    "fundingTime",
    "funding_time",
    "nextFunding",
];
const TICKER_PREV_KEYS: &[&str] = &["prev_price_24h", "prevPrice24h", "p24"];
const TICKER_CHANGE_KEYS: &[&str] = &[
    "change24hPct",
    "change_24h_pct",
    "price24hPcnt",
    "price_24h_pcnt",
    "pP",
    "pp",
];

fn normalize_change_24h_pcnt(raw: &str) -> String {
    let value: f64 = raw.parse().unwrap_or(0.0);
    let pct = value / 10_000.0;
    format!("{pct:.6}")
}

fn compute_change_from_prices(last: &str, prev: &str) -> Option<String> {
    let last: f64 = last.parse().ok()?;
    let prev: f64 = prev.parse().ok()?;
    if prev == 0.0 {
        return None;
    }
    let pct = (last - prev) / prev * 100.0;
    Some(format!("{pct:.6}"))
}

fn resolve_change_24h_pct(value: &Value, last_price: &str) -> String {
    if let Some(raw) = get_str(value, TICKER_CHANGE_KEYS) {
        return normalize_change_24h_pcnt(&raw);
    }
    if let Some(prev) = get_str(value, TICKER_PREV_KEYS) {
        if let Some(pct) = compute_change_from_prices(last_price, &prev) {
            return pct;
        }
    }
    "0".into()
}

fn pick_ticker_field(base: &str, parsed: &str, value: &Value, keys: &[&str]) -> String {
    if get_str(value, keys).is_some() {
        parsed.to_string()
    } else {
        base.to_string()
    }
}

pub fn parse_ticker(value: &Value, symbol: &str) -> Ticker {
    let last_price = get_str(value, TICKER_LAST_KEYS).unwrap_or_else(|| "0".into());
    Ticker {
        symbol: get_str(value, &["symbol", "s"]).unwrap_or_else(|| symbol.to_string()),
        last_price: last_price.clone(),
        bid_price: get_str(value, TICKER_BID_KEYS).unwrap_or_else(|| "0".into()),
        ask_price: get_str(value, TICKER_ASK_KEYS).unwrap_or_else(|| "0".into()),
        volume_24h: get_str(value, TICKER_VOLUME_KEYS).unwrap_or_else(|| "0".into()),
        change_24h_pct: resolve_change_24h_pct(value, &last_price),
        mark_price: get_str(value, TICKER_MARK_KEYS).unwrap_or_default(),
        high_24h: get_str(value, TICKER_HIGH_24H_KEYS).unwrap_or_default(),
        low_24h: get_str(value, TICKER_LOW_24H_KEYS).unwrap_or_default(),
        funding_rate: get_str(value, TICKER_FUNDING_RATE_KEYS).unwrap_or_default(),
        next_funding_time: TICKER_NEXT_FUNDING_KEYS
            .iter()
            .find_map(|key| value.get(*key).and_then(parse_i64_value))
            .or_else(|| get_str(value, TICKER_NEXT_FUNDING_KEYS).and_then(|s| s.parse::<i64>().ok()))
            .map(normalize_open_time_ms),
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
        change_24h_pct: {
            let parsed = if get_str(value, TICKER_CHANGE_KEYS).is_some() {
                delta.change_24h_pct.clone()
            } else if get_str(value, TICKER_PREV_KEYS).is_some() {
                resolve_change_24h_pct(value, &pick_ticker_field(
                    &base.last_price,
                    &delta.last_price,
                    value,
                    TICKER_LAST_KEYS,
                ))
            } else {
                base.change_24h_pct.clone()
            };
            pick_ticker_field(&base.change_24h_pct, &parsed, value, TICKER_CHANGE_KEYS)
        },
        mark_price: pick_ticker_field(&base.mark_price, &delta.mark_price, value, TICKER_MARK_KEYS),
        high_24h: pick_ticker_field(&base.high_24h, &delta.high_24h, value, TICKER_HIGH_24H_KEYS),
        low_24h: pick_ticker_field(&base.low_24h, &delta.low_24h, value, TICKER_LOW_24H_KEYS),
        funding_rate: pick_ticker_field(
            &base.funding_rate,
            &delta.funding_rate,
            value,
            TICKER_FUNDING_RATE_KEYS,
        ),
        next_funding_time: if get_str(value, TICKER_NEXT_FUNDING_KEYS).is_some() {
            delta.next_funding_time
        } else {
            base.next_funding_time
        },
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
    let mut by_time = std::collections::BTreeMap::new();
    for kline in klines {
        by_time.insert(kline.open_time, kline);
    }
    by_time.into_values().collect()
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
        filled_qty: get_str(value, &["cumExecQty", "cum_exec_qty", "filled_qty", "filledQty"])
            .unwrap_or_else(|| "0".into()),
        avg_price: get_str(value, &["avgPrice", "avg_price"]).unwrap_or_else(|| "0".into()),
    }
}

pub fn parse_orders(payload: &Value) -> Vec<Order> {
    extract_list(payload).iter().map(|v| parse_order(v)).collect()
}

pub fn parse_position(value: &Value) -> Position {
    let position_idx = value
        .get("positionIdx")
        .or_else(|| value.get("position_idx"))
        .map(parse_i32_value)
        .unwrap_or(0);
    Position {
        symbol: get_str(value, &["symbol", "s"]).unwrap_or_default(),
        side: get_str(value, &["side", "positionSide", "position_side", "direction"]).unwrap_or_default(),
        size: get_str(value, &[
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
        ])
        .unwrap_or_else(|| "0".into()),
        entry_price: get_str(value, &[
            "entryPrice",
            "entry_price",
            "avgPrice",
            "avg_price",
            "openPrice",
            "open_price",
        ])
        .unwrap_or_else(|| "0".into()),
        leverage: get_str(value, &["leverage"]).unwrap_or_else(|| "1".into()),
        unrealised_pnl: get_str(value, &[
            "unrealisedPnl",
            "unrealised_pnl",
            "unrealizedPnl",
            "unrealized_pnl",
            "profitUnreal",
            "profit_unreal",
            "pnl",
        ])
        .unwrap_or_else(|| "0".into()),
        position_idx,
    }
}

pub fn parse_positions(payload: &Value) -> Vec<Position> {
    extract_list(payload)
        .iter()
        .map(|v| parse_position(v))
        .filter(|p| !is_zero_size(&p.size))
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
        params.insert("start".into(), normalize_kline_query_time(s).to_string());
    }
    if let Some(e) = end {
        params.insert("end".into(), normalize_kline_query_time(e).to_string());
    }
    params
}

/// EasiCoin kline `start`/`end` query params use **seconds** (ms values are rejected).
fn normalize_kline_query_time(value: i64) -> i64 {
    if value >= 10_000_000_000 {
        value / 1000
    } else {
        value
    }
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
) -> Vec<(String, String)> {
    let mut params = Vec::new();
    if let Some(s) = symbol {
        if !s.is_empty() {
            params.push(("symbol".into(), s.into()));
        }
    }
    if let Some(c) = coin {
        if !c.is_empty() {
            params.push(("coin".into(), c.into()));
        }
    }
    if let Some(id) = order_id {
        params.push(("order_id".into(), id.into()));
    }
    if let Some(id) = order_link_id {
        params.push(("order_link_id".into(), id.into()));
    }
    if let Some(f) = order_filter {
        params.push(("order_filter".into(), f.into()));
    }
    if let Some(l) = limit {
        params.push(("limit".into(), l.to_string()));
    }
    if let Some(c) = cursor {
        params.push(("cursor".into(), c.into()));
    }
    if let Some(s) = start_time {
        params.push(("start_time".into(), s.to_string()));
    }
    if let Some(e) = end_time {
        params.push(("end_time".into(), e.to_string()));
    }
    if let Some(t) = exec_type {
        params.push(("exec_type".into(), t.into()));
    }
    let has_symbol = params.iter().any(|(k, _)| k == "symbol");
    let has_coin = params.iter().any(|(k, _)| k == "coin");
    if !has_symbol && !has_coin {
        params.push(("coin".into(), DEFAULT_SETTLE_COIN.into()));
    }
    params
}

pub fn build_transfer_history_params(
    start_time: i64,
    end_time: i64,
    coin: Option<&str>,
    page_num: Option<u32>,
    page_size: Option<u32>,
) -> Vec<(String, String)> {
    let mut params = Vec::new();
    params.push(("start_time".into(), start_time.to_string()));
    params.push(("end_time".into(), end_time.to_string()));
    if let Some(c) = coin {
        params.push(("coin".into(), c.into()));
    }
    if let Some(p) = page_num {
        params.push(("page_num".into(), p.to_string()));
    }
    if let Some(p) = page_size {
        params.push(("page_size".into(), p.to_string()));
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
    fn parse_ticker_normalizes_price_24h_pcnt_fixed_point() {
        let ticker = parse_ticker(
            &json!({
                "symbol": "BTCUSDT",
                "last_price": "62816.9",
                "price_24h_pcnt": "1829"
            }),
            "BTCUSDT",
        );
        assert_eq!(ticker.change_24h_pct, "0.182900");
    }

    #[test]
    fn parse_ticker_supports_legacy_aliases() {
        let ticker = parse_ticker(
            &json!({
                "s": "ETHUSDT",
                "price": "3200",
                "bid1Price": "3199",
                "ask1Price": "3201",
                "volume": "1000",
                "price24hPcnt": "3950"
            }),
            "BTCUSDT",
        );
        assert_eq!(ticker.symbol, "ETHUSDT");
        assert_eq!(ticker.last_price, "3200");
        assert_eq!(ticker.bid_price, "3199");
        assert_eq!(ticker.ask_price, "3201");
        assert_eq!(ticker.volume_24h, "1000");
        assert_eq!(ticker.change_24h_pct, "0.395000");
    }

    #[test]
    fn merge_ticker_applies_ws_pp_delta() {
        let base = Ticker {
            symbol: "BTCUSDT".into(),
            last_price: "61700".into(),
            bid_price: "61699".into(),
            ask_price: "61701".into(),
            volume_24h: "1234".into(),
            change_24h_pct: "0.100000".into(),
            ..Ticker::default()
        };
        let merged = merge_ticker(Some(&base), &json!({"pP": "1963", "p": "61800"}), "BTCUSDT");
        assert_eq!(merged.last_price, "61800");
        assert_eq!(merged.change_24h_pct, "0.196300");
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
            ..Ticker::default()
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
            ..Ticker::default()
        };
        let merged = merge_ticker(
            Some(&base),
            &json!({"change": "100", "price24hPcnt": "340"}),
            "BTCUSDT",
        );
        assert_eq!(merged.change_24h_pct, "0.034000");
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
            ..Ticker::default()
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
            ..Ticker::default()
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
    fn parse_positions_filters_decimal_zero_size() {
        let positions = parse_positions(&json!({
            "data": {
                "list": [
                    {"symbol": "BTCUSDT", "size": "0.01", "side": "Buy"},
                    {"symbol": "ETHUSDT", "size": "0.000", "side": "Buy"},
                    {"symbol": "SOLUSDT", "size": "0", "side": "Buy"}
                ]
            }
        }));
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].symbol, "BTCUSDT");
    }

    #[test]
    fn order_query_params_default_coin_when_scope_missing() {
        let params = build_order_query_params(None, None, None, None, None, None, None, None, None, None);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], ("coin".into(), "USDT".into()));
    }

    #[test]
    fn order_query_params_keep_symbol_without_coin() {
        let params = build_order_query_params(Some("BTCUSDT"), None, None, None, None, None, None, None, None, None);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].0, "symbol");
    }

    #[test]
    fn order_query_params_ignore_empty_symbol() {
        let params = build_order_query_params(Some(""), None, None, None, None, None, None, None, None, None);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0], ("coin".into(), "USDT".into()));
    }

    #[test]
    fn build_kline_params_normalize_millisecond_times_to_seconds() {
        let params = build_kline_params("BTCUSDT", "1", Some(10), Some(1_700_000_000_000), Some(1_700_000_060_000));
        assert_eq!(params.get("start"), Some(&"1700000000".to_string()));
        assert_eq!(params.get("end"), Some(&"1700000060".to_string()));
    }

    #[test]
    fn parse_positions_supports_position_list_envelope() {
        let positions = parse_positions(&json!({
            "data": {
                "positionList": [{
                    "symbol": "BTCUSDT",
                    "positionAmt": "0.5",
                    "positionSide": "Buy",
                    "entryPrice": "60000",
                    "positionIdx": 1,
                    "unrealisedPnl": "12.5"
                }]
            }
        }));
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].size, "0.5");
        assert_eq!(positions[0].position_idx, 1);
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
