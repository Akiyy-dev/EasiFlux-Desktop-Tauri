use std::collections::HashMap;
use std::sync::Arc;

use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::api::PublicApi;
use crate::auth::time_sync::sync_from_server;
use crate::auth::{Signer, TimeSync};
use crate::error::{AppError, AppResult};
use crate::models::config::{ApiCredential, DEFAULT_BASE_URL, RECV_WINDOW_MS};

use super::response::{error_message, is_success_response, is_timestamp_error};

pub fn normalize_base_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        DEFAULT_BASE_URL.to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Clone)]
pub struct ApiClient {
    http: Client,
    base_url: Arc<RwLock<String>>,
    credential: Arc<RwLock<Option<ApiCredential>>>,
    signer: Arc<RwLock<Option<Signer>>>,
    time_sync: Arc<TimeSync>,
}

impl ApiClient {
    pub fn new() -> Self {
        Self {
            http: Client::builder()
                .user_agent("EasiFlux-Desktop/0.3.0")
                .build()
                .expect("http client"),
            base_url: Arc::new(RwLock::new(DEFAULT_BASE_URL.to_string())),
            credential: Arc::new(RwLock::new(None)),
            signer: Arc::new(RwLock::new(None)),
            time_sync: Arc::new(TimeSync::new()),
        }
    }

    pub fn time_sync(&self) -> Arc<TimeSync> {
        self.time_sync.clone()
    }

    pub async fn set_credential(&self, credential: ApiCredential) {
        let base = normalize_base_url(&credential.base_url);
        *self.base_url.write().await = base;
        *self.signer.write().await = Some(Signer::new(
            credential.api_key.clone(),
            credential.api_secret.clone(),
        ));
        *self.credential.write().await = Some(credential);
    }

    pub async fn clear_credential(&self) {
        *self.credential.write().await = None;
        *self.signer.write().await = None;
    }

    pub async fn has_credential(&self) -> bool {
        self.credential.read().await.is_some()
    }

    pub async fn base_url(&self) -> String {
        self.base_url.read().await.clone()
    }

    pub async fn public_get(&self, path: &str, params: HashMap<String, String>) -> AppResult<Value> {
        let url = format!("{}{}", self.base_url().await, path);
        let response = self.http.get(&url).query(&params).send().await?;
        self.parse_response(response).await
    }

    pub async fn private_get(
        &self,
        path: &str,
        params: HashMap<String, String>,
    ) -> AppResult<Value> {
        self.ensure_time_sync().await?;
        match self.private_get_once(path, params.clone()).await {
            Ok(v) => Ok(v),
            Err(e) => {
                if let AppError::Trading(ref msg) = e {
                    if msg.contains("timestamp") || msg.contains("recv_window") {
                        self.force_time_sync().await?;
                        return self.private_get_once(path, params).await;
                    }
                }
                if let AppError::Connection(ref msg) = e {
                    if msg.contains("timestamp") || msg.contains("recv_window") {
                        self.force_time_sync().await?;
                        return self.private_get_once(path, params).await;
                    }
                }
                Err(e)
            }
        }
    }

    async fn private_get_once(
        &self,
        path: &str,
        params: HashMap<String, String>,
    ) -> AppResult<Value> {
        let url = format!("{}{}", self.base_url().await, path);
        let query = encode_query(&params);
        let headers = self.sign_headers(&query, "").await?;
        let mut req = self.http.get(&url).query(&params);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let response = req.send().await?;
        self.parse_response(response).await
    }

    pub async fn private_post(&self, path: &str, body: Value) -> AppResult<Value> {
        self.ensure_time_sync().await?;
        match self.private_post_once(path, body.clone()).await {
            Ok(v) => Ok(v),
            Err(e) => {
                if let AppError::Trading(ref msg) = e {
                    if msg.contains("timestamp") || msg.contains("recv_window") {
                        self.force_time_sync().await?;
                        return self.private_post_once(path, body).await;
                    }
                }
                Err(e)
            }
        }
    }

    async fn private_post_once(&self, path: &str, body: Value) -> AppResult<Value> {
        let url = format!("{}{}", self.base_url().await, path);
        let body_text =
            serde_json::to_string(&body).map_err(|e| AppError::Internal(e.to_string()))?;
        let headers = self.sign_headers("", &body_text).await?;
        let mut req = self
            .http
            .post(&url)
            .header("Content-Type", "application/json");
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let response = req.body(body_text).send().await?;
        self.parse_response(response).await
    }

    async fn ensure_time_sync(&self) -> AppResult<()> {
        if self.time_sync.offset_ms() == 0 {
            self.force_time_sync().await?;
        }
        Ok(())
    }

    async fn force_time_sync(&self) -> AppResult<()> {
        sync_from_server(
            self.time_sync.as_ref(),
            PublicApi::server_time(self),
        )
        .await
    }

    async fn sign_headers(&self, query: &str, body: &str) -> AppResult<Vec<(String, String)>> {
        let signer = self
            .signer
            .read()
            .await
            .clone()
            .ok_or(AppError::Auth("未配置 API 凭据".into()))?;
        let payload = if body.is_empty() { query } else { body };
        Ok(signer.prepare_headers(
            self.time_sync.timestamp_ms(),
            RECV_WINDOW_MS,
            payload,
        ))
    }

    async fn parse_response(&self, response: reqwest::Response) -> AppResult<Value> {
        let status = response.status();
        let text = response.text().await?;
        if text.is_empty() {
            if status.is_success() {
                return Ok(json!({}));
            }
            return Err(AppError::Connection(format!("HTTP {}", status)));
        }
        let payload: Value =
            serde_json::from_str(&text).map_err(|e| AppError::Connection(e.to_string()))?;
        if !status.is_success() {
            return Err(AppError::Connection(
                error_message(&payload).unwrap_or_else(|| format!("HTTP {}", status)),
            ));
        }
        if !is_success_response(&payload) {
            let msg = error_message(&payload).unwrap_or_else(|| "API 返回错误".into());
            if is_timestamp_error(&payload) {
                return Err(AppError::Trading(format!("timestamp: {}", msg)));
            }
            return Err(AppError::Trading(msg));
        }
        Ok(payload)
    }
}

impl Default for ApiClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Encode query without sorting keys (SDK default: sort_query_for_signature=False).
pub fn encode_query(params: &HashMap<String, String>) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_query_preserves_insertion_order() {
        let mut params = HashMap::new();
        params.insert("symbol".into(), "BTCUSDT".into());
        params.insert("limit".into(), "10".into());
        let encoded = encode_query(&params);
        assert!(encoded.contains("symbol=BTCUSDT"));
        assert!(encoded.contains("limit=10"));
    }
}
