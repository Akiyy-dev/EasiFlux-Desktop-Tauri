use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{Sink, SinkExt, Stream, StreamExt};
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::api::endpoints::{WS_PRIVATE, WS_PUBLIC};
use crate::api::mapper::{parse_balance, parse_depth, parse_klines, parse_order, parse_position};
use crate::auth::Signer;
use crate::auth::TimeSync;
use crate::error::AppResult;
use crate::events::EventEmitter;
use crate::models::config::normalize_account_id;
use crate::models::config::AppConfig;
use crate::models::trading::{Order, OrderStreamContext};
use crate::services::connection::{ConnectionObservationSource, SessionNotificationObserver};
use crate::services::trading::OrderNotificationObserver;
use crate::services::{AccountLifecycleCoordinator, MarketService};

use super::messages::{
    build_auth_message, build_ping_message, build_subscribe_message, default_auth_expires_ms,
};
use super::topics::{
    topic_candle, topic_depth, topic_ticker, TOPIC_EXECUTION, TOPIC_ORDER, TOPIC_POSITION,
    TOPIC_WALLET,
};

const HEARTBEAT_SECS: u64 = 15;
const RECONNECT_SECS: u64 = 3;
const PRIVATE_AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const PRIVATE_AUTH_ERROR: &str = "私有 WebSocket 鉴权失败";
const PRIVATE_SUBSCRIPTION_ERROR: &str = "私有 WebSocket 订阅失败";
const PUBLIC_SESSION_ERROR: &str = "公共 WebSocket 会话不可用";
const PRIVATE_SESSION_ERROR: &str = "私有 WebSocket 会话不可用";

#[derive(Clone, Copy)]
enum FreshnessDomain {
    Ticker,
    Depth,
    Candle,
    Balance,
    Order,
    Position,
}

#[derive(Default)]
struct WsFreshness {
    ticker: AtomicU64,
    depth: AtomicU64,
    candle: AtomicU64,
    balance: AtomicU64,
    order: AtomicU64,
    position: AtomicU64,
}

impl WsFreshness {
    fn timestamp(&self, domain: FreshnessDomain) -> &AtomicU64 {
        match domain {
            FreshnessDomain::Ticker => &self.ticker,
            FreshnessDomain::Depth => &self.depth,
            FreshnessDomain::Candle => &self.candle,
            FreshnessDomain::Balance => &self.balance,
            FreshnessDomain::Order => &self.order,
            FreshnessDomain::Position => &self.position,
        }
    }

    fn touch(&self, domain: FreshnessDomain, timestamp_ms: u64) {
        self.timestamp(domain)
            .store(timestamp_ms, Ordering::Relaxed);
    }

    fn reset_domains(&self, domains: &[FreshnessDomain]) {
        for domain in domains {
            self.touch(*domain, 0);
        }
    }

    fn reset_public(&self) {
        self.reset_domains(&[
            FreshnessDomain::Ticker,
            FreshnessDomain::Depth,
            FreshnessDomain::Candle,
        ]);
    }

    fn reset_private(&self) {
        self.reset_domains(&[
            FreshnessDomain::Balance,
            FreshnessDomain::Order,
            FreshnessDomain::Position,
        ]);
    }

    fn reset(&self) {
        self.reset_public();
        self.reset_private();
    }

    fn is_fresh_at(&self, domain: FreshnessDomain, now_ms: u64, stale_ms: u64) -> bool {
        let last = self.timestamp(domain).load(Ordering::Relaxed);
        last != 0 && now_ms.saturating_sub(last) <= stale_ms
    }

    fn is_market_fresh_at(&self, now_ms: u64, stale_ms: u64) -> bool {
        [
            FreshnessDomain::Ticker,
            FreshnessDomain::Depth,
            FreshnessDomain::Candle,
        ]
        .into_iter()
        .all(|domain| self.is_fresh_at(domain, now_ms, stale_ms))
    }

    fn is_balance_fresh_at(&self, now_ms: u64, stale_ms: u64) -> bool {
        self.is_fresh_at(FreshnessDomain::Balance, now_ms, stale_ms)
    }

    fn is_private_panels_fresh_at(&self, now_ms: u64, stale_ms: u64) -> bool {
        [FreshnessDomain::Order, FreshnessDomain::Position]
            .into_iter()
            .all(|domain| self.is_fresh_at(domain, now_ms, stale_ms))
    }
}

async fn abort_and_wait_for_tasks(
    public_task: Option<tauri::async_runtime::JoinHandle<()>>,
    private_task: Option<tauri::async_runtime::JoinHandle<()>>,
) {
    if let Some(handle) = public_task.as_ref() {
        handle.abort();
    }
    if let Some(handle) = private_task.as_ref() {
        handle.abort();
    }
    if let Some(handle) = public_task {
        let _ = handle.await;
    }
    if let Some(handle) = private_task {
        let _ = handle.await;
    }
}

async fn abort_wait_and_reset_freshness(
    public_task: Option<tauri::async_runtime::JoinHandle<()>>,
    private_task: Option<tauri::async_runtime::JoinHandle<()>>,
    public_connected: &AtomicBool,
    private_connected: &AtomicBool,
    freshness: &WsFreshness,
) {
    abort_and_wait_for_tasks(public_task, private_task).await;
    public_connected.store(false, Ordering::Relaxed);
    private_connected.store(false, Ordering::Relaxed);
    freshness.reset();
}

#[derive(Clone)]
struct Subscription {
    topic: String,
    private: bool,
}

#[derive(Default)]
struct OrderObservationBuffer {
    context: Option<OrderStreamContext>,
    snapshot_seeded: bool,
    pending: Vec<Order>,
}

impl OrderObservationBuffer {
    fn reset(&mut self, context: OrderStreamContext) {
        self.context = Some(context);
        self.snapshot_seeded = false;
        self.pending.clear();
    }

    fn matches(&self, context: &OrderStreamContext) -> bool {
        self.context.as_ref() == Some(context)
    }

    fn defer(&mut self, context: &OrderStreamContext, orders: Vec<Order>) -> bool {
        if !self.matches(context) || self.snapshot_seeded {
            return false;
        }
        self.pending.extend(orders);
        true
    }

    fn pending_for_seed(&self, context: &OrderStreamContext) -> Option<Vec<Order>> {
        if !self.matches(context) || self.snapshot_seeded {
            return None;
        }
        Some(self.pending.clone())
    }

    fn finish_seed(&mut self, context: &OrderStreamContext) -> bool {
        if !self.matches(context) || self.snapshot_seeded {
            return false;
        }
        self.pending.clear();
        self.snapshot_seeded = true;
        true
    }
}

async fn seed_order_gate<F, Fut>(
    order_observation_buffer: &Arc<Mutex<OrderObservationBuffer>>,
    order_snapshot_seeded: &AtomicBool,
    context: &OrderStreamContext,
    replay: F,
) -> AppResult<()>
where
    F: FnOnce(Vec<Order>) -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let mut buffer = order_observation_buffer.lock().await;
    let Some(pending) = buffer.pending_for_seed(context) else {
        return Ok(());
    };
    replay(pending).await?;
    if buffer.finish_seed(context) {
        order_snapshot_seeded.store(true, Ordering::Release);
    }
    Ok(())
}

