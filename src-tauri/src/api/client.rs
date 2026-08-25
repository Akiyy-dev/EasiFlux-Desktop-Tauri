use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use crate::api::PublicApi;
use crate::auth::time_sync::sync_from_server;
use crate::auth::{Signer, TimeSync};
use crate::error::{AppError, AppResult};
use crate::models::config::{
    canonical_api_base_url, ApiCredential, DEFAULT_BASE_URL, RECV_WINDOW_MS,
};
use crate::models::trading::SessionContext;

use super::endpoints;
use super::response::{classify_auth_failure, is_success_response, AuthFailureKind};

/// Ordered query pairs for private GET signing (SDK insertion order).
pub type QueryParams = Vec<(String, String)>;
pub(crate) type AuthFailureObserver = Arc<
    dyn Fn(
            ApiSessionContext,
            AuthFailureKind,
        ) -> Pin<Box<dyn Future<Output = Option<String>> + Send>>
        + Send
        + Sync,
>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApiSessionContext {
    pub session: SessionContext,
    pub notification_session_token: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ApiSessionFingerprint([u8; 32]);

#[derive(Clone)]
struct InstalledApiSession {
    context: ApiSessionContext,
    fingerprint: ApiSessionFingerprint,
    request_owner: uuid::Uuid,
    rearm_pending: bool,
}

#[derive(Clone)]
struct RememberedApiSession {
    fingerprint: ApiSessionFingerprint,
    notification_session_token: String,
    rearm_pending: bool,
}

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
    canonical_api_base_url(url).unwrap_or_default()
}

#[derive(Clone)]
pub struct ApiClient {
    http: Client,
    base_url: Arc<RwLock<String>>,
    credential: Arc<RwLock<Option<ApiCredential>>>,
    signer: Arc<RwLock<Option<Signer>>>,
    time_sync: Arc<TimeSync>,
    session_context: Arc<RwLock<Option<InstalledApiSession>>>,
    remembered_session: Arc<RwLock<Option<RememberedApiSession>>>,
    session_install_lock: Arc<tokio::sync::Mutex<()>>,
    auth_failure_observer: Arc<std::sync::RwLock<Option<AuthFailureObserver>>>,
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
            session_context: Arc::new(RwLock::new(None)),
            remembered_session: Arc::new(RwLock::new(None)),
            session_install_lock: Arc::new(tokio::sync::Mutex::new(())),
            auth_failure_observer: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    pub fn time_sync(&self) -> Arc<TimeSync> {
        self.time_sync.clone()
    }

    pub async fn set_credential(&self, credential: ApiCredential) {
        let _install = self.session_install_lock.lock().await;
        let credential = credential.normalize();
        if !credential.is_valid() {
            *self.base_url.write().await = String::new();
            *self.credential.write().await = None;
            *self.signer.write().await = None;
            *self.session_context.write().await = None;
            return;
        }
        self.set_credential_material(credential).await;
        *self.session_context.write().await = None;
    }

    async fn set_credential_material(&self, credential: ApiCredential) {
        let base = normalize_base_url(&credential.base_url);
        *self.base_url.write().await = base;
        *self.signer.write().await = Some(Signer::new(
            credential.api_key.clone(),
            credential.api_secret.clone(),
        ));
        *self.credential.write().await = Some(credential);
    }

