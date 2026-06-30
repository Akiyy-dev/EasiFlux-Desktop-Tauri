use std::sync::Arc;

use tokio::sync::RwLock;

use crate::api::{ApiClient, PrivateApi, PublicApi};
use crate::auth::time_sync::sync_from_server;
use crate::auth::Signer;
use crate::error::{AppError, AppResult};
use crate::events::EventEmitter;
use crate::models::config::{ApiCredential, ConnectionStatus};
use crate::storage::CredentialStore;
use crate::ws::WsManager;

pub struct ConnectionService {
    api: Arc<ApiClient>,
    ws: Arc<WsManager>,
    emitter: EventEmitter,
    status: Arc<RwLock<ConnectionStatus>>,
}

impl ConnectionService {
    pub fn new(api: Arc<ApiClient>, ws: Arc<WsManager>, emitter: EventEmitter) -> Self {
        Self {
            api,
            ws,
            emitter,
            status: Arc::new(RwLock::new(ConnectionStatus::Disconnected)),
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

        let fetch = || async {
            PublicApi::server_time(&self.api).await
        };
        sync_from_server(self.api.time_sync().as_ref(), fetch()).await?;

        if start_realtime {
            self.ws
                .configure(&credential.base_url, Signer::new(credential.api_key.clone(), credential.api_secret.clone()))
                .await;
            self.ws.subscribe_all(symbol).await;
            self.ws.start(symbol).await?;
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
}