async fn observe_manual_order_snapshots_for_session(
    config: &Arc<RwLock<AppConfig>>,
    account_lifecycle: &Arc<AccountLifecycleCoordinator>,
    order_observation_buffer: &Arc<Mutex<OrderObservationBuffer>>,
    observer: &OrderNotificationObserver,
    context: &OrderStreamContext,
    snapshots: &[Order],
    now_ms: u64,
) {
    let active_account_id = normalize_account_id(&config.read().await.active_account_id);
    if account_lifecycle.current_session_epoch() != context.session_epoch
        || active_account_id != context.account_id
    {
        return;
    }
    let buffer = order_observation_buffer.lock().await;
    if !buffer.matches(context) || !buffer.snapshot_seeded {
        return;
    }
    for (offset, snapshot) in snapshots.iter().enumerate() {
        observer
            .observe(
                &context.account_id,
                context.session_epoch,
                snapshot,
                crate::services::notification::OrderObservationOrigin::Snapshot,
                None,
                now_ms.saturating_add(offset as u64),
            )
            .await;
    }
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
    freshness: Arc<WsFreshness>,
    notification_observer: OrderNotificationObserver,
    session_notification_observer: SessionNotificationObserver,
    config: Arc<RwLock<AppConfig>>,
    account_lifecycle: Arc<AccountLifecycleCoordinator>,
    order_observation_buffer: Arc<Mutex<OrderObservationBuffer>>,
    order_snapshot_seeded: Arc<AtomicBool>,
}

impl WsManager {
    pub fn new(
        emitter: EventEmitter,
        time_sync: Arc<TimeSync>,
        notification_observer: OrderNotificationObserver,
        config: Arc<RwLock<AppConfig>>,
        account_lifecycle: Arc<AccountLifecycleCoordinator>,
        session_notification_observer: SessionNotificationObserver,
    ) -> Self {
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
            freshness: Arc::new(WsFreshness::default()),
            notification_observer,
            session_notification_observer,
            config,
            account_lifecycle,
            order_observation_buffer: Arc::new(Mutex::new(OrderObservationBuffer::default())),
            order_snapshot_seeded: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_market(&self, market: Arc<MarketService>) {
        if let Ok(mut guard) = self.market.write() {
            *guard = Some(market);
        }
    }

    pub async fn configure(&self, ws_public_url: &str, ws_private_url: &str, signer: Signer) {
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

    pub fn is_market_healthy(&self, stale_ms: u64) -> bool {
        self.is_public_connected()
            && self
                .freshness
                .is_market_fresh_at(self.time_sync.local_timestamp_ms(), stale_ms)
    }

    pub fn is_balance_healthy(&self, stale_ms: u64) -> bool {
        self.is_private_connected()
            && self
                .freshness
                .is_balance_fresh_at(self.time_sync.local_timestamp_ms(), stale_ms)
    }

    pub fn is_private_panels_healthy(&self, stale_ms: u64) -> bool {
        self.order_snapshot_seeded.load(Ordering::Acquire)
            && self.is_private_connected()
            && self
                .freshness
                .is_private_panels_fresh_at(self.time_sync.local_timestamp_ms(), stale_ms)
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

    pub async fn start(&self, symbol: &str, context: OrderStreamContext) -> AppResult<()> {
        self.stop().await;
        self.order_observation_buffer
            .lock()
            .await
            .reset(context.clone());
        self.order_snapshot_seeded.store(false, Ordering::Release);
        *self.active_symbol.write().await = symbol.to_string();
        self.running.store(true, Ordering::Relaxed);
        self.emitter
            .emit_websocket_for_session(&context, "connecting");

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
        let freshness = self.freshness.clone();
        let notification_observer = self.notification_observer.clone();
        let session_notification_observer = self.session_notification_observer.clone();
        let config = self.config.clone();
        let account_lifecycle = self.account_lifecycle.clone();
        let order_observation_buffer = self.order_observation_buffer.clone();

        if !public_topics.is_empty() {
            let emitter_p = emitter.clone();
            let market_p = market.clone();
            let running_p = running.clone();
            let pc = public_connected.clone();
            let sym = symbol_owned.clone();
            let subs = subscriptions.clone();
            let session_freshness = freshness.clone();
            let stream_context = context.clone();
            let handle = tauri::async_runtime::spawn(async move {
                run_public_loop(
                    ws_public,
                    subs,
                    sym,
                    emitter_p,
                    market_p,
                    running_p,
                    pc,
                    session_freshness,
                    stream_context,
                )
                .await;
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
                let session_freshness = freshness.clone();
                let observer = notification_observer.clone();
                let session_observer = session_notification_observer.clone();
                let session_config = config.clone();
                let lifecycle = account_lifecycle.clone();
                let observation_buffer = order_observation_buffer.clone();
                let stream_context = context.clone();
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
                        session_freshness,
                        observer,
                        session_observer,
                        session_config,
                        lifecycle,
                        stream_context,
                        observation_buffer,
                    )
                    .await;
                });
                *self.private_task.lock().await = Some(handle);
            }
        }

        Ok(())
    }

    /// Opens the live-order gate after the caller has seeded REST snapshots
    /// while holding the account lifecycle read guard.
    pub(crate) async fn seed_order_snapshots_and_mark(
        &self,
        context: &OrderStreamContext,
        snapshots: &[Order],
    ) -> AppResult<()> {
        let active_account_id = normalize_account_id(&self.config.read().await.active_account_id);
        if self.account_lifecycle.current_session_epoch() != context.session_epoch
            || active_account_id != context.account_id
        {
            return Ok(());
        }
        seed_order_gate(
            &self.order_observation_buffer,
            &self.order_snapshot_seeded,
            context,
            |pending| {
                self.notification_observer.seed_snapshot_then_replay(
                    context,
                    snapshots,
                    pending,
                    local_timestamp_ms(),
                )
            },
        )
        .await
    }

    /// Applies a manual REST snapshot only after the initial scheduler snapshot
    /// has opened this immutable session's live-order gate. The caller already
    /// holds the lifecycle read guard.
    pub(crate) async fn observe_manual_order_snapshots(
        &self,
        context: &OrderStreamContext,
        snapshots: &[Order],
    ) {
        observe_manual_order_snapshots_for_session(
            &self.config,
            &self.account_lifecycle,
            &self.order_observation_buffer,
            &self.notification_observer,
            context,
            snapshots,
            local_timestamp_ms(),
        )
        .await;
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        self.public_connected.store(false, Ordering::Relaxed);
        self.private_connected.store(false, Ordering::Relaxed);
        let public_task = self.public_task.lock().await.take();
        let private_task = self.private_task.lock().await.take();
        abort_wait_and_reset_freshness(
            public_task,
            private_task,
            &self.public_connected,
            &self.private_connected,
            &self.freshness,
        )
        .await;
    }

    pub(crate) async fn activate_committed_session(&self, context: &OrderStreamContext) {
        if self.is_private_connected() {
            self.session_notification_observer
                .observe_connection_status_guarded(
                    context,
                    ConnectionObservationSource::PrivateWebsocket,
                    crate::models::config::ConnectionStatus::Connected,
                    local_timestamp_ms(),
                )
                .await;
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
    freshness: Arc<WsFreshness>,
    context: OrderStreamContext,
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
            &freshness,
            &context,
        )
        .await
        {
            Ok(()) => connected.store(false, Ordering::Relaxed),
            Err(_) => {
                connected.store(false, Ordering::Relaxed);
                tracing::warn!("public websocket session unavailable");
                emitter.emit_websocket_for_session(&context, "error");
            }
        }
        freshness.reset_public();
        if !running.load(Ordering::Relaxed) {
            break;
        }
        tokio::time::sleep(Duration::from_secs(RECONNECT_SECS)).await;
    }
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
    freshness: Arc<WsFreshness>,
    notification_observer: OrderNotificationObserver,
    session_notification_observer: SessionNotificationObserver,
    config: Arc<RwLock<AppConfig>>,
    account_lifecycle: Arc<AccountLifecycleCoordinator>,
    context: OrderStreamContext,
    order_observation_buffer: Arc<Mutex<OrderObservationBuffer>>,
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
            &freshness,
            &notification_observer,
            &session_notification_observer,
            &config,
            &account_lifecycle,
            &context,
            &order_observation_buffer,
        )
        .await
        {
            Ok(()) => connected.store(false, Ordering::Relaxed),
            Err(_) => {
                report_private_failure_if_running(&running, || async {
                    connected.store(false, Ordering::Relaxed);
                    tracing::warn!("private websocket session unavailable");
                    emitter.emit_websocket_for_session(&context, "error");
                    session_notification_observer
                        .observe_connection_status(
                            &context,
                            ConnectionObservationSource::PrivateWebsocket,
                            crate::models::config::ConnectionStatus::Error,
                            local_timestamp_ms(),
                        )
                        .await;
                })
                .await;
            }
        }
        freshness.reset_private();
        if !running.load(Ordering::Relaxed) {
            break;
        }
        tokio::time::sleep(Duration::from_secs(RECONNECT_SECS)).await;
    }
}