    pub(crate) async fn set_credential_for_session(
        &self,
        credential: ApiCredential,
        context: SessionContext,
    ) {
        let _install = self.session_install_lock.lock().await;
        let credential = credential.normalize();
        if !credential.is_valid() {
            *self.base_url.write().await = String::new();
            *self.credential.write().await = None;
            *self.signer.write().await = None;
            *self.session_context.write().await = None;
            return;
        }
        let fingerprint = api_session_fingerprint(&context.account_id, &credential);
        let remembered = self
            .remembered_session
            .read()
            .await
            .as_ref()
            .filter(|remembered| remembered.fingerprint == fingerprint)
            .cloned();
        let notification_session_token = remembered
            .as_ref()
            .map(|remembered| remembered.notification_session_token.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let rearm_pending = remembered.is_some_and(|remembered| remembered.rearm_pending);
        self.set_credential_material(credential).await;
        *self.session_context.write().await = Some(InstalledApiSession {
            context: ApiSessionContext {
                session: context,
                notification_session_token: notification_session_token.clone(),
            },
            fingerprint,
            request_owner: uuid::Uuid::new_v4(),
            rearm_pending,
        });
        *self.remembered_session.write().await = Some(RememberedApiSession {
            fingerprint,
            notification_session_token,
            rearm_pending,
        });
    }

    #[cfg(test)]
    pub(crate) async fn notification_session_context_for_test(&self) -> Option<ApiSessionContext> {
        self.session_context
            .read()
            .await
            .as_ref()
            .map(|installed| installed.context.clone())
    }

    pub(crate) fn set_auth_failure_observer(&self, observer: AuthFailureObserver) {
        if let Ok(mut current) = self.auth_failure_observer.write() {
            *current = Some(observer);
        }
    }

    pub async fn clear_credential(&self) {
        let _install = self.session_install_lock.lock().await;
        *self.credential.write().await = None;
        *self.signer.write().await = None;
        *self.session_context.write().await = None;
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
        self.parse_response(response, None, None).await
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
        let context = self.session_for_request().await;
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
        self.parse_response(response, None, context).await
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
        let context = self.session_for_request().await;
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
        self.parse_response(response, Some(path), context).await
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

    async fn session_for_request(&self) -> Option<InstalledApiSession> {
        let _install = self.session_install_lock.lock().await;
        self.session_context.read().await.clone()
    }

    async fn mark_current_session_expired(
        &self,
        request: &InstalledApiSession,
    ) -> Option<ApiSessionContext> {
        let _install = self.session_install_lock.lock().await;
        let context = {
            let mut installed = self.session_context.write().await;
            let current = installed
                .as_mut()
                .filter(|current| current.owns_response(request))?;
            current.rearm_pending = true;
            current.context.clone()
        };
        *self.remembered_session.write().await = Some(RememberedApiSession {
            fingerprint: request.fingerprint,
            notification_session_token: context.notification_session_token.clone(),
            rearm_pending: true,
        });
        Some(context)
    }

    async fn confirm_authenticated_private_response(&self, request: &InstalledApiSession) {
        let _install = self.session_install_lock.lock().await;
        let remembered = {
            let mut installed = self.session_context.write().await;
            let Some(current) = installed
                .as_mut()
                .filter(|current| current.owns_response(request) && current.rearm_pending)
            else {
                return;
            };
            current.context.notification_session_token = uuid::Uuid::new_v4().to_string();
            current.rearm_pending = false;
            RememberedApiSession {
                fingerprint: current.fingerprint,
                notification_session_token: current.context.notification_session_token.clone(),
                rearm_pending: false,
            }
        };
        *self.remembered_session.write().await = Some(remembered);
    }

    async fn parse_response(
        &self,
        response: reqwest::Response,
        path: Option<&str>,
        context: Option<InstalledApiSession>,
    ) -> AppResult<Value> {
        let status = response.status();
        let status_auth_failure =
            classify_auth_failure(Some(status.as_u16()), &serde_json::json!({}));
        let text = response.text().await?;
        if text.is_empty() && !status.is_success() {
            if status_auth_failure != AuthFailureKind::Other {
                return Err(AppError::AuthFailure(status_auth_failure));
            }
            return Err(AppError::Connection(format!("HTTP {}", status)));
        }
        let payload: Value = if text.is_empty() {
            json!({})
        } else {
            match serde_json::from_str(&text) {
                Ok(payload) => payload,
                Err(_) if status_auth_failure != AuthFailureKind::Other => {
                    return Err(AppError::AuthFailure(status_auth_failure));
                }
                Err(_) => return Err(AppError::Connection("API 响应格式无效".into())),
            }
        };
        let auth_failure = classify_auth_failure(Some(status.as_u16()), &payload);
        if auth_failure == AuthFailureKind::SessionExpired {
            let observer = self
                .auth_failure_observer
                .read()
                .ok()
                .and_then(|observer| observer.clone());
            let current_context = match context.as_ref() {
                Some(context) => self.mark_current_session_expired(context).await,
                None => None,
            };
            if let (Some(context), Some(observer)) = (current_context, observer) {
                if let Some(notification_id) = observer(context, auth_failure).await {
                    return Err(AppError::Notified {
                        code: "AUTH_SESSION_EXPIRED",
                        message: "账户会话已失效",
                        notification_id,
                        cause: Some(crate::error::NotificationCause::AuthFailure(auth_failure)),
                    });
                }
            }
        }
        if auth_failure != AuthFailureKind::Other {
            return Err(AppError::AuthFailure(auth_failure));
        }
        if !status.is_success() {
            return Err(AppError::Connection(format!("HTTP {}", status)));
        }
        if path == Some(endpoints::CREATE_ORDER) {
            match super::response::classify_create_order_outcome(&payload) {
                super::response::CreateOrderOutcome::Rejected => {
                    if let Some(context) = context.as_ref() {
                        self.confirm_authenticated_private_response(context).await;
                    }
                    return Err(AppError::TradingFailure(
                        crate::models::trading::TradingFailure::rejected(),
                    ));
                }
                super::response::CreateOrderOutcome::Accepted(_) => {}
                super::response::CreateOrderOutcome::Ambiguous => {
                    return Err(AppError::Internal("订单提交结果不明确".into()));
                }
            }
        }
        if !is_success_response(&payload) {
            return Err(AppError::Trading("API 返回错误".into()));
        }
        if let Some(context) = context.as_ref() {
            self.confirm_authenticated_private_response(context).await;
        }
        Ok(payload)
    }
}

impl InstalledApiSession {
    fn owns_response(&self, request: &Self) -> bool {
        self.request_owner == request.request_owner
            && self.fingerprint == request.fingerprint
            && self.context == request.context
    }
}

fn api_session_fingerprint(account_id: &str, credential: &ApiCredential) -> ApiSessionFingerprint {
    let mut hasher = Sha256::new();
    hasher.update(b"easiflux.api-notification-session.v1\0");
    for value in [
        account_id,
        credential.api_key.as_str(),
        credential.api_secret.as_str(),
        credential.base_url.as_str(),
    ] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    ApiSessionFingerprint(hasher.finalize().into())
}

fn should_retry_private_request(error: &AppError) -> bool {
    matches!(
        error,
        AppError::AuthFailure(AuthFailureKind::Timestamp | AuthFailureKind::Signature)
    )
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
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    fn accept_raw_request(listener: &std::net::TcpListener) -> (std::net::TcpStream, String) {
        use std::io::Read;

        let (mut socket, _) = listener.accept().expect("accept test request");
        let mut request = [0_u8; 2048];
        let length = socket.read(&mut request).expect("read test request");
        (
            socket,
            String::from_utf8_lossy(&request[..length]).into_owned(),
        )
    }

    fn write_json_response(socket: &mut std::net::TcpStream, body: &str) {
        use std::io::Write;

        socket
            .write_all(raw_response("200 OK", body).as_bytes())
            .expect("write test response");
    }

    async fn install_test_session(client: &ApiClient, address: std::net::SocketAddr) {
        client
            .set_credential_for_session(
                ApiCredential {
                    label: "Alpha".into(),
                    api_key: "key".into(),
                    api_secret: "secret".into(),
                    base_url: format!("http://{address}"),
                },
                SessionContext {
                    account_id: "alpha".into(),
                    session_epoch: 4,
                },
            )
            .await;
        client.time_sync().set_server_time(
            client
                .time_sync()
                .local_timestamp_ms()
                .saturating_add(1_000),
        );
    }

    #[tokio::test]
    async fn private_session_expired_returns_the_single_committed_notification_id() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let address = listener.local_addr().expect("test listener address");
        let server = std::thread::spawn(move || {
            for body in [
                r#"{"code":0,"data":{"time":"1782850580"}}"#,
                r#"{"code":26200003,"message":"provider secret expired text"}"#,
            ] {
                let (mut socket, _) = listener.accept().expect("accept test request");
                let mut request = [0_u8; 2048];
                let _ = socket.read(&mut request).expect("read test request");
                let response = raw_response("200 OK", body);
                socket
                    .write_all(response.as_bytes())
                    .expect("write test response");
            }
        });
        let client = ApiClient::new();
        client
            .set_credential_for_session(
                ApiCredential {
                    label: "Alpha".into(),
                    api_key: "key".into(),
                    api_secret: "secret".into(),
                    base_url: format!("http://{address}"),
                },
                SessionContext {
                    account_id: "alpha".into(),
                    session_epoch: 4,
                },
            )
            .await;
        let calls = Arc::new(AtomicUsize::new(0));
        let captured_calls = Arc::clone(&calls);
        client.set_auth_failure_observer(Arc::new(move |context, failure| {
            assert_eq!(context.session.account_id, "alpha");
            assert_eq!(context.session.session_epoch, 4);
            assert!(crate::models::notification::is_generated_uuid(
                &context.notification_session_token
            ));
            assert_eq!(failure, AuthFailureKind::SessionExpired);
            captured_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Some("committed-notification-id".to_string()) })
        }));

