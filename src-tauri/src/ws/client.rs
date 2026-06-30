use std::collections::HashMap;
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
        }
    }

    pub async fn configure(&self, base_url: &str, signer: Signer) {
        *self.base_url.write().await = base_url.trim_end_matches('/').to_string();
        *self.signer.write().await = Some(signer);
    }

    pub async fn is_running(&self) -> bool {
        self.shutdown_tx.lock().await.is_some()
    }

    pub async fn start(&self, symbol: &str) -> AppResult<()> {
        self.stop().await;
        *self.active_symbol.write().await = symbol.to_string();
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        *self.shutdown_tx.lock().await = Some(shutdown_tx);

        let ws_url = self.ws_url().await?;
        let subs = self.subscriptions.lock().await.clone();
        let emitter = self.emitter.clone();
        let signer = self.signer.read().await.clone();
        let time_sync = self.time_sync.clone();
        let symbol_owned = symbol.to_string();

        tauri::async_runtime::spawn(async move {
            loop {
                match connect_async(&ws_url).await {
                    Ok((stream, _)) => {
                        emitter.emit_connection("connected");
                        let (mut write, mut read) = stream.split();

                        if let Some(ref s) = signer {
                            let ts = time_sync.timestamp_ms().to_string();
                            let sign = s.ws_auth_sign(&ts);
                            let auth = json!({
                                "op": "auth",
                                "args": {
                                    "api_key": s.api_key(),
                                    "timestamp": ts,
                                    "sign": sign,
                                }
                            });
                            if write.send(Message::Text(auth.to_string().into())).await.is_err() {
                                emitter.emit_connection("error");
                                break;
                            }
                        }

                        for sub in &subs {
                            let msg = json!({
                                "op": "subscribe",
                                "channel": sub.channel,
                                "args": sub.params,
                            });
                            if write.send(Message::Text(msg.to_string().into())).await.is_err() {
                                break;
                            }
                        }

                        loop {
                            tokio::select! {
                                _ = shutdown_rx.recv() => {
                                    let _ = write.close().await;
                                    emitter.emit_connection("disconnected");
                                    return;
                                }
                                msg = read.next() => {
                                    match msg {
                                        Some(Ok(Message::Text(text))) => {
                                            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                                                handle_message(&value, &symbol_owned, &emitter);
                                            }
                                        }
                                        Some(Ok(Message::Close(_))) | None => break,
                                        Some(Err(_)) => break,
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("WS connect failed: {}", e);
                        emitter.emit_connection("error");
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
        self.emitter.emit_connection("disconnected");
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

    async fn ws_url(&self) -> AppResult<String> {
        let base = self.base_url.read().await.clone();
        if base.is_empty() {
            return Err(AppError::NotConnected);
        }
        let parsed = Url::parse(&base).map_err(|e| AppError::Connection(e.to_string()))?;
        let host = parsed.host_str().ok_or_else(|| AppError::Connection("无效 base_url".into()))?;
        let scheme = if parsed.scheme() == "https" { "wss" } else { "ws" };
        Ok(format!("{}://{}/ws", scheme, host))
    }
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
