use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::api::endpoints::{WS_PRIVATE, WS_PUBLIC};
use crate::api::mapper::{parse_balance, parse_depth, parse_klines, parse_order, parse_position};
use crate::auth::Signer;
use crate::auth::TimeSync;
use crate::error::AppResult;
use crate::events::EventEmitter;
use crate::services::MarketService;

use super::messages::{build_auth_message, build_ping_message, build_subscribe_message, default_auth_expires_ms};
use super::topics::{
    topic_candle, topic_depth, topic_ticker, TOPIC_EXECUTION, TOPIC_ORDER, TOPIC_POSITION, TOPIC_WALLET,
};

const HEARTBEAT_SECS: u64 = 15;
const RECONNECT_SECS: u64 = 3;

#[derive(Clone)]
struct Subscription {
    topic: String,
    private: bool,
}

pub struct WsManager {
    ws_public_url: Arc<RwLock<String>>,
    ws_private_url: Arc<RwLock<String>>,
    signer: Arc<RwLock<Option<Signer>>>,
    time_sync: Arc<TimeSync>,
    emitter: EventEmitter,
    market: Arc<std::sync::RwLock<Option<Arc<MarketService>>>>,
    subscriptions: Arc<Mutex<Vec<Subscription>>>,
    running: Arc<AtomicBool>,
    public_connected: Arc<AtomicBool>,
    private_connected: Arc<AtomicBool>,
    active_symbol: Arc<RwLock<String>>,
    public_task: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    private_task: Arc<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>>,
    last_public_message_at: Arc<AtomicU64>,
    last_private_message_at: Arc<AtomicU64>,
}

