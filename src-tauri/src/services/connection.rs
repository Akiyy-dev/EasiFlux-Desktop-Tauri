use std::sync::Arc;

use tokio::sync::{mpsc, Mutex, RwLock};

use crate::api::{ApiClient, PublicApi};
use crate::auth::time_sync::sync_from_server;
use crate::auth::Signer;
use crate::error::{AppError, AppResult};
use crate::events::EventEmitter;
use crate::models::config::{ApiCredential, AppConfig, ConnectionStatus};
use crate::services::MarketService;
use crate::storage::CredentialStore;
use crate::ws::WsManager;

pub struct ConnectionService {
    api: Arc<ApiClient>,
    ws: Arc<WsManager>,
    market: Arc<MarketService>,
    config: Arc<RwLock<AppConfig>>,
    emitter: EventEmitter,
    status: Arc<RwLock<ConnectionStatus>>,
    poll_shutdown: Arc<Mutex<Option<mpsc::Sender<()>>>>,
}

impl ConnectionService {
    pub fn new(
        api: Arc<ApiClient>,
        ws: Arc<WsManager>,
        market: Arc<MarketService>,
        config: Arc<RwLock<AppConfig>>,
        emitter: EventEmitter,
    ) -> Self {
        Self {
            api,
            ws,
            market,
            config,
            emitter,
            status: Arc::new(RwLock::new(ConnectionStatus::Disconnected)),
            poll_shutdown: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn status(&self) -> ConnectionStatus {
        *self.status.read().await
    }

    async fn set_status(&self, status: ConnectionStatus) {
        *self.status.write().await = status;
        self.emitter
            .emit_connection(&format!("{:?}", status).to_lowercase());
    }

    pub async fn connect(
        &self,
        account_id: &str,
        start_realtime: bool,
        symbol: &str,
        credential: Option<ApiCredential>,
    ) -> AppResult<()> {
        self.set_status(ConnectionStatus::Connecting).await;

        match self
            .connect_inner(account_id, start_realtime, symbol, credential)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) => {
                let msg = e.user_message();
                self.set_status(ConnectionStatus::Error).await;
                self.emitter.emit_error(&msg);
                Err(e)
            }
        }
    }

    async fn connect_inner(
        &self,
        account_id: &str,
        start_realtime: bool,
        symbol: &str,
        credential: Option<ApiCredential>,
    ) -> AppResult<()> {
        let credential = match credential {
            Some(c) => c,
            None => CredentialStore::load(account_id)?
                .ok_or_else(|| AppError::Auth("未找到 API 凭据".into()))?,
        };

        self.api.set_credential(credential.clone()).await;
        sync_from_server(
            self.api.time_sync().as_ref(),
            PublicApi::server_time(&self.api),
        )
        .await?;

        if start_realtime {
            let (ws_public, ws_private) = {
                let cfg = self.config.read().await;
                (cfg.ws_public_url.clone(), cfg.ws_private_url.clone())
            };
            self.ws
                .configure(
                    &ws_public,
                    &ws_private,
                    Signer::new(credential.api_key.clone(), credential.api_secret.clone()),
                )
                .await;
            let kline_interval = self.market.kline_interval().await;
            self.ws.subscribe_all(symbol, &kline_interval).await;
            if let Err(e) = self.ws.start(symbol).await {
                self.emitter
                    .emit_error(&format!("WebSocket 启动失败: {}", e));
                self.emitter.emit_websocket("error");
            }
        }

        self.set_status(ConnectionStatus::Connected).await;
        self.emitter.emit_log("info", "API 连接成功");
        self.start_market_poll().await;
        Ok(())
    }

    pub async fn disconnect(&self) {
        self.stop_market_poll().await;
        self.ws.stop().await;
        self.api.clear_credential().await;
        self.set_status(ConnectionStatus::Disconnected).await;
    }

    pub async fn refresh_realtime(&self, symbol: &str) -> AppResult<()> {
        let use_ws = self.config.read().await.use_websocket;
        if !use_ws || self.status().await != ConnectionStatus::Connected {
            return Ok(());
        }
        let kline_interval = self.market.kline_interval().await;
        self.ws.subscribe_all(symbol, &kline_interval).await;
        self.ws.start(symbol).await
    }

    pub async fn test_connection(&self, credential: &ApiCredential) -> AppResult<()> {
        let temp = ApiClient::new();
        temp.set_credential(credential.clone()).await;
        sync_from_server(
            temp.time_sync().as_ref(),
            PublicApi::server_time(&temp),
        )
        .await?;
        PublicApi::ticker(&temp, "BTCUSDT").await?;
        Ok(())
    }

    async fn start_market_poll(&self) {
        self.stop_market_poll().await;
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        *self.poll_shutdown.lock().await = Some(shutdown_tx);

        let market = self.market.clone();
        let config = self.config.clone();
        let emitter = self.emitter.clone();

        tauri::async_runtime::spawn(async move {
            const KLINE_POLL_EVERY_N: u64 = 5;
            let mut poll_tick: u64 = 0;

            loop {
                let interval_secs = {
                    let cfg = config.read().await;
                    cfg.ticker_poll_interval.max(1.0)
                };

                tokio::select! {
                    _ = shutdown_rx.recv() => return,
                    _ = tokio::time::sleep(std::time::Duration::from_secs_f64(interval_secs)) => {}
                }

                poll_tick += 1;
                let symbol = market.active_symbol().await;

                if let Err(e) = market.refresh_ticker_depth(&symbol).await {
                    emitter.emit_error(&format!("行情轮询失败: {}", e));
                }

                if poll_tick % KLINE_POLL_EVERY_N == 0 {
                    if let Err(e) = market.refresh_klines(&symbol).await {
                        emitter.emit_error(&format!("K线轮询失败: {}", e));
                    }
                }
            }
        });
    }

    async fn stop_market_poll(&self) {
        if let Some(tx) = self.poll_shutdown.lock().await.take() {
            let _ = tx.send(()).await;
        }
    }
}
