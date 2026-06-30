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
    ) -> AppResult<()> {
        self.set_status(ConnectionStatus::Connecting).await;
        let credential = CredentialStore::load(account_id)?
            .ok_or_else(|| AppError::Auth("未找到 API 凭据".into()))?;
        self.api.set_credential(credential.clone()).await;

        sync_from_server(
            self.api.time_sync().as_ref(),
            PublicApi::server_time(&self.api),
        )
        .await?;

        if start_realtime {
            self.ws
                .configure(
                    &credential.base_url,
                    Signer::new(credential.api_key.clone(), credential.api_secret.clone()),
                )
                .await;
            self.ws.subscribe_all(symbol).await;
            self.ws.start(symbol).await?;
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
        self.ws.subscribe_all(symbol).await;
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
        let ws = self.ws.clone();

        tauri::async_runtime::spawn(async move {
            loop {
                let (use_ws, interval_secs) = {
                    let cfg = config.read().await;
                    (cfg.use_websocket, cfg.ticker_poll_interval.max(1.0))
                };

                if use_ws && ws.is_connected() {
                    tokio::select! {
                        _ = shutdown_rx.recv() => return,
                        _ = tokio::time::sleep(std::time::Duration::from_secs_f64(interval_secs)) => continue,
                    }
                }

                tokio::select! {
                    _ = shutdown_rx.recv() => return,
                    _ = tokio::time::sleep(std::time::Duration::from_secs_f64(interval_secs)) => {}
                }

                let symbol = market.active_symbol().await;
                if let Err(e) = market.refresh_snapshot(&symbol).await {
                    tracing::warn!("Market poll failed: {}", e);
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