        let error = client
            .private_get("/private/test", Vec::new())
            .await
            .expect_err("documented session expiry must fail");
        server.join().expect("test server exits");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": "AUTH_SESSION_EXPIRED",
                "message": "账户会话已失效",
                "notificationId": "committed-notification-id",
            })
        );
    }

    #[tokio::test]
    async fn concurrent_private_success_rearms_the_expired_token_once() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let address = listener.local_addr().expect("test listener address");
        let (both_ready_tx, both_ready_rx) = tokio::sync::oneshot::channel();
        let (release_first_tx, release_first_rx) = std::sync::mpsc::channel();
        let (release_second_tx, release_second_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut expired, _) = accept_raw_request(&listener);
            write_json_response(&mut expired, r#"{"code":26200003}"#);

            let mut first = None;
            let mut second = None;
            for _ in 0..2 {
                let (socket, request) = accept_raw_request(&listener);
                if request.contains("GET /private/first ") {
                    first = Some(socket);
                } else if request.contains("GET /private/second ") {
                    second = Some(socket);
                } else {
                    panic!("unexpected request: {request}");
                }
            }
            both_ready_tx.send(()).expect("signal concurrent requests");
            release_first_rx.recv().expect("release first response");
            write_json_response(
                first.as_mut().expect("first request socket"),
                r#"{"code":0,"data":{"ok":true}}"#,
            );
            release_second_rx.recv().expect("release second response");
            write_json_response(
                second.as_mut().expect("second request socket"),
                r#"{"code":0,"data":{"ok":true}}"#,
            );
        });

        let client = ApiClient::new();
        install_test_session(&client, address).await;
        client.set_auth_failure_observer(Arc::new(|_, _| {
            Box::pin(async { Some("expired-notification".into()) })
        }));
        client
            .private_get("/private/expired", Vec::new())
            .await
            .expect_err("expiry arms authenticated recovery");
        let expired_token = client
            .notification_session_context_for_test()
            .await
            .unwrap()
            .notification_session_token;

        let first_client = client.clone();
        let first =
            tokio::spawn(
                async move { first_client.private_get("/private/first", Vec::new()).await },
            );
        let second_client = client.clone();
        let second = tokio::spawn(async move {
            second_client
                .private_get("/private/second", Vec::new())
                .await
        });
        both_ready_rx.await.expect("both requests reached server");

        release_first_tx.send(()).expect("release first success");
        first.await.unwrap().unwrap();
        let recovered_token = client
            .notification_session_context_for_test()
            .await
            .unwrap()
            .notification_session_token;
        assert_ne!(recovered_token, expired_token);

        release_second_tx.send(()).expect("release second success");
        second.await.unwrap().unwrap();
        assert_eq!(
            client
                .notification_session_context_for_test()
                .await
                .unwrap()
                .notification_session_token,
            recovered_token
        );
        server.join().expect("test server exits");
    }

    #[tokio::test]
    async fn stale_expiry_after_authenticated_recovery_cannot_notify() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let address = listener.local_addr().expect("test listener address");
        let (stale_ready_tx, stale_ready_rx) = tokio::sync::oneshot::channel();
        let (release_stale_tx, release_stale_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut expired, _) = accept_raw_request(&listener);
            write_json_response(&mut expired, r#"{"code":26200003}"#);

            let (mut stale, request) = accept_raw_request(&listener);
            assert!(request.contains("GET /private/stale-expiry "));
            stale_ready_tx.send(()).expect("signal stale request");

            let (mut recovered, request) = accept_raw_request(&listener);
            assert!(request.contains("GET /private/recovered "));
            write_json_response(
                &mut recovered,
                r#"{"code":0,"data":{"authenticated":true}}"#,
            );

            release_stale_rx.recv().expect("release stale expiry");
            write_json_response(&mut stale, r#"{"code":26200003}"#);
        });

        let client = ApiClient::new();
        install_test_session(&client, address).await;
        let notifications = Arc::new(AtomicUsize::new(0));
        let captured_notifications = Arc::clone(&notifications);
        client.set_auth_failure_observer(Arc::new(move |_, _| {
            captured_notifications.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Some("expired-notification".into()) })
        }));
        client
            .private_get("/private/expired", Vec::new())
            .await
            .expect_err("initial expiry must notify");
        let expired_token = client
            .notification_session_context_for_test()
            .await
            .unwrap()
            .notification_session_token;

        let stale_client = client.clone();
        let stale = tokio::spawn(async move {
            stale_client
                .private_get("/private/stale-expiry", Vec::new())
                .await
        });
        stale_ready_rx.await.expect("stale request reached server");
        client
            .private_get("/private/recovered", Vec::new())
            .await
            .expect("newer private success confirms recovery");
        assert_ne!(
            client
                .notification_session_context_for_test()
                .await
                .unwrap()
                .notification_session_token,
            expired_token
        );

        release_stale_tx.send(()).expect("release stale failure");
        let stale_error = stale.await.unwrap().expect_err("stale expiry still fails");
        assert!(matches!(
            stale_error,
            AppError::AuthFailure(AuthFailureKind::SessionExpired)
        ));
        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        server.join().expect("test server exits");
    }

    #[tokio::test]
    async fn stale_private_success_cannot_rotate_a_replaced_identity() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let address = listener.local_addr().expect("test listener address");
        let (stale_ready_tx, stale_ready_rx) = tokio::sync::oneshot::channel();
        let (release_stale_tx, release_stale_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            for body in [
                r#"{"code":26200003}"#,
                r#"{"code":0,"data":{"authenticated":true}}"#,
                r#"{"code":26200003}"#,
            ] {
                let (mut socket, _) = accept_raw_request(&listener);
                write_json_response(&mut socket, body);
            }
            let (mut stale, request) = accept_raw_request(&listener);
            assert!(request.contains("GET /private/stale-success "));
            stale_ready_tx.send(()).expect("signal stale request");
            release_stale_rx.recv().expect("release stale success");
            write_json_response(&mut stale, r#"{"code":0,"data":{"authenticated":true}}"#);
        });

        let client = ApiClient::new();
        install_test_session(&client, address).await;
        client.set_auth_failure_observer(Arc::new(|_, _| {
            Box::pin(async { Some("expired-notification".into()) })
        }));
        client
            .private_get("/private/expired", Vec::new())
            .await
            .expect_err("initial expiry must fail");
        client
            .private_get("/private/recovered", Vec::new())
            .await
            .expect("current private success confirms recovery");
        client
            .private_get("/private/expired-again", Vec::new())
            .await
            .expect_err("second expiry arms another recovery");

        let stale_client = client.clone();
        let stale = tokio::spawn(async move {
            stale_client
                .private_get("/private/stale-success", Vec::new())
                .await
        });
        stale_ready_rx.await.expect("stale success reached server");
        client
            .set_credential_for_session(
                ApiCredential {
                    label: "Replacement".into(),
                    api_key: "key-two".into(),
                    api_secret: "secret-two".into(),
                    base_url: format!("http://{address}"),
                },
                SessionContext {
                    account_id: "alpha".into(),
                    session_epoch: 4,
                },
            )
            .await;
        let replacement_token = client
            .notification_session_context_for_test()
            .await
            .unwrap()
            .notification_session_token;

        release_stale_tx.send(()).expect("release stale success");
        stale.await.unwrap().unwrap();
        assert_eq!(
            client
                .notification_session_context_for_test()
                .await
                .unwrap()
                .notification_session_token,
            replacement_token
        );
        server.join().expect("test server exits");
    }

    #[tokio::test]
    async fn empty_private_get_and_post_success_rearm_expired_tokens() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let address = listener.local_addr().expect("test listener address");
        let server = std::thread::spawn(move || {
            for (expected_request, body) in [
                ("GET /private/expired ", r#"{"code":26200003}"#),
                ("GET /private/empty ", ""),
                ("GET /private/expired-again ", r#"{"code":26200003}"#),
                ("POST /private/empty ", ""),
            ] {
                let (mut socket, request) = accept_raw_request(&listener);
                assert!(request.contains(expected_request), "{request}");
                write_json_response(&mut socket, body);
            }
        });

        let client = ApiClient::new();
        install_test_session(&client, address).await;
        client.set_auth_failure_observer(Arc::new(|_, _| {
            Box::pin(async { Some("expired-notification".into()) })
        }));
        client
            .private_get("/private/expired", Vec::new())
            .await
            .expect_err("initial expiry must fail");
        let first_expired_token = client
            .notification_session_context_for_test()
            .await
            .unwrap()
            .notification_session_token;

        assert_eq!(
            client
                .private_get("/private/empty", Vec::new())
                .await
                .unwrap(),
            json!({})
        );
        let get_recovered_token = client
            .notification_session_context_for_test()
            .await
            .unwrap()
            .notification_session_token;
        assert_ne!(get_recovered_token, first_expired_token);

        client
            .private_get("/private/expired-again", Vec::new())
            .await
            .expect_err("second expiry must fail");
        assert_eq!(
            client
                .private_post("/private/empty", json!({"request": true}))
                .await
                .unwrap(),
            json!({})
        );
        assert_ne!(
            client
                .notification_session_context_for_test()
                .await
                .unwrap()
                .notification_session_token,
            get_recovered_token
        );
        server.join().expect("test server exits");
    }

    #[tokio::test]
    async fn ambiguous_create_order_cannot_rearm_before_special_validation() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let address = listener.local_addr().expect("test listener address");
        let server = std::thread::spawn(move || {
            for body in [
                r#"{"code":26200003}"#,
                r#"{"code":0,"data":{}}"#,
                r#"{"code":0,"data":{"order_id":"order-1"}}"#,
            ] {
                let (mut socket, _) = accept_raw_request(&listener);
                write_json_response(&mut socket, body);
            }
        });

        let client = ApiClient::new();
        install_test_session(&client, address).await;
        client.set_auth_failure_observer(Arc::new(|_, _| {
            Box::pin(async { Some("expired-notification".into()) })
        }));
        client
            .private_get("/private/expired", Vec::new())
            .await
            .expect_err("initial expiry must fail");
        let expired_token = client
            .notification_session_context_for_test()
            .await
            .unwrap()
            .notification_session_token;

        let ambiguous = client
            .private_post(endpoints::CREATE_ORDER, json!({"symbol": "BTCUSDT"}))
            .await
            .expect_err("ambiguous order response must fail closed");
        assert!(matches!(ambiguous, AppError::Internal(_)));
        assert_eq!(
            client
                .notification_session_context_for_test()
                .await
                .unwrap()
                .notification_session_token,
            expired_token
        );

        client
            .private_post(endpoints::CREATE_ORDER, json!({"symbol": "BTCUSDT"}))
            .await
            .expect("structurally accepted order confirms private authentication");
        assert_ne!(
            client
                .notification_session_context_for_test()
                .await
                .unwrap()
                .notification_session_token,
            expired_token
        );
        server.join().expect("test server exits");
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
    async fn documented_http_403_is_rate_limited_even_without_json_but_401_and_429_are_other() {
        for body in ["", "<html>provider secret</html>"] {
            let error = public_get_from_raw_response(raw_response("403 Forbidden", body)).await;
            assert!(matches!(
                error,
                AppError::AuthFailure(AuthFailureKind::RateLimited)
            ));
        }
        for code in [26200003, 26200002, 26200004, 99999999] {
            let error = public_get_from_raw_response(raw_response(
                "403 Forbidden",
                &serde_json::json!({"code": code, "message": "session expired"}).to_string(),
            ))
            .await;
            assert!(matches!(
                error,
                AppError::AuthFailure(AuthFailureKind::RateLimited)
            ));
        }
        for status in ["401 Unauthorized", "429 Too Many Requests"] {
            let error = public_get_from_raw_response(raw_response(status, "")).await;
            assert!(
                matches!(error, AppError::Connection(_)),
                "{status}: {error:?}"
            );
        }
        let spoof = public_get_from_raw_response(raw_response(
            "401 Unauthorized",
            r#"{"code":99999999,"message":"session expired invalid api key"}"#,
        ))
        .await;
        assert!(!matches!(
            spoof,
            AppError::AuthFailure(AuthFailureKind::SessionExpired)
        ));
    }

    #[tokio::test]
    async fn set_base_url_normalizes_public_request_target() {
        let client = ApiClient::new();

        client.set_base_url(" https://sandbox.example.test/ ").await;

        assert_eq!(client.base_url().await, "https://sandbox.example.test");
    }

    #[tokio::test]
    async fn unsafe_credential_url_is_quarantined_without_production_fallback_or_raw_error() {
        const UNSAFE: &str = "https://user:raw-secret@127.0.0.1:9/api?token=raw";
        let client = ApiClient::new();

        client
            .set_credential(ApiCredential {
                label: "unsafe".into(),
                api_key: "key".into(),
                api_secret: "secret".into(),
                base_url: UNSAFE.into(),
            })
            .await;

        assert!(!client.has_credential().await);
        assert_eq!(client.base_url().await, "");
        assert_ne!(client.base_url().await, DEFAULT_BASE_URL);
        let error = client
            .public_get("/common/timestamp", HashMap::new())
            .await
            .expect_err("quarantined client must not send a request");
        let rendered = serde_json::to_string(&error).unwrap();
        assert!(!rendered.contains("raw-secret"));
        assert!(!rendered.contains("token="));
        assert!(!rendered.contains("127.0.0.1"));
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
