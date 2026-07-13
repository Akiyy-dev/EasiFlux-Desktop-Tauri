use std::collections::HashMap;

use serde_json::Value;

use crate::auth::time_sync::parse_server_time;
use crate::error::AppResult;
use crate::models::config::{DEFAULT_DEPTH_LIMIT, DEFAULT_KLINE_LIMIT};
use crate::models::market::{Depth, Kline, Ticker};

use super::client::ApiClient;
use super::endpoints;
use super::mapper::{
    build_depth_params, build_fiat_rate_params, build_funding_rate_history_params,
    build_instruments_params, build_kline_params, build_public_trades_params, parse_depth,
    parse_klines, parse_ticker,
};
use super::response::extract_list;

pub struct PublicApi;

impl PublicApi {
    pub async fn server_time(client: &ApiClient) -> AppResult<u64> {
        let payload = client
            .public_get(endpoints::SERVER_TIME, HashMap::new())
            .await?;
        parse_server_time(&payload)
    }

    pub async fn ticker(client: &ApiClient, symbol: &str) -> AppResult<Ticker> {
        let mut params = HashMap::new();
        params.insert("symbol".into(), symbol.into());
        let payload = client.public_get(endpoints::TICKER, params).await?;
        let items = extract_list(&payload);
        if let Some(item) = items.iter().find(|item| {
            super::response::get_str(item, &["symbol", "s"])
                .is_some_and(|value| value.eq_ignore_ascii_case(symbol))
        }) {
            return Ok(parse_ticker(item, symbol));
        }
        if let Some(item) = items.first() {
            return Ok(parse_ticker(item, symbol));
        }
        Ok(parse_ticker(super::response::extract_data(&payload), symbol))
    }

    pub async fn klines(
        client: &ApiClient,
        symbol: &str,
        interval: &str,
        limit: u32,
        start: Option<i64>,
        end: Option<i64>,
    ) -> AppResult<Vec<Kline>> {
        let params = build_kline_params(symbol, interval, Some(limit), start, end);
        let payload = client.public_get(endpoints::KLINE, params).await?;
        Ok(parse_klines(&payload, symbol, interval))
    }

    pub async fn depth(client: &ApiClient, symbol: &str, depth: u32) -> AppResult<Depth> {
        let params = build_depth_params(symbol, depth);
        let payload = client.public_get(endpoints::DEPTH, params).await?;
        Ok(parse_depth(&payload, symbol))
    }

    pub async fn public_trades(
        client: &ApiClient,
        symbol: &str,
        limit: Option<u32>,
    ) -> AppResult<Value> {
        let params = build_public_trades_params(symbol, limit);
        client.public_get(endpoints::PUBLIC_TRADES, params).await
    }

    pub async fn funding_rate_history(
        client: &ApiClient,
        symbol: &str,
        from_time: Option<i64>,
        to_time: Option<i64>,
        limit: Option<u32>,
    ) -> AppResult<Value> {
        let params = build_funding_rate_history_params(symbol, from_time, to_time, limit);
        client
            .public_get(endpoints::FUNDING_RATE_HISTORY, params)
            .await
    }

    pub async fn mark_price_kline(
        client: &ApiClient,
        symbol: &str,
        interval: &str,
        limit: Option<u32>,
        start: Option<i64>,
        end: Option<i64>,
    ) -> AppResult<Value> {
        let params = build_kline_params(symbol, interval, limit, start, end);
        client
            .public_get(endpoints::MARK_PRICE_KLINE, params)
            .await
    }

    pub async fn instruments(client: &ApiClient, symbol: Option<&str>) -> AppResult<Value> {
        let params = build_instruments_params(symbol);
        client.public_get(endpoints::INSTRUMENTS, params).await
    }

    pub async fn risk_limit(client: &ApiClient, symbol: &str) -> AppResult<Value> {
        let mut params = HashMap::new();
        params.insert("symbol".into(), symbol.into());
        client.public_get(endpoints::RISK_LIMIT, params).await
    }

    pub async fn market_close_time(client: &ApiClient) -> AppResult<Value> {
        client
            .public_get(endpoints::MARKET_CLOSE_TIME, HashMap::new())
            .await
    }

    pub async fn fiat_rate(client: &ApiClient, symbol_list: Option<&str>) -> AppResult<Value> {
        let params = build_fiat_rate_params(symbol_list);
        client.public_get(endpoints::FIAT_RATE, params).await
    }

    pub async fn market_snapshot(
        client: &ApiClient,
        symbol: &str,
        interval: &str,
    ) -> AppResult<(Ticker, Depth, Vec<Kline>)> {
        let ticker = Self::ticker(client, symbol).await?;
        let depth = Self::depth(client, symbol, DEFAULT_DEPTH_LIMIT).await?;
        let klines = Self::klines(client, symbol, interval, DEFAULT_KLINE_LIMIT, None, None).await?;
        Ok((ticker, depth, klines))
    }
}