impl WsManager {
    pub fn new(emitter: EventEmitter, time_sync: Arc<TimeSync>) -> Self {
        Self {
            ws_public_url: Arc::new(RwLock::new(WS_PUBLIC.to_string())),
            ws_private_url: Arc::new(RwLock::new(WS_PRIVATE.to_string())),
            signer: Arc::new(RwLock::new(None)),
            time_sync,
            emitter,
            market: Arc::new(std::sync::RwLock::new(None)),
            subscriptions: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(AtomicBool::new(false)),
            public_connected: Arc::new(AtomicBool::new(false)),
            private_connected: Arc::new(AtomicBool::new(false)),
            active_symbol: Arc::new(RwLock::new(String::new())),
            public_task: Arc::new(Mutex::new(None)),
            private_task: Arc::new(Mutex::new(None)),
            last_public_message_at: Arc::new(AtomicU64::new(0)),
            last_private_message_at: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn set_market(&self, market: Arc<MarketService>) {
        if let Ok(mut guard) = self.market.write() {
            *guard = Some(market);
        }
    }

    pub async fn configure(
        &self,
        ws_public_url: &str,
        ws_private_url: &str,
        signer: Signer,
    ) {
        *self.ws_public_url.write().await = ws_public_url.to_string();
        *self.ws_private_url.write().await = ws_private_url.to_string();
        *self.signer.write().await = Some(signer);
    }

    pub fn is_connected(&self) -> bool {
        self.public_connected.load(Ordering::Relaxed)
            || self.private_connected.load(Ordering::Relaxed)
    }

    pub fn is_public_connected(&self) -> bool {
        self.public_connected.load(Ordering::Relaxed)
    }

    pub fn is_private_connected(&self) -> bool {
        self.private_connected.load(Ordering::Relaxed)
    }

    fn is_channel_fresh(&self, last_message_at: &AtomicU64, stale_ms: u64) -> bool {
        let last = last_message_at.load(Ordering::Relaxed);
        if last == 0 {
            return false;
        }
        let now = self.time_sync.local_timestamp_ms();
        now.saturating_sub(last) <= stale_ms
    }

    pub fn is_public_healthy(&self, stale_ms: u64) -> bool {
        self.is_public_connected()
            && self.is_channel_fresh(&self.last_public_message_at, stale_ms)
    }

    pub fn is_private_healthy(&self, stale_ms: u64) -> bool {
        self.is_private_connected()
            && self.is_channel_fresh(&self.last_private_message_at, stale_ms)
    }

    fn touch_public_message(&self) {
        self.last_public_message_at.store(
            self.time_sync.local_timestamp_ms(),
            Ordering::Relaxed,
        );
    }

    fn touch_private_message(&self) {
        self.last_private_message_at.store(
            self.time_sync.local_timestamp_ms(),
            Ordering::Relaxed,
        );
    }

    pub async fn subscribe_all(&self, symbol: &str, kline_interval: &str) {
        let mut subs = self.subscriptions.lock().await;
        subs.clear();
        subs.push(Subscription {
            topic: topic_ticker(symbol),
            private: false,
        });
        subs.push(Subscription {
            topic: topic_depth(symbol, "1"),
            private: false,
        });
        subs.push(Subscription {
            topic: topic_candle(symbol, kline_interval),
            private: false,
        });
        for t in [TOPIC_POSITION, TOPIC_ORDER, TOPIC_EXECUTION, TOPIC_WALLET] {
            subs.push(Subscription {
                topic: t.to_string(),
                private: true,
            });
        }
    }

    pub async fn start(&self, symbol: &str) -> AppResult<()> {
        self.stop().await;
        *self.active_symbol.write().await = symbol.to_string();
        self.running.store(true, Ordering::Relaxed);
        self.emitter.emit_websocket("connecting");

        let ws_public = self.ws_public_url.read().await.clone();
        let ws_private = self.ws_private_url.read().await.clone();
        let public_topics = topic_snapshot(&self.subscriptions, false).await;
        let private_topics = topic_snapshot(&self.subscriptions, true).await;

        let emitter = self.emitter.clone();
        let market = self.market.read().ok().and_then(|g| g.clone());
        let signer = self.signer.read().await.clone();
        let time_sync = self.time_sync.clone();
        let symbol_owned = symbol.to_string();
        let running = self.running.clone();
        let public_connected = self.public_connected.clone();
        let private_connected = self.private_connected.clone();
        let subscriptions = self.subscriptions.clone();
        let last_public_message_at = self.last_public_message_at.clone();

        if !public_topics.is_empty() {
            let emitter_p = emitter.clone();
            let market_p = market.clone();
            let running_p = running.clone();
            let pc = public_connected.clone();
            let sym = symbol_owned.clone();
            let subs = subscriptions.clone();
            let last_msg = last_public_message_at.clone();
            let handle = tauri::async_runtime::spawn(async move {
                run_public_loop(ws_public, subs, sym, emitter_p, market_p, running_p, pc, last_msg).await;
            });
            *self.public_task.lock().await = Some(handle);
        }

        if !private_topics.is_empty() {
            if let Some(s) = signer {
                let emitter_pr = emitter.clone();
                let market_pr = market.clone();
                let running_pr = running.clone();
                let prc = private_connected.clone();
                let sym = symbol_owned.clone();
                let subs = subscriptions.clone();
                let last_msg = self.last_private_message_at.clone();
                let handle = tauri::async_runtime::spawn(async move {
                    run_private_loop(
                        ws_private,
                        subs,
                        s,
                        time_sync,
                        sym,
                        emitter_pr,
                        market_pr,
                        running_pr,
                        prc,
                        last_msg,
                    )
                    .await;
                });
                *self.private_task.lock().await = Some(handle);
            }
        }

        Ok(())
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        self.public_connected.store(false, Ordering::Relaxed);
        self.private_connected.store(false, Ordering::Relaxed);
        if let Some(handle) = self.public_task.lock().await.take() {
            handle.abort();
        }
        if let Some(handle) = self.private_task.lock().await.take() {
            handle.abort();
        }
    }
}

async fn run_public_loop(
    url: String,
    subscriptions: Arc<Mutex<Vec<Subscription>>>,
    symbol: String,
    emitter: EventEmitter,
    market: Option<Arc<MarketService>>,
    running: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    last_message_at: Arc<AtomicU64>,
) {
    while running.load(Ordering::Relaxed) {
        let topics = topic_snapshot(&subscriptions, false).await;
        if topics.is_empty() {
            tokio::time::sleep(Duration::from_secs(RECONNECT_SECS)).await;
            continue;
        }
        match run_public_session(
            &url,
            &topics,
            &symbol,
            &emitter,
            market.as_ref(),
            &running,
            &connected,
            &last_message_at,
        )
        .await {
            Ok(()) => connected.store(false, Ordering::Relaxed),
            Err(e) => {
                connected.store(false, Ordering::Relaxed);
                tracing::warn!("Public WS error: {}", e);
                emitter.emit_websocket("error");
            }
        }
        if !running.load(Ordering::Relaxed) {
            break;
        }
        tokio::time::sleep(Duration::from_secs(RECONNECT_SECS)).await;
    }
    emitter.emit_websocket("disconnected");
}

async fn run_private_loop(
    url: String,
    subscriptions: Arc<Mutex<Vec<Subscription>>>,
    signer: Signer,
    time_sync: Arc<TimeSync>,
    symbol: String,
    emitter: EventEmitter,
    market: Option<Arc<MarketService>>,
    running: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    last_message_at: Arc<AtomicU64>,
) {
    while running.load(Ordering::Relaxed) {
        let topics = topic_snapshot(&subscriptions, true).await;
        if topics.is_empty() {
            tokio::time::sleep(Duration::from_secs(RECONNECT_SECS)).await;
            continue;
        }
        match run_private_session(
            &url,
            &topics,
            &signer,
            &time_sync,
            &symbol,
            &emitter,
            market.as_ref(),
            &running,
            &connected,
            &last_message_at,
        )
        .await
        {
            Ok(()) => connected.store(false, Ordering::Relaxed),
            Err(e) => {
                connected.store(false, Ordering::Relaxed);
                tracing::warn!("Private WS error: {}", e);
                emitter.emit_error(&format!("Private WebSocket: {}", e));
                emitter.emit_websocket("error");
            }
        }
        if !running.load(Ordering::Relaxed) {
            break;
        }
        tokio::time::sleep(Duration::from_secs(RECONNECT_SECS)).await;
    }
}

async fn run_public_session(
    url: &str,
    topics: &[String],
    symbol: &str,
    emitter: &EventEmitter,
    market: Option<&Arc<MarketService>>,
    running: &Arc<AtomicBool>,
    connected: &Arc<AtomicBool>,
    last_message_at: &Arc<AtomicU64>,
) -> Result<(), String> {
    let (stream, _) = connect_async(url).await.map_err(|e| e.to_string())?;
    let (mut write, mut read) = stream.split();
    connected.store(true, Ordering::Relaxed);
    emitter.emit_websocket("connected");

    if let Some(market) = market {
        let interval = market.kline_interval().await;
        market.schedule_kline_backfill(symbol, &interval);
    }

    write
        .send(Message::Text(
            build_subscribe_message(topics).to_string().into(),
        ))
        .await
        .map_err(|e| e.to_string())?;

    let mut heartbeat = tokio::time::interval(Duration::from_secs(HEARTBEAT_SECS));
    loop {
        if !running.load(Ordering::Relaxed) {
            return Ok(());
        }
        tokio::select! {
            _ = heartbeat.tick() => {
                write.send(Message::Text(build_ping_message().to_string().into())).await.map_err(|e| e.to_string())?;
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(value) = serde_json::from_str::<Value>(&text) {
                            handle_message(&value, symbol, emitter, market, Some(last_message_at));
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return Err("closed".into()),
                    Some(Err(e)) => return Err(e.to_string()),
                    _ => {}
                }
            }
        }
    }
}

async fn run_private_session(
    url: &str,
    topics: &[String],
    signer: &Signer,
    time_sync: &TimeSync,
    symbol: &str,
    emitter: &EventEmitter,
    market: Option<&Arc<MarketService>>,
    running: &Arc<AtomicBool>,
    connected: &Arc<AtomicBool>,
    last_message_at: &Arc<AtomicU64>,
) -> Result<(), String> {
    let (stream, _) = connect_async(url).await.map_err(|e| e.to_string())?;
    let (mut write, mut read) = stream.split();
    connected.store(true, Ordering::Relaxed);
    emitter.emit_websocket("connected");

    let expires = default_auth_expires_ms(time_sync.timestamp_ms());
    write
        .send(Message::Text(
            build_auth_message(signer, expires).to_string().into(),
        ))
        .await
        .map_err(|e| e.to_string())?;
    write
        .send(Message::Text(
            build_subscribe_message(topics).to_string().into(),
        ))
        .await
        .map_err(|e| e.to_string())?;

    let mut heartbeat = tokio::time::interval(Duration::from_secs(HEARTBEAT_SECS));
    loop {
        if !running.load(Ordering::Relaxed) {
            return Ok(());
        }
        tokio::select! {
            _ = heartbeat.tick() => {
                write.send(Message::Text(build_ping_message().to_string().into())).await.map_err(|e| e.to_string())?;
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(value) = serde_json::from_str::<Value>(&text) {
                            if value.get("op").and_then(|v| v.as_str()) == Some("auth") {
                                continue;
                            }
                            handle_message(&value, symbol, emitter, market, Some(last_message_at));
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return Err("closed".into()),
                    Some(Err(e)) => return Err(e.to_string()),
                    _ => {}
                }
            }
        }
    }
}

fn handle_message(
    message: &Value,
    symbol: &str,
    emitter: &EventEmitter,
    market: Option<&Arc<MarketService>>,
    last_message_at: Option<&Arc<AtomicU64>>,
) {
    if let Some(last) = last_message_at {
        last.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            Ordering::Relaxed,
        );
    }

    let topic = message
        .get("topic")
        .or_else(|| message.get("channel"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match super::topics::event_name_for_topic(topic) {
        "ticker" => {
            let data = message.get("data").unwrap_or(message);
            if let Some(market) = market {
                if let Some(obj) = data.as_object() {
                    market.merge_and_emit_ticker(&Value::Object(obj.clone()), symbol);
                    return;
                }
                if let Some(arr) = data.as_array() {
                    if let Some(first) = arr.first() {
                        market.merge_and_emit_ticker(first, symbol);
                    }
                }
            }
        }
        "depth" => {
            let data = message.get("data").unwrap_or(message);
            emitter.emit_depth(parse_depth(data, symbol));
        }
        "candle" => {
            let data = message.get("data").unwrap_or(message);
            let interval = topic.split('.').nth(1).unwrap_or("1");
            let updates = parse_klines(data, symbol, interval);
            if let Some(market) = market {
                if market.merge_and_emit_klines(symbol, interval, updates) {
                    market.schedule_kline_backfill(symbol, interval);
                }
            }
        }
        "order" => {
            let data = message.get("data").unwrap_or(message);
            dispatch_list(data, |item| emitter.emit_order(parse_order(item)));
        }
        "position" => {
            let data = message.get("data").unwrap_or(message);
            dispatch_list(data, |item| emitter.emit_position(parse_position(item)));
        }
        "balance" => {
            let data = message.get("data").unwrap_or(message);
            dispatch_list(data, |item| emitter.emit_balance(parse_balance(item)));
        }
        _ => {}
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

async fn topic_snapshot(subscriptions: &Arc<Mutex<Vec<Subscription>>>, private: bool) -> Vec<String> {
    subscriptions
        .lock()
        .await
        .iter()
        .filter(|item| item.private == private)
        .map(|item| item.topic.clone())
        .collect()
}
