mod error;
mod validation;

use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::header::RETRY_AFTER;
use reqwest::{redirect, Client, StatusCode};
use url::Url;

pub(crate) use crate::models::news::{ValidatedNewsMessage, ValidatedNewsPage};
use crate::storage::NewsApiToken;
use error::{classify_status, parse_retry_after};
pub use error::{NewsFetchError, NewsFetchErrorKind};
use validation::decode_page;

const NEWS_MESSAGES_PATH: &str = "api/public/v1/messages";
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[async_trait]
pub trait NewsPageFetcher: Send + Sync {
    async fn fetch_after(
        &self,
        token: &NewsApiToken,
        cursor: i64,
        limit: usize,
    ) -> Result<ValidatedNewsPage, NewsFetchError>;
}

#[derive(Clone)]
pub struct TgForwarderNewsClient {
    http: Client,
    base_url: Url,
}

impl TgForwarderNewsClient {
    pub fn new(base_url: Url) -> Result<Self, NewsFetchError> {
        Self::with_timeout_policy(base_url, Duration::from_secs(5), Duration::from_secs(15))
    }

    fn with_timeout_policy(
        base_url: Url,
        connect_timeout: Duration,
        request_timeout: Duration,
    ) -> Result<Self, NewsFetchError> {
        let http = Client::builder()
            .connect_timeout(connect_timeout)
            .timeout(request_timeout)
            .redirect(redirect::Policy::none())
            .build()
            .map_err(|_| NewsFetchError::new(NewsFetchErrorKind::Contract))?;
        Ok(Self { http, base_url })
    }
}

#[async_trait]
impl NewsPageFetcher for TgForwarderNewsClient {
    async fn fetch_after(
        &self,
        token: &NewsApiToken,
        cursor: i64,
        limit: usize,
    ) -> Result<ValidatedNewsPage, NewsFetchError> {
        if cursor < 0 || !(1..=100).contains(&limit) {
            return Err(NewsFetchError::new(NewsFetchErrorKind::Contract));
        }

        let url = self
            .base_url
            .join(NEWS_MESSAGES_PATH)
            .map_err(|_| NewsFetchError::new(NewsFetchErrorKind::Contract))?;
        let cursor_query = cursor.to_string();
        let limit_query = limit.to_string();
        let response = self
            .http
            .get(url)
            .query(&[("cursor", cursor_query), ("limit", limit_query)])
            .bearer_auth(token.as_str())
            .send()
            .await
            .map_err(|_| NewsFetchError::new(NewsFetchErrorKind::Transient))?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = if matches!(
                status,
                StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
            ) {
                response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(parse_retry_after)
            } else {
                None
            };
            return Err(NewsFetchError::with_retry_after(
                classify_status(status),
                retry_after,
            ));
        }

        let mut body = Vec::new();
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|_| NewsFetchError::new(NewsFetchErrorKind::Transient))?;
            let next_len = body
                .len()
                .checked_add(chunk.len())
                .ok_or_else(|| NewsFetchError::new(NewsFetchErrorKind::Contract))?;
            if next_len > MAX_RESPONSE_BYTES {
                return Err(NewsFetchError::new(NewsFetchErrorKind::Contract));
            }
            body.extend_from_slice(&chunk);
        }

        decode_page(&body, cursor, limit)
    }
}

#[cfg(test)]
#[path = "news_client_tests.rs"]
mod tests;
