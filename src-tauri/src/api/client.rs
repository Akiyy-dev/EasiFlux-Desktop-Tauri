use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::api::PublicApi;
use crate::auth::time_sync::sync_from_server;
use crate::auth::{Signer, TimeSync};
use crate::error::{AppError, AppResult};
use crate::models::config::{ApiCredential, DEFAULT_BASE_URL, RECV_WINDOW_MS};

use super::endpoints;
use super::response::{error_message, is_sign_error, is_success_response, is_timestamp_error};

/// Ordered query pairs for private GET signing (SDK insertion order).
pub type QueryParams = Vec<(String, String)>;

#[derive(Debug, Clone, Copy)]
struct HttpTimeoutPolicy {
    connect: Duration,
    request: Duration,
}

impl HttpTimeoutPolicy {
    const fn production() -> Self {
        Self {
            connect: Duration::from_secs(5),
            request: Duration::from_secs(15),
        }
    }
}

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
        Self::with_timeout_policy(HttpTimeoutPolicy::production())
    }

    fn with_timeout_policy(timeout_policy: HttpTimeoutPolicy) -> Self {
        Self {
            http: Client::builder()
                .user_agent("EasiFlux-Desktop/0.3.0")
                .connect_timeout(timeout_policy.connect)
                .timeout(timeout_policy.request)
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
        let credential = credential.normalize();
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

    pub async fn set_base_url(&self, base_url: &str) {
        *self.base_url.write().await = normalize_base_url(base_url);
    }

    pub async fn has_credential(&self) -> bool {
        self.credential.read().await.is_some()
    }

    pub async fn base_url(&self) -> String {
        self.base_url.read().await.clone()
    }

    pub async fn public_get(
        &self,
        path: &str,
        params: HashMap<String, String>,
    ) -> AppResult<Value> {
        let url = format!("{}{}", self.base_url().await, path);
        let response = self.http.get(&url).query(&params).send().await?;
        self.parse_response(response, None).await
    }

    pub async fn private_get(&self, path: &str, params: QueryParams) -> AppResult<Value> {
        self.ensure_time_sync().await?;
        match self.private_get_once(path, &params).await {
            Ok(v) => Ok(v),
            Err(e) if should_retry_private_request(&e) => {
                self.force_time_sync().await?;
                self.private_get_once(path, &params).await
            }
            Err(e) => Err(e),
        }
    }

    async fn private_get_once(&self, path: &str, params: &QueryParams) -> AppResult<Value> {
        let query = encode_query(params);
        let headers = self.sign_headers(&query, "").await?;
        let base = format!("{}{}", self.base_url().await, path);
        let request_url = if query.is_empty() {
            base
        } else {
            format!("{}?{}", base, query)
        };
        let mut req = self.http.get(request_url);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let response = req.send().await?;
        self.parse_response(response, None).await
    }

    pub async fn private_post(&self, path: &str, body: Value) -> AppResult<Value> {
        self.ensure_time_sync().await?;
        match self.private_post_once(path, body.clone()).await {
            Ok(v) => Ok(v),
            Err(e) if should_retry_private_request(&e) => {
                self.force_time_sync().await?;
                self.private_post_once(path, body).await
            }
            Err(e) => Err(e),
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
        self.parse_response(response, Some(path)).await
    }

    async fn ensure_time_sync(&self) -> AppResult<()> {
        if self.time_sync.offset_ms() == 0 {
            self.force_time_sync().await?;
        }
        Ok(())
    }

    async fn force_time_sync(&self) -> AppResult<()> {
        sync_from_server(self.time_sync.as_ref(), PublicApi::server_time(self)).await
    }

    async fn sign_headers(&self, query: &str, body: &str) -> AppResult<Vec<(String, String)>> {
        let signer = self
            .signer
            .read()
            .await
            .clone()
            .ok_or(AppError::Auth("未配置 API 凭据".into()))?;
        let payload = if body.is_empty() { query } else { body };
        Ok(signer.prepare_headers(self.time_sync.timestamp_ms(), RECV_WINDOW_MS, payload))
    }

    async fn parse_response(
        &self,
        response: reqwest::Response,
        path: Option<&str>,
    ) -> AppResult<Value> {
        let status = response.status();
        let text = response.text().await?;
        if text.is_empty() {
            if status.is_success() {
                return Ok(json!({}));
            }
            return Err(AppError::Connection(format!("HTTP {}", status)));
        }
        let payload: Value = serde_json::from_str(&text)
            .map_err(|_| AppError::Connection("API 响应格式无效".into()))?;
        if status.is_success() && path == Some(endpoints::CREATE_ORDER) {
            match super::response::classify_create_order_outcome(&payload) {
                super::response::CreateOrderOutcome::Rejected => {
                    return Err(AppError::TradingFailure(
                        crate::models::trading::TradingFailure::rejected(),
                    ));
                }
                super::response::CreateOrderOutcome::Accepted(_) => {}
                super::response::CreateOrderOutcome::ProviderFailure => {}
                super::response::CreateOrderOutcome::Ambiguous => {
                    return Err(AppError::Internal("订单提交结果不明确".into()));
                }
            }
        }
        if !status.is_success() {
            return Err(AppError::Connection(format!("HTTP {}", status)));
        }
        if !is_success_response(&payload) {
            let msg = error_message(&payload).unwrap_or_else(|| "API 返回错误".into());
            if is_timestamp_error(&payload) {
                return Err(AppError::Trading("timestamp: 请求时间校验失败".into()));
            }
            if is_sign_error(&payload) || is_sign_error_message(&msg) {
                return Err(AppError::Trading(
                    "sign: 签名校验失败（请重新保存 API Secret）".into(),
                ));
            }
            return Err(AppError::Trading("API 返回错误".into()));
        }
        Ok(payload)
    }
}

fn should_retry_private_request(error: &AppError) -> bool {
    match error {
        AppError::Trading(msg) => {
            msg.contains("timestamp")
                || msg.contains("recv_window")
                || msg.contains("sign")
                || msg.contains("error sign")
        }
        AppError::Connection(msg) => msg.contains("timestamp") || msg.contains("recv_window"),
        AppError::TradingFailure(_) => false,
        _ => false,
    }
}

fn is_sign_error_message(message: &str) -> bool {
    message.contains("error sign") || message.contains("invalid signature")
}

impl Default for ApiClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Encode query preserving insertion order and URL escaping (SDK compatible).
pub fn encode_query(params: &QueryParams) -> String {
    if params.is_empty() {
        return String::new();
    }
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Signer;
    use crate::models::config::RECV_WINDOW_MS;

    async fn public_get_from_raw_response(response: String) -> AppError {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let address = listener.local_addr().expect("test listener address");
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept test request");
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).expect("read test request");
            socket
                .write_all(response.as_bytes())
                .expect("write test response");
        });
        let client = ApiClient::new();
        client.set_base_url(&format!("http://{address}")).await;

        let error = client
            .public_get("/test", HashMap::new())
            .await
            .expect_err("response must fail");
        server.join().expect("test server should exit");
        error
    }

    fn raw_response(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn provider_and_transport_details_are_not_exposed_to_command_callers() {
        const PRIVATE: &str = "provider-secret-quantity=999";
        let cases = [
            raw_response(
                "200 OK",
                &format!(r#"{{"code":7001,"message":"{PRIVATE}"}}"#),
            ),
            raw_response("400 Bad Request", &format!(r#"{{"message":"{PRIVATE}"}}"#)),
            raw_response(
                "200 OK",
                &format!(r#"{{"code":7002,"message":"timestamp {PRIVATE}"}}"#),
            ),
            raw_response(
                "200 OK",
                &format!(r#"{{"code":7003,"message":"invalid signature {PRIVATE}"}}"#),
            ),
            raw_response("200 OK", PRIVATE),
        ];

        for response in cases {
            let error = public_get_from_raw_response(response).await;
            for rendered in [error.to_string(), serde_json::to_string(&error).unwrap()] {
                assert!(
                    !rendered.contains(PRIVATE),
                    "leaked provider detail: {rendered}"
                );
                assert!(
                    !rendered.contains("127.0.0.1"),
                    "leaked request URL: {rendered}"
                );
            }
        }
    }

    #[tokio::test]
    async fn set_base_url_normalizes_public_request_target() {
        let client = ApiClient::new();

        client.set_base_url(" https://sandbox.example.test/ ").await;

        assert_eq!(client.base_url().await, "https://sandbox.example.test");
    }

    #[test]
    fn production_http_timeouts_are_explicit_and_bounded() {
        let policy = HttpTimeoutPolicy::production();

        assert_eq!(policy.connect, std::time::Duration::from_secs(5));
        assert_eq!(policy.request, std::time::Duration::from_secs(15));
        assert!(policy.connect < policy.request);
    }

    #[tokio::test]
    async fn stalled_response_is_cancelled_by_total_request_timeout() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        listener
            .set_nonblocking(true)
            .expect("make test listener nonblocking");
        let address = listener.local_addr().expect("test listener address");
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            loop {
                match listener.accept() {
                    Ok((_socket, _)) => {
                        std::thread::sleep(std::time::Duration::from_millis(250));
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "test server did not receive the request"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(error) => panic!("accept request: {error}"),
                }
            }
        });
        let request_timeout = std::time::Duration::from_millis(40);
        let client = ApiClient::with_timeout_policy(HttpTimeoutPolicy {
            connect: std::time::Duration::from_secs(1),
            request: request_timeout,
        });
        client.set_base_url(&format!("http://{address}")).await;

        let started = std::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            client.public_get("/stalled", HashMap::new()),
        )
        .await
        .expect("the configured request timeout should terminate the call");
        let elapsed = started.elapsed();
        server.join().expect("test server should exit");

        assert!(matches!(result, Err(AppError::Connection(_))));
        assert!(elapsed < std::time::Duration::from_millis(200));
    }

    #[test]
    fn encode_query_preserves_insertion_order() {
        let params = vec![
            ("symbol".into(), "BTCUSDT".into()),
            ("limit".into(), "10".into()),
        ];
        assert_eq!(encode_query(&params), "symbol=BTCUSDT&limit=10");
    }

    #[test]
    fn encode_query_multi_param_matches_sdk_order() {
        let params = vec![
            ("symbol".into(), "BTCUSDT".into()),
            ("coin".into(), "USDT".into()),
        ];
        assert_eq!(encode_query(&params), "symbol=BTCUSDT&coin=USDT");
    }

    #[test]
    fn encode_query_empty_params() {
        assert_eq!(encode_query(&Vec::new()), "");
    }

    #[test]
    fn signed_get_payload_matches_sdk_layout() {
        let signer = Signer::new("key".to_string(), "secret".to_string());
        let headers = signer.prepare_headers(1_700_000_000_000, RECV_WINDOW_MS, "symbol=BTCUSDT");
        let signature = headers
            .iter()
            .find(|(name, _)| name == "Access-Sign")
            .map(|(_, value)| value.as_str())
            .expect("signature header");
        let expected = signer.sign(&format!(
            "1700000000000key{}{}symbol=BTCUSDT",
            RECV_WINDOW_MS, ""
        ));
        assert_eq!(signature, expected);
    }

    #[test]
    fn signed_get_empty_query_payload_matches_sdk_layout() {
        let signer = Signer::new("key".to_string(), "secret".to_string());
        let headers = signer.prepare_headers(1_700_000_000_000, RECV_WINDOW_MS, "");
        let signature = headers
            .iter()
            .find(|(name, _)| name == "Access-Sign")
            .map(|(_, value)| value.as_str())
            .expect("signature header");
        let expected = signer.sign(&format!("1700000000000key{}{}", RECV_WINDOW_MS, ""));
        assert_eq!(signature, expected);
    }
}