async fn report_private_failure_if_running<F, Fut>(running: &AtomicBool, report: F) -> bool
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    if !running.load(Ordering::Acquire) {
        return false;
    }
    report().await;
    true
}

async fn run_public_session(
    url: &str,
    topics: &[String],
    symbol: &str,
    emitter: &EventEmitter,
    market: Option<&Arc<MarketService>>,
    running: &Arc<AtomicBool>,
    connected: &Arc<AtomicBool>,
    freshness: &Arc<WsFreshness>,
    context: &OrderStreamContext,
) -> Result<(), String> {
    let (stream, _) = connect_async(url)
        .await
        .map_err(|_| PUBLIC_SESSION_ERROR.to_string())?;
    let (mut write, mut read) = stream.split();
    if !running.load(Ordering::Relaxed) {
        return Ok(());
    }
    connected.store(true, Ordering::Relaxed);
    emitter.emit_websocket_for_session(context, "connected");

    if let Some(market) = market {
        let interval = market.kline_interval().await;
        market.schedule_kline_backfill(symbol, &interval);
    }

    write
        .send(Message::Text(
            build_subscribe_message(topics).to_string().into(),
        ))
        .await
        .map_err(|_| PUBLIC_SESSION_ERROR.to_string())?;

    let mut heartbeat = tokio::time::interval(Duration::from_secs(HEARTBEAT_SECS));
    loop {
        if !running.load(Ordering::Relaxed) {
            return Ok(());
        }
        tokio::select! {
            _ = heartbeat.tick() => {
                write.send(Message::Text(build_ping_message().to_string().into())).await.map_err(|_| PUBLIC_SESSION_ERROR.to_string())?;
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(value) = serde_json::from_str::<Value>(&text) {
                            handle_message(&value, symbol, emitter, market, Some(freshness), context);
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return Err("连接已关闭".into()),
                    Some(Err(_)) => return Err(PUBLIC_SESSION_ERROR.to_string()),
                    _ => {}
                }
            }
        }
    }
}

fn validate_private_auth_ack(response: &Value) -> Result<(), &'static str> {
    if response.get("op").and_then(Value::as_str) == Some("auth")
        && response.get("success").and_then(Value::as_bool) == Some(true)
        && response.get("request").is_none()
        && ["code", "retCode", "ret_code"]
            .iter()
            .all(|field| response.get(*field).is_none())
    {
        Ok(())
    } else {
        Err(PRIVATE_AUTH_ERROR)
    }
}

async fn await_private_auth_response<S>(read: &mut S, timeout: Duration) -> Result<(), String>
where
    S: Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let message = tokio::time::timeout(timeout, read.next())
        .await
        .map_err(|_| PRIVATE_AUTH_ERROR.to_string())?
        .ok_or_else(|| PRIVATE_AUTH_ERROR.to_string())?
        .map_err(|_| PRIVATE_AUTH_ERROR.to_string())?;
    let Message::Text(text) = message else {
        return Err(PRIVATE_AUTH_ERROR.to_string());
    };
    let response =
        serde_json::from_str::<Value>(&text).map_err(|_| PRIVATE_AUTH_ERROR.to_string())?;
    validate_private_auth_ack(&response).map_err(str::to_string)
}

async fn authenticate_and_subscribe<W, S, F>(
    write: &mut W,
    read: &mut S,
    auth_message: &Value,
    topics: &[String],
    running: &AtomicBool,
    connected: &AtomicBool,
    on_connected: F,
    auth_timeout: Duration,
) -> Result<(), String>
where
    W: Sink<Message> + Unpin,
    S: Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    F: FnOnce(),
{
    connected.store(false, Ordering::Relaxed);
    write
        .send(Message::Text(auth_message.to_string().into()))
        .await
        .map_err(|_| PRIVATE_AUTH_ERROR.to_string())?;
    await_private_auth_response(read, auth_timeout).await?;
    write
        .send(Message::Text(
            build_subscribe_message(topics).to_string().into(),
        ))
        .await
        .map_err(|_| PRIVATE_SUBSCRIPTION_ERROR.to_string())?;
    if running.load(Ordering::Relaxed) {
        connected.store(true, Ordering::Relaxed);
        on_connected();
    }
    Ok(())
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
    freshness: &Arc<WsFreshness>,
    notification_observer: &OrderNotificationObserver,
    session_notification_observer: &SessionNotificationObserver,
    config: &Arc<RwLock<AppConfig>>,
    account_lifecycle: &Arc<AccountLifecycleCoordinator>,
    context: &OrderStreamContext,
    order_observation_buffer: &Arc<Mutex<OrderObservationBuffer>>,
) -> Result<(), String> {
    let (stream, _) = connect_async(url)
        .await
        .map_err(|_| PRIVATE_SESSION_ERROR.to_string())?;
    let (mut write, mut read) = stream.split();

    let expires = default_auth_expires_ms(time_sync.timestamp_ms());
    authenticate_and_subscribe(
        &mut write,
        &mut read,
        &build_auth_message(signer, expires),
        topics,
        running,
        connected,
        || {},
        PRIVATE_AUTH_TIMEOUT,
    )
    .await?;
    if connected.load(Ordering::Relaxed) {
        emitter.emit_websocket_for_session(context, "connected");
        session_notification_observer
            .observe_connection_status(
                context,
                ConnectionObservationSource::PrivateWebsocket,
                crate::models::config::ConnectionStatus::Connected,
                local_timestamp_ms(),
            )
            .await;
    }

    let mut heartbeat = tokio::time::interval(Duration::from_secs(HEARTBEAT_SECS));
    loop {
        if !running.load(Ordering::Relaxed) {
            return Ok(());
        }
        tokio::select! {
            _ = heartbeat.tick() => {
                write.send(Message::Text(build_ping_message().to_string().into())).await.map_err(|_| PRIVATE_SESSION_ERROR.to_string())?;
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(value) = serde_json::from_str::<Value>(&text) {
                            handle_message(&value, symbol, emitter, market, Some(freshness), context);
                            observe_private_orders(
                                &value,
                                notification_observer,
                                config,
                                account_lifecycle,
                                context,
                                order_observation_buffer,
                            )
                            .await;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return Err("连接已关闭".into()),
                    Some(Err(_)) => return Err(PRIVATE_SESSION_ERROR.to_string()),
                    _ => {}
                }
            }
        }
    }
}

