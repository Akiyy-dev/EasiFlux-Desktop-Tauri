use std::collections::HashMap;

use crate::auth::time_sync::parse_server_time;
use crate::error::AppResult;
use crate::models::config::{DEFAULT_DEPTH_LIMIT, DEFAULT_KLINE_LIMIT};
use crate::models::market::{Depth, Kline, Ticker};

use super::client::ApiClient;
use super::endpoints;
use super::mapper::{
    build_depth_params, build_kline_params, parse_depth, parse_klines, parse_ticker,
};
use super::response::extract_data;

pub struct PublicApi;

impl PublicApi {
    pub async fn server_time(client: &ApiClient) -> AppResult<u64> {
        let payload = client
            .public_get(endpoints::SERVER_TIME, HashMap::new())
            .await?;
        parse_server_time(extract_data(&payload))
    }

    pub async fn ticker(client: &ApiClient, symbol: &str) -> AppResult<Ticker> {
        let mut params = HashMap::new();
        params.insert("symbol".into(), symbol.into());
        let payload = client.public_get(endpoints::TICKER, params).await?;
        let items = super::response::extract_list(&payload);
        if let Some(first) = items.first() {
            return Ok(parse_ticker(first, symbol));
        }
        Ok(parse_ticker(extract_data(&payload), symbol))
    }

    pub async fn klines(
        client: &ApiClient,
        symbol: &str,
        interval: &str,
        limit: u32,
    ) -> AppResult<Vec<Kline>> {
        let params = build_kline_params(symbol, interval, limit);
        let payload = client.public_get(endpoints::KLINE, params).await?;
        Ok(parse_klines(&payload, symbol, interval))
    }

    pub async fn depth(client: &ApiClient, symbol: &str, limit: u32) -> AppResult<Depth> {
        let params = build_depth_params(symbol, limit);
        let payload = client.public_get(endpoints::DEPTH, params).await?;
        Ok(parse_depth(&payload, symbol))
    }

    pub async fn market_snapshot(
        client: &ApiClient,
        symbol: &str,
        interval: &str,
    ) -> AppResult<(Ticker, Depth, Vec<Kline>)> {
        let ticker = Self::ticker(client, symbol).await?;
        let depth = Self::depth(client, symbol, DEFAULT_DEPTH_LIMIT).await?;
        let klines = Self::klines(client, symbol, interval, DEFAULT_KLINE_LIMIT).await?;
        Ok((ticker, depth, klines))
    }
}
