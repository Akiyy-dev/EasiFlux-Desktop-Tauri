use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

use crate::api::mapper::{parse_balance, parse_depth, parse_order, parse_position, parse_ticker};
use crate::auth::Signer;
use crate::auth::TimeSync;
use crate::error::{AppError, AppResult};
use crate::events::EventEmitter;

pub const PUBLIC_CHANNELS: &[&str] = &["ticker", "depth"];
pub const PRIVATE_CHANNELS: &[&str] = &["order", "position", "balance"];

#[derive(Clone)]
struct Subscription {
    channel: String,
    params: HashMap<String, String>,
    private: bool,
}

pub struct WsManager {
    base_url: Arc<RwLock<String>>,
    signer: Arc<RwLock<Option<Signer>>>,
    time_sync: Arc<TimeSync>,
    emitter: EventEmitter,
    subscriptions: Arc<Mutex<Vec<Subscription>>>,
    shutdown_tx: Arc<Mutex<Option<mpsc::Sender<()>>>>,
    active_symbol: Arc<RwLock<String>>,
    connected: Arc<AtomicBool>,
}

impl WsManager {
    pub fn new(emitter: EventEmitter, time_sync: Arc<TimeSync>) -> Self {
        Self {
            base_url: Arc::new(RwLock::new(String::new())),
            signer: Arc::new(RwLock::new(None)),
            time_sync,
            emitter,
            subscriptions: Arc::new(Mutex::new(Vec::new())),
            shutdown_tx: Arc::new(Mutex::new(None)),
            active_symbol: Arc::new(RwLock::new(String::new())),
            connected: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn configure(&self, base_url: &str, signer: Signer) {
        *self.base_url.write().await = base_url.trim_end_matches('/').to_string();
        *self.signer.write().await = Some(signer);
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub async fn is_running(&self) -> bool {
        self.shutdown_tx.lock().await.is_some()
    }

    pub async fn start(&self, symbol: &str) -> AppResult<()> {
        let was_running = self.shutdown_tx.lock().await.is_some();
        if was_running {
            if let Some(tx) = self.shutdown_tx.lock().await.take() {
                let _ = tx.send(()).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        *self.active_symbol.write().await = symbol.to_string();
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        *self.shutdown_tx.lock().await = Some(shutdown_tx);

        let ws_url = build_ws_url(&self.base_url.read().await)?;
        let subs = self.subscriptions.lock().await.clone();
        let emitter = self.emitter.clone();
        let signer = self.signer.read().await.clone();
        let time_sync = self.time_sync.clone();
        let symbol_owned = symbol.to_string();
        let connected = self.connected.clone();

        tracing::debug!("WebSocket starting: {}", ws_url);
        self.emitter.emit_websocket("connecting");

        tauri::async_runtime::spawn(async move {
            loop {
                match connect_async(&ws_url).await {
                    Ok((stream, _)) => {
                        connected.store(true, Ordering::Relaxed);
                        emitter.emit_websocket("connected");
                        tracing::debug!("WebSocket connected: {}", ws_url);
                        let (mut write, mut read) = stream.split();

                        if !send_subscriptions(&mut write, &subs, &signer, &time_sync, &emitter)
                            .await
                        {
                            connected.store(false, Ordering::Relaxed);
                            break;
                        }

                        loop {
                            tokio::select! {
                                _ = shutdown_rx.recv() => {
                                    let _ = write.close().await;
                                    connected.store(false, Ordering::Relaxed);
                                    emitter.emit_websocket("disconnected");
                                    return;
                                }
                                msg = read.next() => {
                                    match msg {
                                        Some(Ok(Message::Text(text))) => {
                                            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                                                if let Some(auth_ok) = parse_auth_response(&value) {
                                                    if auth_ok {
                                                        tracing::debug!("WebSocket auth succeeded");
                                                    } else {
                                                        let reason = auth_failure_reason(&value);
                                                        tracing::warn!("WebSocket auth failed: {}", reason);
                                                        emitter.emit_error(&format!(
                                                            "WebSocket 鉴权失败: {}",
                                                            reason
                                                        ));
                                                        emitter.emit_websocket("error");
                                                        connected.store(false, Ordering::Relaxed);
                                                        break;
                                                    }
                                                    continue;
                                                }
                                                handle_message(&value, &symbol_owned, &emitter);
                                            }
                                        }
                                        Some(Ok(Message::Close(_))) | None => {
                                            connected.store(false, Ordering::Relaxed);
                                            break;
                                        }
                                        Some(Err(e)) => {
                                            tracing::warn!("WebSocket read error: {}", e);
                                            connected.store(false, Ordering::Relaxed);
                                            emitter.emit_websocket("error");
                                            break;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        connected.store(false, Ordering::Relaxed);
                        tracing::warn!("WS connect failed: {}", e);
                        emitter.emit_error(&format!("WebSocket 连接失败: {}", e));
                        emitter.emit_websocket("error");
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        });

        Ok(())
    }

    pub async fn stop(&self) {
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(()).await;
        }
    }

    pub async fn subscribe_all(&self, symbol: &str) {
        let mut subs = self.subscriptions.lock().await;
        subs.clear();
        subs.push(Subscription {
            channel: "ticker".into(),
            params: HashMap::from([("symbol".into(), symbol.into())]),
            private: false,
        });
        subs.push(Subscription {
            channel: "depth".into(),
            params: HashMap::from([
                ("symbol".into(), symbol.into()),
                ("depth".into(), "20".into()),
            ]),
            private: false,
        });
        for ch in PRIVATE_CHANNELS {
            subs.push(Subscription {
                channel: (*ch).into(),
                params: HashMap::new(),
                private: true,
            });
        }
    }
}

pub fn build_ws_url(base: &str) -> AppResult<String> {
    if base.is_empty() {
        return Err(AppError::NotConnected);
    }
    let parsed = Url::parse(base).map_err(|e| AppError::Connection(e.to_string()))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::Connection("无效 base_url".into()))?;
    let scheme = if parsed.scheme() == "https" {
        "wss"
    } else {
        "ws"
    };
    Ok(format!("{}://{}/ws", scheme, host))
}

pub fn build_auth_message(signer: &Signer, timestamp: &str) -> Value {
    json!({
        "op": "auth",
        "args": {
            "api_key": signer.api_key(),
            "timestamp": timestamp,
            "sign": signer.ws_auth_sign(timestamp),
        }
    })
}

pub fn build_subscribe_message(channel: &str, params: &HashMap<String, String>) -> Value {
    json!({
        "op": "subscribe",
        "channel": channel,
        "args": params,
    })
}

pub fn parse_auth_response(message: &Value) -> Option<bool> {
    if message.get("op").and_then(|v| v.as_str()) != Some("auth") {
        return None;
    }
    if message.get("success").and_then(|v| v.as_bool()) == Some(true) {
        return Some(true);
    }
    if matches!(
        message.get("code").and_then(|v| v.as_i64()),
        Some(0) | Some(200)
    ) {
        return Some(true);
    }
    if message
        .get("data")
        .and_then(|d| d.get("success"))
        .and_then(|v| v.as_bool())
        == Some(true)
    {
        return Some(true);
    }
    if message.get("success").and_then(|v| v.as_bool()) == Some(false) {
        return Some(false);
    }
    if let Some(code) = message.get("code").and_then(|v| v.as_i64()) {
        if code != 0 && code != 200 {
            return Some(false);
        }
    }
    None
}

fn auth_failure_reason(message: &Value) -> String {
    for key in ["msg", "message", "error", "detail"] {
        if let Some(text) = message.get(key).and_then(|v| v.as_str()) {
            if !text.is_empty() {
                return text.to_string();
            }
        }
    }
    "未知原因".into()
}

async fn send_subscriptions(
    write: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    subs: &[Subscription],
    signer: &Option<Signer>,
    time_sync: &TimeSync,
    emitter: &EventEmitter,
) -> bool {
    let mut authenticated = false;
    for sub in subs {
        if sub.private && !authenticated {
            if let Some(s) = signer {
                let ts = time_sync.timestamp_ms().to_string();
                let auth = build_auth_message(s, &ts);
                if write
                    .send(Message::Text(auth.to_string().into()))
                    .await
                    .is_err()
                {
                    emitter.emit_error("WebSocket 鉴权消息发送失败");
                    emitter.emit_websocket("error");
                    return false;
                }
                tracing::debug!("WebSocket auth sent for private channels");
            }
            authenticated = true;
        }

        let msg = build_subscribe_message(&sub.channel, &sub.params);
        if write
            .send(Message::Text(msg.to_string().into()))
            .await
            .is_err()
        {
            emitter.emit_error(&format!("WebSocket 订阅 {} 失败", sub.channel));
            emitter.emit_websocket("error");
            return false;
        }
        tracing::debug!("WebSocket subscribed: {}", sub.channel);
    }
    true
}

fn handle_message(message: &Value, symbol: &str, emitter: &EventEmitter) {
    let channel = message
        .get("channel")
        .or_else(|| message.get("topic"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if channel == "ticker" || channel.contains("ticker") {
        if let Some(obj) = message.get("data").and_then(|d| d.as_object()) {
            emitter.emit_ticker(parse_ticker(&Value::Object(obj.clone()), symbol));
        }
    } else if channel == "depth" || channel.contains("depth") {
        let data = message.get("data").unwrap_or(message);
        emitter.emit_depth(parse_depth(data, symbol));
    } else if channel.contains("order") {
        let data = message.get("data").unwrap_or(message);
        dispatch_list(data, |item| emitter.emit_order(parse_order(item)));
    } else if channel.contains("position") {
        let data = message.get("data").unwrap_or(message);
        dispatch_list(data, |item| emitter.emit_position(parse_position(item)));
    } else if channel.contains("balance") {
        let data = message.get("data").unwrap_or(message);
        dispatch_list(data, |item| emitter.emit_balance(parse_balance(item)));
    }
}

fn dispatch_list(data: &Value, mut handler: impl FnMut(&Value)) {
    if let Some(arr) = data.as_array() {
        for item in arr {
            handler(item);
        }
    } else if data.is_object() {
        handler(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Signer;

    #[test]
    fn ws_url_from_https_base() {
        let url = build_ws_url("https://api.easicoin.io").unwrap();
        assert_eq!(url, "wss://api.easicoin.io/ws");
    }

    #[test]
    fn ws_url_rejects_empty_base() {
        assert!(build_ws_url("").is_err());
    }

    #[test]
    fn auth_message_format() {
        let signer = Signer::new("mykey".into(), "secret".into());
        let msg = build_auth_message(&signer, "1234567890");
        assert_eq!(msg["op"], "auth");
        assert_eq!(msg["args"]["api_key"], "mykey");
        assert_eq!(msg["args"]["timestamp"], "1234567890");
        assert_eq!(msg["args"]["sign"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn parse_auth_success_response() {
        let ok = json!({"op": "auth", "success": true});
        assert_eq!(parse_auth_response(&ok), Some(true));

        let code_ok = json!({"op": "auth", "code": 0});
        assert_eq!(parse_auth_response(&code_ok), Some(true));
    }

    #[test]
    fn parse_auth_failure_response() {
        let fail = json!({"op": "auth", "success": false, "msg": "invalid sign"});
        assert_eq!(parse_auth_response(&fail), Some(false));
    }

    #[test]
    fn parse_non_auth_message_returns_none() {
        let ticker = json!({"channel": "ticker", "data": {}});
        assert_eq!(parse_auth_response(&ticker), None);
    }
}