async fn observe_private_orders(
    message: &Value,
    observer: &OrderNotificationObserver,
    config: &Arc<RwLock<AppConfig>>,
    account_lifecycle: &Arc<AccountLifecycleCoordinator>,
    context: &OrderStreamContext,
    order_observation_buffer: &Arc<Mutex<OrderObservationBuffer>>,
) {
    let topic = message
        .get("topic")
        .or_else(|| message.get("channel"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if super::topics::event_name_for_topic(topic) != "order" {
        return;
    }
    let data = message.get("data").unwrap_or(message);
    let orders = if let Some(items) = data.as_array() {
        items.iter().map(parse_order).collect::<Vec<_>>()
    } else if data.is_object() {
        vec![parse_order(data)]
    } else {
        Vec::new()
    };
    if orders.is_empty() {
        return;
    }

    // Lock order is lifecycle -> config -> live buffer -> notification. The
    // scheduler holds the same lifecycle read guard while seeding and opening
    // the buffer, so a pending account switch cannot invert these locks.
    let _lifecycle_guard = account_lifecycle.read_guard().await;
    let active_account_id = normalize_account_id(&config.read().await.active_account_id);
    if account_lifecycle.current_session_epoch() != context.session_epoch
        || active_account_id != context.account_id
    {
        return;
    }
    let mut buffer = order_observation_buffer.lock().await;
    if !buffer.matches(context) {
        return;
    }
    if !buffer.snapshot_seeded {
        let _ = buffer.defer(context, orders);
        return;
    }
    for order in orders {
        observer
            .observe(
                &context.account_id,
                context.session_epoch,
                &order,
                crate::services::notification::OrderObservationOrigin::Realtime,
                None,
                local_timestamp_ms(),
            )
            .await;
    }
}

fn message_is_control_frame(message: &Value) -> bool {
    message.get("op").is_some()
        || message
            .get("request")
            .and_then(Value::as_object)
            .is_some_and(|request| request.contains_key("op"))
}

fn freshness_domain_for_topic(topic: &str) -> Option<FreshnessDomain> {
    if topic.starts_with(super::topics::TOPIC_TICKER_PREFIX) {
        return Some(FreshnessDomain::Ticker);
    }
    if topic.starts_with(super::topics::TOPIC_DEPTH_PREFIX) {
        return Some(FreshnessDomain::Depth);
    }
    if topic.starts_with(super::topics::TOPIC_CANDLE_PREFIX) {
        return Some(FreshnessDomain::Candle);
    }
    match topic {
        TOPIC_WALLET => Some(FreshnessDomain::Balance),
        TOPIC_ORDER => Some(FreshnessDomain::Order),
        TOPIC_POSITION => Some(FreshnessDomain::Position),
        _ => None,
    }
}

fn record_message_freshness(message: &Value, freshness: &WsFreshness, timestamp_ms: u64) {
    if message_is_control_frame(message) {
        return;
    }
    let topic = message
        .get("topic")
        .or_else(|| message.get("channel"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(domain) = freshness_domain_for_topic(topic) {
        freshness.touch(domain, timestamp_ms);
    }
}

fn local_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn handle_message(
    message: &Value,
    symbol: &str,
    emitter: &EventEmitter,
    market: Option<&Arc<MarketService>>,
    freshness: Option<&WsFreshness>,
    context: &OrderStreamContext,
) {
    if let Some(freshness) = freshness {
        record_message_freshness(message, freshness, local_timestamp_ms());
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
            dispatch_list(data, |item| emitter.emit_order(context, parse_order(item)));
        }
        "position" => {
            let data = message.get("data").unwrap_or(message);
            dispatch_list(data, |item| {
                emitter.emit_position(context, parse_position(item))
            });
        }
        "balance" => {
            let data = message.get("data").unwrap_or(message);
            dispatch_list(data, |item| {
                emitter.emit_balance(context, parse_balance(item))
            });
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

async fn topic_snapshot(
    subscriptions: &Arc<Mutex<Vec<Subscription>>>,
    private: bool,
) -> Vec<String> {
    subscriptions
        .lock()
        .await
        .iter()
        .filter(|item| item.private == private)
        .map(|item| item.topic.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use futures_util::{stream, Sink};
    use serde_json::json;
    use tokio_tungstenite::tungstenite::Message;

    use super::{
        abort_and_wait_for_tasks, abort_wait_and_reset_freshness, authenticate_and_subscribe,
        await_private_auth_response, observe_manual_order_snapshots_for_session,
        record_message_freshness, report_private_failure_if_running, seed_order_gate,
        validate_private_auth_ack, FreshnessDomain, OrderObservationBuffer, WsFreshness,
        PRIVATE_AUTH_ERROR, PRIVATE_SUBSCRIPTION_ERROR,
    };
    use crate::models::config::AppConfig;
    use crate::models::notification::{ListNotificationsRequest, NotificationFilter};
    use crate::models::trading::{Order, OrderStatus, OrderStreamContext};
    use crate::services::notification::{
        NotificationAvailability, NotificationEmitter, NotificationRuntime, NotificationService,
        ViewContext,
    };
    use crate::services::trading::OrderNotificationObserver;
    use crate::services::AccountLifecycleCoordinator;
    use crate::storage::notification_store::{NotificationFileV1, NotificationPersistence};
    use crate::storage::NotificationStore;

    struct FailAtPersistence {
        attempts: AtomicUsize,
        fail_at: usize,
    }

    impl NotificationPersistence for FailAtPersistence {
        fn save(&self, _file: &NotificationFileV1) -> crate::error::AppResult<()> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt == self.fail_at {
                Err(crate::error::AppError::Storage(
                    "private-notification-store-detail".into(),
                ))
            } else {
                Ok(())
            }
        }
    }

    fn buffered_order(id: &str, status: OrderStatus) -> Order {
        Order {
            order_id: id.into(),
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Limit".into(),
            price: "100".into(),
            qty: "1".into(),
            status,
            order_link_id: None,
            filled_qty: "0".into(),
            avg_price: "0".into(),
        }
    }

    #[test]
    fn live_order_buffer_preserves_frame_order_until_matching_snapshot_is_seeded() {
        let context = OrderStreamContext {
            account_id: "alpha".into(),
            session_epoch: 7,
        };
        let wrong = OrderStreamContext {
            account_id: "beta".into(),
            session_epoch: 7,
        };
        let mut buffer = OrderObservationBuffer::default();
        buffer.reset(context.clone());
        assert!(buffer.defer(
            &context,
            vec![
                buffered_order("order-buffer-1", OrderStatus::New),
                buffered_order("order-buffer-1", OrderStatus::PartiallyFilled),
                buffered_order("order-buffer-1", OrderStatus::Filled),
            ],
        ));
        assert!(buffer.pending_for_seed(&wrong).is_none());

        let pending = buffer.pending_for_seed(&context).unwrap();
        assert_eq!(
            pending
                .into_iter()
                .map(|order| order.status)
                .collect::<Vec<_>>(),
            vec![
                OrderStatus::New,
                OrderStatus::PartiallyFilled,
                OrderStatus::Filled,
            ]
        );
        assert!(buffer.finish_seed(&context));
        assert!(!buffer.defer(
            &context,
            vec![buffered_order("order-buffer-2", OrderStatus::New)]
        ));
    }

    #[tokio::test]
    async fn cancelled_seed_retains_pending_and_retries_once() {
        let context = OrderStreamContext {
            account_id: "alpha".into(),
            session_epoch: 7,
        };
        let buffer = Arc::new(tokio::sync::Mutex::new(OrderObservationBuffer::default()));
        buffer.lock().await.reset(context.clone());
        assert!(buffer.lock().await.defer(
            &context,
            vec![
                buffered_order("order-cancel-seed-1", OrderStatus::New),
                buffered_order("order-cancel-seed-1", OrderStatus::Filled),
            ],
        ));
        let seeded = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (_release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        let task_buffer = Arc::clone(&buffer);
        let task_seeded = Arc::clone(&seeded);
        let task_context = context.clone();
        let task = tokio::spawn(async move {
            seed_order_gate(
                &task_buffer,
                &task_seeded,
                &task_context,
                |pending| async move {
                    assert_eq!(pending.len(), 2);
                    let _ = started_tx.send(());
                    let _ = release_rx.await;
                    Ok(())
                },
            )
            .await
        });
        started_rx.await.unwrap();

        let drain_buffer = Arc::clone(&buffer);
        let drain_context = context.clone();
        let mut drain = tokio::spawn(async move {
            drain_buffer.lock().await.defer(
                &drain_context,
                vec![buffered_order("order-cancel-seed-2", OrderStatus::New)],
            )
        });
        assert!(tokio::time::timeout(Duration::from_millis(20), &mut drain)
            .await
            .is_err());
        task.abort();
        let _ = task.await;
        assert!(drain.await.unwrap());

        let gate = buffer.lock().await;
        assert!(!gate.snapshot_seeded);
        assert_eq!(gate.pending.len(), 3);
        drop(gate);
        let replayed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let replayed_for_seed = Arc::clone(&replayed);
        seed_order_gate(&buffer, &seeded, &context, move |pending| async move {
            for order in pending {
                let identity = (order.order_id, order.status);
                let mut observed = replayed_for_seed.lock().unwrap();
                if !observed.contains(&identity) {
                    observed.push(identity);
                }
            }
            Ok(())
        })
        .await
        .unwrap();

        assert!(seeded.load(Ordering::Acquire));
        let gate = buffer.lock().await;
        assert!(gate.snapshot_seeded);
        assert!(gate.pending.is_empty());
        assert_eq!(replayed.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn failed_gate_replay_retains_full_batch_and_retries_partial_commit_once() {
        let persistence = Arc::new(FailAtPersistence {
            attempts: AtomicUsize::new(0),
            fail_at: 2,
        });
        let emitted = Arc::new(AtomicUsize::new(0));
        let emitted_by_callback = Arc::clone(&emitted);
        let emitter: NotificationEmitter = Arc::new(move |_| {
            emitted_by_callback.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        let service = Arc::new(NotificationService::from_snapshot(
            NotificationFileV1::empty(),
            Arc::clone(&persistence),
            emitter,
            1_784_606_400_000,
        ));
        let observer = OrderNotificationObserver::new(Arc::new(NotificationRuntime::Available(
            Arc::clone(&service),
        )));
        let context = OrderStreamContext {
            account_id: "alpha".into(),
            session_epoch: 7,
        };
        let buffer = Arc::new(tokio::sync::Mutex::new(OrderObservationBuffer::default()));
        buffer.lock().await.reset(context.clone());
        let pending = vec![
            buffered_order("order-gate-persist-1", OrderStatus::New),
            buffered_order("order-gate-persist-1", OrderStatus::Filled),
            buffered_order("order-gate-persist-2", OrderStatus::PartiallyFilled),
            buffered_order("order-gate-persist-2", OrderStatus::Filled),
        ];
        assert!(buffer.lock().await.defer(&context, pending.clone()));
        let seeded = AtomicBool::new(false);

        let first = seed_order_gate(&buffer, &seeded, &context, |buffered| {
            observer.seed_snapshot_then_replay(&context, &[], buffered, 1_784_606_400_000)
        })
        .await;

        assert!(first.is_err());
        let rendered = first.unwrap_err().to_string();
        assert!(!rendered.contains("private-notification-store-detail"));
        assert!(!seeded.load(Ordering::Acquire));
        let gate = buffer.lock().await;
        assert!(!gate.snapshot_seeded);
        assert_eq!(
            gate.pending
                .iter()
                .map(|order| (order.order_id.as_str(), order.status.clone()))
                .collect::<Vec<_>>(),
            pending
                .iter()
                .map(|order| (order.order_id.as_str(), order.status.clone()))
                .collect::<Vec<_>>()
        );
        drop(gate);
        assert_eq!(service.revision().await, "1");
        assert_eq!(emitted.load(Ordering::SeqCst), 1);

        seed_order_gate(&buffer, &seeded, &context, |buffered| {
            observer.seed_snapshot_then_replay(&context, &[], buffered, 1_784_606_400_100)
        })
        .await
        .unwrap();

        assert!(seeded.load(Ordering::Acquire));
        let gate = buffer.lock().await;
        assert!(gate.snapshot_seeded);
        assert!(gate.pending.is_empty());
        drop(gate);
        assert_eq!(persistence.attempts.load(Ordering::SeqCst), 3);
        assert_eq!(service.revision().await, "2");
        assert_eq!(emitted.load(Ordering::SeqCst), 2);
        let records = service
            .list(
                ViewContext::account("alpha").unwrap(),
                ListNotificationsRequest {
                    account_id: Some("alpha".into()),
                    filter: NotificationFilter::All,
                    cursor: None,
                    limit: 100,
                },
                1_784_606_401_000,
            )
            .await
            .unwrap()
            .items;
        assert_eq!(records.len(), 2);
    }

    #[tokio::test]
    async fn fail_once_gate_replay_stays_closed_until_the_batch_commits() {
        let persistence = Arc::new(FailAtPersistence {
            attempts: AtomicUsize::new(0),
            fail_at: 1,
        });
        let emitted = Arc::new(AtomicUsize::new(0));
        let emitted_by_callback = Arc::clone(&emitted);
        let service = Arc::new(NotificationService::from_snapshot(
            NotificationFileV1::empty(),
            Arc::clone(&persistence),
            Arc::new(move |_| {
                emitted_by_callback.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
            1_784_606_400_000,
        ));
        let observer = OrderNotificationObserver::new(Arc::new(NotificationRuntime::Available(
            Arc::clone(&service),
        )));
        let context = OrderStreamContext {
            account_id: "alpha".into(),
            session_epoch: 7,
        };
        let buffer = Arc::new(tokio::sync::Mutex::new(OrderObservationBuffer::default()));
        buffer.lock().await.reset(context.clone());
        assert!(buffer.lock().await.defer(
            &context,
            vec![
                buffered_order("order-gate-fail-once-1", OrderStatus::New),
                buffered_order("order-gate-fail-once-1", OrderStatus::Filled),
            ],
        ));
        let seeded = AtomicBool::new(false);

        let first = seed_order_gate(&buffer, &seeded, &context, |pending| {
            observer.seed_snapshot_then_replay(&context, &[], pending, 1_784_606_400_000)
        })
        .await;

        assert!(first.is_err());
        assert!(!first
            .unwrap_err()
            .to_string()
            .contains("private-notification-store-detail"));
        assert!(!seeded.load(Ordering::Acquire));
        let gate = buffer.lock().await;
        assert!(!gate.snapshot_seeded);
        assert_eq!(gate.pending.len(), 2);
        drop(gate);
        assert_eq!(persistence.attempts.load(Ordering::SeqCst), 1);
        assert_eq!(service.revision().await, "0");
        assert_eq!(emitted.load(Ordering::SeqCst), 0);

        seed_order_gate(&buffer, &seeded, &context, |pending| {
            observer.seed_snapshot_then_replay(&context, &[], pending, 1_784_606_400_100)
        })
        .await
        .unwrap();

        assert!(seeded.load(Ordering::Acquire));
        let gate = buffer.lock().await;
        assert!(gate.snapshot_seeded);
        assert!(gate.pending.is_empty());
        drop(gate);
        assert_eq!(persistence.attempts.load(Ordering::SeqCst), 2);
        assert_eq!(service.revision().await, "1");
        assert_eq!(emitted.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn permanent_invalid_order_content_is_skipped_and_gate_opens_without_retry_loop() {
        let persistence = Arc::new(FailAtPersistence {
            attempts: AtomicUsize::new(0),
            fail_at: usize::MAX,
        });
        let service = Arc::new(NotificationService::from_snapshot(
            NotificationFileV1::empty(),
            Arc::clone(&persistence),
            Arc::new(|_| Ok(())),
            1_784_606_400_000,
        ));
        let observer = OrderNotificationObserver::new(Arc::new(NotificationRuntime::Available(
            Arc::clone(&service),
        )));
        let context = OrderStreamContext {
            account_id: "alpha".into(),
            session_epoch: 7,
        };
        let mut new = buffered_order("", OrderStatus::New);
        new.order_link_id = Some("private-invalid-order-link".into());
        let mut filled = new.clone();
        filled.status = OrderStatus::Filled;
        let buffer = Arc::new(tokio::sync::Mutex::new(OrderObservationBuffer::default()));
        buffer.lock().await.reset(context.clone());
        assert!(buffer.lock().await.defer(&context, vec![new, filled]));
        let seeded = AtomicBool::new(false);

        let result = tokio::time::timeout(
            Duration::from_millis(100),
            seed_order_gate(&buffer, &seeded, &context, |pending| {
                observer.seed_snapshot_then_replay(&context, &[], pending, 1_784_606_400_000)
            }),
        )
        .await
        .expect("permanent notification validation must not enter retry backoff");

        assert!(result.is_ok());
        assert!(seeded.load(Ordering::Acquire));
        let gate = buffer.lock().await;
        assert!(gate.snapshot_seeded);
        assert!(gate.pending.is_empty());
        drop(gate);
        assert_eq!(persistence.attempts.load(Ordering::SeqCst), 0);
        assert_eq!(service.revision().await, "0");
        let records = service
            .list(
                ViewContext::account("alpha").unwrap(),
                ListNotificationsRequest {
                    account_id: Some("alpha".into()),
                    filter: NotificationFilter::All,
                    cursor: None,
                    limit: 100,
                },
                1_784_606_401_000,
            )
            .await
            .unwrap()
            .items;
        assert!(records.is_empty());
    }

    #[tokio::test]
    async fn unavailable_notification_runtime_opens_gate_without_retry_error() {
        let observer = OrderNotificationObserver::new(Arc::new(NotificationRuntime::Unavailable(
            NotificationAvailability::new(
                "NOTIFICATION_FUTURE_SCHEMA",
                "private-unavailable-detail",
            ),
        )));
        let context = OrderStreamContext {
            account_id: "alpha".into(),
            session_epoch: 7,
        };
        let buffer = Arc::new(tokio::sync::Mutex::new(OrderObservationBuffer::default()));
        buffer.lock().await.reset(context.clone());
        assert!(buffer.lock().await.defer(
            &context,
            vec![
                buffered_order("order-unavailable-runtime-1", OrderStatus::New),
                buffered_order("order-unavailable-runtime-1", OrderStatus::Filled),
            ],
        ));
        let seeded = AtomicBool::new(false);

        let result = seed_order_gate(&buffer, &seeded, &context, |pending| {
            observer.seed_snapshot_then_replay(&context, &[], pending, 1_784_606_400_000)
        })
        .await;

        assert!(result.is_ok());
        assert!(seeded.load(Ordering::Acquire));
        let gate = buffer.lock().await;
        assert!(gate.snapshot_seeded);
        assert!(gate.pending.is_empty());
    }

    #[tokio::test]
    async fn manual_snapshot_cannot_interleave_with_ws_drain_and_stale_context_is_noop() {
        let path = std::env::temp_dir()
            .join(format!(
                "easiflux-manual-order-gate-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ))
            .join("notifications.v1.json");
        let emitter: NotificationEmitter = Arc::new(|_| Ok(()));
        let service = Arc::new(
            NotificationService::load(
                NotificationStore::with_path(path.clone()),
                &["alpha".into()],
                1_784_606_400_000,
                emitter,
            )
            .unwrap(),
        );
        let observer = OrderNotificationObserver::new(Arc::new(NotificationRuntime::Available(
            Arc::clone(&service),
        )));
        let lifecycle = Arc::new(AccountLifecycleCoordinator::new());
        let mut app_config = AppConfig::default();
        app_config.active_account_id = "alpha".into();
        let config = Arc::new(tokio::sync::RwLock::new(app_config));
        let context = OrderStreamContext {
            account_id: "alpha".into(),
            session_epoch: lifecycle.current_session_epoch(),
        };
        let buffer = Arc::new(tokio::sync::Mutex::new(OrderObservationBuffer::default()));
        buffer.lock().await.reset(context.clone());
        assert!(buffer.lock().await.defer(
            &context,
            vec![
                buffered_order("order-manual-gate-1", OrderStatus::New),
                buffered_order("order-manual-gate-1", OrderStatus::Filled),
            ],
        ));
        let seeded = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let seed_buffer = Arc::clone(&buffer);
        let seed_flag = Arc::clone(&seeded);
        let seed_context = context.clone();
        let replay_context = context.clone();
        let seed_observer = observer.clone();
        let seed_task = tokio::spawn(async move {
            seed_order_gate(
                &seed_buffer,
                &seed_flag,
                &seed_context,
                |pending| async move {
                    let _ = started_tx.send(());
                    let _ = release_rx.await;
                    seed_observer
                        .seed_snapshot_then_replay(
                            &replay_context,
                            &[buffered_order("order-manual-gate-1", OrderStatus::Filled)],
                            pending,
                            1_784_606_400_000,
                        )
                        .await
                },
            )
            .await
        });
        started_rx.await.unwrap();

        let manual_config = Arc::clone(&config);
        let manual_lifecycle = Arc::clone(&lifecycle);
        let manual_buffer = Arc::clone(&buffer);
        let manual_observer = observer.clone();
        let manual_context = context.clone();
        let mut manual_task = tokio::spawn(async move {
            observe_manual_order_snapshots_for_session(
                &manual_config,
                &manual_lifecycle,
                &manual_buffer,
                &manual_observer,
                &manual_context,
                &[buffered_order("order-manual-gate-1", OrderStatus::Filled)],
                1_784_606_400_100,
            )
            .await;
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut manual_task)
                .await
                .is_err()
        );
        release_tx.send(()).unwrap();
        seed_task.await.unwrap().unwrap();
        manual_task.await.unwrap();

        let records = service
            .list(
                ViewContext::account("alpha").unwrap(),
                ListNotificationsRequest {
                    account_id: Some("alpha".into()),
                    filter: NotificationFilter::All,
                    cursor: None,
                    limit: 100,
                },
                1_784_606_401_000,
            )
            .await
            .unwrap()
            .items;
        assert_eq!(records.len(), 1);

        lifecycle.advance_session_epoch();
        observe_manual_order_snapshots_for_session(
            &config,
            &lifecycle,
            &buffer,
            &observer,
            &context,
            &[buffered_order("order-stale-manual-1", OrderStatus::New)],
            1_784_606_402_000,
        )
        .await;
        assert!(observer
            .observe(
                "alpha",
                context.session_epoch,
                &buffered_order("order-stale-manual-1", OrderStatus::Filled),
                crate::services::notification::OrderObservationOrigin::Realtime,
                None,
                1_784_606_402_001,
            )
            .await
            .is_none());
        assert_eq!(service.revision().await, "1");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    struct FailSecondSink {
        sends: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Sink<Message> for FailSecondSink {
        type Error = &'static str;

        fn poll_ready(
            self: std::pin::Pin<&mut Self>,
            _context: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn start_send(self: std::pin::Pin<&mut Self>, _item: Message) -> Result<(), Self::Error> {
            let attempt = self.sends.fetch_add(1, Ordering::SeqCst);
            if attempt == 1 {
                Err("raw subscribe transport detail")
            } else {
                Ok(())
            }
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _context: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            _context: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    struct TouchOnDrop(Arc<WsFreshness>, FreshnessDomain);

    impl Drop for TouchOnDrop {
        fn drop(&mut self) {
            self.0.touch(self.1, 99);
        }
    }

    struct SetTrueOnDrop(Arc<AtomicBool>);

    impl Drop for SetTrueOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn abort_wait_helper_drops_both_task_futures_before_returning() {
        let public_dropped = Arc::new(AtomicBool::new(false));
        let private_dropped = Arc::new(AtomicBool::new(false));
        let (public_started_tx, public_started_rx) = tokio::sync::oneshot::channel();
        let (private_started_tx, private_started_rx) = tokio::sync::oneshot::channel();

        let public_flag = public_dropped.clone();
        let public_task = tauri::async_runtime::spawn(async move {
            let _drop_flag = DropFlag(public_flag);
            public_started_tx.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        let private_flag = private_dropped.clone();
        let private_task = tauri::async_runtime::spawn(async move {
            let _drop_flag = DropFlag(private_flag);
            private_started_tx.send(()).unwrap();
            std::future::pending::<()>().await;
        });

        public_started_rx.await.unwrap();
        private_started_rx.await.unwrap();
        abort_and_wait_for_tasks(Some(public_task), Some(private_task)).await;

        assert!(public_dropped.load(Ordering::SeqCst));
        assert!(private_dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn stop_order_resets_freshness_after_aborted_tasks_finish_dropping() {
        let freshness = Arc::new(WsFreshness::default());
        let public_connected = Arc::new(AtomicBool::new(false));
        let private_connected = Arc::new(AtomicBool::new(false));
        freshness.touch(FreshnessDomain::Ticker, 41);
        freshness.touch(FreshnessDomain::Balance, 42);
        let (public_started_tx, public_started_rx) = tokio::sync::oneshot::channel();
        let (private_started_tx, private_started_rx) = tokio::sync::oneshot::channel();

        let public_freshness = freshness.clone();
        let public_connected_on_drop = public_connected.clone();
        let public_task = tauri::async_runtime::spawn(async move {
            let _touch_on_drop = TouchOnDrop(public_freshness, FreshnessDomain::Depth);
            let _set_connected_on_drop = SetTrueOnDrop(public_connected_on_drop);
            public_started_tx.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        let private_freshness = freshness.clone();
        let private_connected_on_drop = private_connected.clone();
        let private_task = tauri::async_runtime::spawn(async move {
            let _touch_on_drop = TouchOnDrop(private_freshness, FreshnessDomain::Position);
            let _set_connected_on_drop = SetTrueOnDrop(private_connected_on_drop);
            private_started_tx.send(()).unwrap();
            std::future::pending::<()>().await;
        });

        public_started_rx.await.unwrap();
        private_started_rx.await.unwrap();
        abort_wait_and_reset_freshness(
            Some(public_task),
            Some(private_task),
            &public_connected,
            &private_connected,
            &freshness,
        )
        .await;

        assert!(!public_connected.load(Ordering::SeqCst));
        assert!(!private_connected.load(Ordering::SeqCst));

        for domain in [
            FreshnessDomain::Ticker,
            FreshnessDomain::Depth,
            FreshnessDomain::Candle,
            FreshnessDomain::Balance,
            FreshnessDomain::Order,
            FreshnessDomain::Position,
        ] {
            assert_eq!(freshness.timestamp(domain).load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    fn reset_channel_freshness_clears_every_previous_session_domain() {
        let freshness = WsFreshness::default();
        for domain in [
            FreshnessDomain::Ticker,
            FreshnessDomain::Depth,
            FreshnessDomain::Candle,
            FreshnessDomain::Balance,
            FreshnessDomain::Order,
            FreshnessDomain::Position,
        ] {
            freshness.touch(domain, 1_700_000_000_000);
        }

        freshness.reset();

        for domain in [
            FreshnessDomain::Ticker,
            FreshnessDomain::Depth,
            FreshnessDomain::Candle,
            FreshnessDomain::Balance,
            FreshnessDomain::Order,
            FreshnessDomain::Position,
        ] {
            assert_eq!(freshness.timestamp(domain).load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    fn new_session_is_not_fresh_until_that_session_touches_the_channel() {
        let now = 1_700_000_000_000;
        let freshness = WsFreshness::default();
        freshness.touch(FreshnessDomain::Ticker, now - 1);
        freshness.touch(FreshnessDomain::Balance, now - 1);

        freshness.reset();

        assert!(!freshness.is_fresh_at(FreshnessDomain::Ticker, now, 5_000));
        assert!(!freshness.is_fresh_at(FreshnessDomain::Balance, now, 5_000));

        freshness.touch(FreshnessDomain::Balance, now);
        assert!(freshness.is_balance_fresh_at(now, 5_000));
    }

    #[test]
    fn private_auth_ack_requires_an_explicit_compatible_success() {
        assert_eq!(PRIVATE_AUTH_ERROR, "私有 WebSocket 鉴权失败");
        assert_eq!(PRIVATE_SUBSCRIPTION_ERROR, "私有 WebSocket 订阅失败");
        assert_eq!(
            validate_private_auth_ack(&json!({"op": "auth", "success": true})),
            Ok(())
        );
    }

    #[test]
    fn private_auth_ack_rejects_conflicts_missing_evidence_and_unknown_frames() {
        for response in [
            json!({"op": "auth"}),
            json!({"op": "auth", "success": false}),
            json!({"op": "auth", "success": true, "code": 401}),
            json!({"op": "auth", "success": false, "code": 0}),
            json!({"op": "auth", "request": {"op": "subscribe"}, "success": true}),
            json!({"op": "subscribe", "request": {"op": "auth"}, "success": true}),
            json!({"op": 1, "request": {"op": "auth"}, "success": true}),
            json!({"op": "auth", "request": {"op": 1}, "success": true}),
            json!({"op": "subscribe", "success": true}),
            json!({"success": true}),
            json!({"op": "auth", "success": "true"}),
            json!({"request": {"op": "auth"}, "code": 0}),
            json!({"op": "auth", "retCode": "0"}),
            json!({"op": "auth", "ret_code": 200}),
            json!({"op": "auth", "code": "SUCCESS"}),
        ] {
            let error = validate_private_auth_ack(&response).unwrap_err();
            assert_eq!(error, PRIVATE_AUTH_ERROR);
            assert!(!error.contains("401"));
        }
    }

    #[test]
    fn private_websocket_auth_spoof_messages_remain_one_sanitized_protocol_error() {
        for response in [
            json!({"op": "auth", "success": false, "message": "session expired"}),
            json!({"op": "auth", "success": false, "code": 26200003}),
            json!({"op": "auth", "success": false, "message": "apiKey=raw-secret"}),
        ] {
            let error = validate_private_auth_ack(&response).unwrap_err();
            assert_eq!(error, PRIVATE_AUTH_ERROR);
            assert!(!error.contains("expired"));
            assert!(!error.contains("26200003"));
            assert!(!error.contains("raw-secret"));
        }
    }

    #[tokio::test]
    async fn private_auth_wait_rejects_non_json_closed_and_timeout_with_one_safe_error() {
        let mut non_json = stream::iter([Ok(Message::Text("apiKey=raw-key".into()))]);
        let mut non_auth = stream::iter([Ok(Message::Text(
            json!({"topic": "contract.wallet", "data": {"coin": "USDT"}})
                .to_string()
                .into(),
        ))]);
        let mut closed = stream::empty();
        let mut pending = stream::pending();

        for result in [
            await_private_auth_response(&mut non_json, Duration::from_millis(10)).await,
            await_private_auth_response(&mut non_auth, Duration::from_millis(10)).await,
            await_private_auth_response(&mut closed, Duration::from_millis(10)).await,
            await_private_auth_response(&mut pending, Duration::from_millis(1)).await,
        ] {
            let error = result.unwrap_err();
            assert_eq!(error, PRIVATE_AUTH_ERROR);
            assert!(!error.contains("raw-key"));
            assert!(!error.contains("apiKey"));
        }
    }

    #[tokio::test]
    async fn subscribe_failure_does_not_mark_private_session_connected() {
        let sends = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut write = FailSecondSink {
            sends: sends.clone(),
        };
        let mut read = stream::iter([Ok(Message::Text(
            json!({"op": "auth", "success": true}).to_string().into(),
        ))]);
        let connected = AtomicBool::new(false);
        let running = AtomicBool::new(true);
        let emitted_connected = AtomicBool::new(false);

        let result = authenticate_and_subscribe(
            &mut write,
            &mut read,
            &json!({"op": "auth", "args": ["key", 1, "signature"]}),
            &["contract.wallet".to_string()],
            &running,
            &connected,
            || emitted_connected.store(true, Ordering::SeqCst),
            Duration::from_millis(10),
        )
        .await;

        assert_eq!(result.unwrap_err(), PRIVATE_SUBSCRIPTION_ERROR);
        assert_eq!(sends.load(Ordering::SeqCst), 2);
        assert!(!connected.load(Ordering::SeqCst));
        assert!(!emitted_connected.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn stopped_private_session_does_not_mark_connected_after_subscribe() {
        let mut write = futures_util::sink::drain();
        let mut read = stream::iter([Ok(Message::Text(
            json!({"op": "auth", "success": true}).to_string().into(),
        ))]);
        let running = AtomicBool::new(false);
        let connected = AtomicBool::new(false);
        let emitted_connected = AtomicBool::new(false);

        let result = authenticate_and_subscribe(
            &mut write,
            &mut read,
            &json!({"op": "auth", "args": ["key", 1, "signature"]}),
            &["contract.wallet".to_string()],
            &running,
            &connected,
            || emitted_connected.store(true, Ordering::SeqCst),
            Duration::from_millis(10),
        )
        .await;

        assert_eq!(result, Ok(()));
        assert!(!connected.load(Ordering::SeqCst));
        assert!(!emitted_connected.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn private_loop_error_after_manual_stop_has_no_failure_side_effect() {
        let running = AtomicBool::new(false);
        let reports = AtomicUsize::new(0);

        let reported = report_private_failure_if_running(&running, || async {
            reports.fetch_add(1, Ordering::SeqCst);
        })
        .await;

        assert!(!reported);
        assert_eq!(reports.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn control_and_unknown_frames_do_not_update_any_freshness_domain() {
        let freshness = WsFreshness::default();
        let now = 1_700_000_000_000;
        for message in [
            json!({"op": "pong", "topic": "tickers-100.BTCUSDT", "data": {}}),
            json!({"request": {"op": "subscribe"}, "topic": "contract.wallet", "data": {}}),
            json!({"op": "auth", "success": true}),
            json!({"topic": "contract.unknown", "data": {}}),
        ] {
            record_message_freshness(&message, &freshness, now);
        }

        assert!(!freshness.is_market_fresh_at(now, 5_000));
        assert!(!freshness.is_balance_fresh_at(now, 5_000));
        assert!(!freshness.is_private_panels_fresh_at(now, 5_000));
    }

    #[test]
    fn wallet_freshness_does_not_make_private_panels_healthy() {
        let freshness = WsFreshness::default();
        let now = 1_700_000_000_000;
        record_message_freshness(
            &json!({"topic": "contract.wallet", "data": {"coin": "USDT"}}),
            &freshness,
            now,
        );

        assert!(freshness.is_balance_fresh_at(now, 5_000));
        assert!(!freshness.is_private_panels_fresh_at(now, 5_000));
    }

    #[test]
    fn order_and_position_freshness_do_not_make_balance_healthy() {
        let freshness = WsFreshness::default();
        let now = 1_700_000_000_000;
        for topic in ["contract.order", "contract.position"] {
            record_message_freshness(
                &json!({"topic": topic, "data": {"symbol": "BTCUSDT"}}),
                &freshness,
                now,
            );
        }

        assert!(freshness.is_private_panels_fresh_at(now, 5_000));
        assert!(!freshness.is_balance_fresh_at(now, 5_000));
    }

    #[test]
    fn every_required_public_topic_must_be_fresh_for_market_health() {
        let freshness = WsFreshness::default();
        let now = 1_700_000_000_000;
        record_message_freshness(
            &json!({"topic": "tickers-100.BTCUSDT", "data": {"symbol": "BTCUSDT"}}),
            &freshness,
            now,
        );
        assert!(!freshness.is_market_fresh_at(now, 5_000));

        record_message_freshness(
            &json!({"topic": "ob_snap_shot.BTCUSDT.1", "data": {"bids": []}}),
            &freshness,
            now,
        );
        assert!(!freshness.is_market_fresh_at(now, 5_000));

        record_message_freshness(
            &json!({"topic": "candle.1.BTCUSDT", "data": []}),
            &freshness,
            now,
        );
        assert!(freshness.is_market_fresh_at(now, 5_000));
    }

    #[test]
    fn execution_does_not_impersonate_order_or_position_freshness() {
        let freshness = WsFreshness::default();
        let now = 1_700_000_000_000;
        record_message_freshness(
            &json!({"topic": "contract.execution", "data": {"symbol": "BTCUSDT"}}),
            &freshness,
            now,
        );

        assert!(!freshness.is_private_panels_fresh_at(now, 5_000));
    }

    #[test]
    fn recognised_topic_without_data_envelope_keeps_top_level_payload_compatibility() {
        let freshness = WsFreshness::default();
        let now = 1_700_000_000_000;
        record_message_freshness(
            &json!({"topic": "contract.wallet", "walletBalance": "1"}),
            &freshness,
            now,
        );

        assert!(freshness.is_balance_fresh_at(now, 5_000));
    }

    #[test]
    fn public_disconnect_clears_only_public_freshness_domains() {
        let freshness = WsFreshness::default();
        let now = 1_700_000_000_000;
        for domain in [
            FreshnessDomain::Ticker,
            FreshnessDomain::Depth,
            FreshnessDomain::Candle,
            FreshnessDomain::Balance,
            FreshnessDomain::Order,
            FreshnessDomain::Position,
        ] {
            freshness.touch(domain, now);
        }

        freshness.reset_public();

        assert!(!freshness.is_market_fresh_at(now, 5_000));
        assert!(freshness.is_balance_fresh_at(now, 5_000));
        assert!(freshness.is_private_panels_fresh_at(now, 5_000));
    }

    #[test]
    fn private_disconnect_clears_only_private_freshness_domains() {
        let freshness = WsFreshness::default();
        let now = 1_700_000_000_000;
        for domain in [
            FreshnessDomain::Ticker,
            FreshnessDomain::Depth,
            FreshnessDomain::Candle,
            FreshnessDomain::Balance,
            FreshnessDomain::Order,
            FreshnessDomain::Position,
        ] {
            freshness.touch(domain, now);
        }

        freshness.reset_private();

        assert!(freshness.is_market_fresh_at(now, 5_000));
        assert!(!freshness.is_balance_fresh_at(now, 5_000));
        assert!(!freshness.is_private_panels_fresh_at(now, 5_000));
    }
}
