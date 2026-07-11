use std::sync::Arc;



use crate::api::{ApiClient, PublicApi};

use crate::auth::Signer;

use crate::error::{AppError, AppResult};

use crate::events::EventEmitter;

use crate::models::config::{ApiCredential, AppConfig, ConnectionStatus};
use crate::models::time::TimeSyncStatus;

use crate::services::{MarketService, TimeService};

use crate::storage::CredentialStore;

use crate::ws::WsManager;



pub struct ConnectionService {

    api: Arc<ApiClient>,

    ws: Arc<WsManager>,

    market: Arc<MarketService>,

    config: Arc<tokio::sync::RwLock<AppConfig>>,

    emitter: EventEmitter,

    time: Arc<TimeService>,

    status: Arc<tokio::sync::RwLock<ConnectionStatus>>,

}



impl ConnectionService {

    pub fn new(

        api: Arc<ApiClient>,

        ws: Arc<WsManager>,

        market: Arc<MarketService>,

        config: Arc<tokio::sync::RwLock<AppConfig>>,

        emitter: EventEmitter,

        time: Arc<TimeService>,

    ) -> Self {

        Self {

            api,

            ws,

            market,

            config,

            emitter,

            time,

            status: Arc::new(tokio::sync::RwLock::new(ConnectionStatus::Disconnected)),

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

            Some(mut c) => {

                c = c.normalize();

                if !c.has_secret() {

                    let stored = CredentialStore::load(account_id)?

                        .ok_or_else(|| AppError::Auth("未找到 API 凭据".into()))?;

                    c.api_secret = stored.api_secret;

                    if c.api_key.is_empty() {

                        c.api_key = stored.api_key;

                    }

                    if c.base_url.is_empty() {

                        c.base_url = stored.base_url;

                    }

                }

                if !c.is_valid() {

                    return Err(AppError::Auth(

                        "API 凭据无效，请在设置中重新保存 API Key 与 Secret".into(),

                    ));

                }

                c

            }

            None => CredentialStore::load(account_id)?

                .ok_or_else(|| AppError::Auth("未找到 API 凭据".into()))?

                .normalize(),

        };



        if !credential.is_valid() {

            return Err(AppError::Auth(

                "API Secret 缺失，请在设置中重新保存完整凭据".into(),

            ));

        }



        self.api.set_credential(credential.clone()).await;

        let snapshot = self.time.sync().await?;
        if snapshot.sync_status == TimeSyncStatus::Failed {
            return Err(AppError::Connection(
                snapshot
                    .last_error
                    .unwrap_or_else(|| "无法同步服务器时间".into()),
            ));
        }

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

        Ok(())

    }



    pub async fn disconnect(&self) {

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

        self.ws.start(symbol).await?;

        Ok(())

    }



    pub async fn test_connection(&self, credential: &ApiCredential) -> AppResult<()> {

        let temp = Arc::new(ApiClient::new());
        temp.set_credential(credential.clone()).await;
        let time = TimeService::new(temp.time_sync(), temp.clone(), self.emitter.clone());

        time.sync().await?;

        PublicApi::ticker(&temp, "BTCUSDT").await?;

        Ok(())

    }

}


