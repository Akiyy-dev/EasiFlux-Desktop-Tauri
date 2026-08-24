use std::sync::Arc;

use crate::api::response::AuthFailureKind;
use crate::api::{ApiClient, PublicApi};

use crate::auth::Signer;

use crate::error::{AppError, AppResult};

use crate::events::EventEmitter;

use crate::models::config::{normalize_account_id, ApiCredential, AppConfig, ConnectionStatus};
use crate::models::notification::{NotificationChannel, NotificationEnvironment};
use crate::models::time::TimeSyncStatus;
use crate::models::trading::{OrderStreamContext, SessionContext};

use crate::services::notification::{
    AvailabilityState, ConnectionObservation, EnvironmentObservation, NotificationRuntime,
};
use crate::services::{AccountLifecycleCoordinator, MarketService, TimeService};

use crate::storage::CredentialStore;

use crate::ws::WsManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionObservationSource {
    Api,
    PrivateWebsocket,
    PublicWebsocket,
}

#[derive(Clone)]
pub(crate) struct SessionNotificationObserver {
    runtime: Arc<NotificationRuntime>,
    config: Arc<tokio::sync::RwLock<AppConfig>>,
    account_lifecycle: Arc<AccountLifecycleCoordinator>,
    emitter: EventEmitter,
}

impl SessionNotificationObserver {
    pub(crate) fn new(
        runtime: Arc<NotificationRuntime>,
        config: Arc<tokio::sync::RwLock<AppConfig>>,
        account_lifecycle: Arc<AccountLifecycleCoordinator>,
        emitter: EventEmitter,
    ) -> Self {
        Self {
            runtime,
            config,
            account_lifecycle,
            emitter,
        }
    }

    async fn is_current_guarded(&self, context: &SessionContext) -> bool {
        let active_account_id = normalize_account_id(&self.config.read().await.active_account_id);
        let current_epoch = self.account_lifecycle.current_session_epoch();
        active_account_id == context.account_id && current_epoch == context.session_epoch
    }

    pub(crate) async fn observe_connection_status(
        &self,
        context: &SessionContext,
        source: ConnectionObservationSource,
        status: ConnectionStatus,
        now_ms: u64,
    ) -> Option<String> {
        let _guard = self.account_lifecycle.read_guard().await;
        self.observe_connection_status_guarded(context, source, status, now_ms)
            .await
    }

    pub(crate) async fn observe_connection_status_guarded(
        &self,
        context: &SessionContext,
        source: ConnectionObservationSource,
        status: ConnectionStatus,
        now_ms: u64,
    ) -> Option<String> {
        let channel = match source {
            ConnectionObservationSource::Api => NotificationChannel::Api,
            ConnectionObservationSource::PrivateWebsocket => NotificationChannel::Websocket,
            ConnectionObservationSource::PublicWebsocket => return None,
        };
        let state = match status {
            ConnectionStatus::Connected => AvailabilityState::Available,
            ConnectionStatus::Error => AvailabilityState::Unavailable,
            ConnectionStatus::Connecting | ConnectionStatus::Disconnected => return None,
        };
        if !self.is_current_guarded(context).await {
            return None;
        }
        let service = match self.runtime.service() {
            Ok(service) => service,
            Err(availability) => {
                tracing::warn!(
                    code = availability.code(),
                    "connection notification observer unavailable"
                );
                return None;
            }
        };
        match service
            .observe_connection(
                ConnectionObservation {
                    account_id: context.account_id.clone(),
                    session_epoch: context.session_epoch,
                    channel,
                    state,
                },
                now_ms,
            )
            .await
        {
            Ok(outcome) => outcome.notification.map(|record| record.id),
            Err(error) => {
                tracing::warn!(
                    code = error.code(),
                    "connection notification observation failed"
                );
                None
            }
        }
    }

    pub(crate) async fn observe_environment(
        &self,
        context: &SessionContext,
        environment_key: &str,
        environment: NotificationEnvironment,
        reachable: bool,
        now_ms: u64,
    ) -> Option<String> {
        let _guard = self.account_lifecycle.read_guard().await;
        self.observe_environment_guarded(context, environment_key, environment, reachable, now_ms)
            .await
    }

    pub(crate) async fn observe_environment_guarded(
        &self,
        context: &SessionContext,
        environment_key: &str,
        environment: NotificationEnvironment,
        reachable: bool,
        now_ms: u64,
    ) -> Option<String> {
        if !self.is_current_guarded(context).await {
            return None;
        }
        let service = match self.runtime.service() {
            Ok(service) => service,
            Err(availability) => {
                tracing::warn!(
                    code = availability.code(),
                    "environment notification observer unavailable"
                );
                return None;
            }
        };
        match service
            .observe_environment(
                EnvironmentObservation {
                    account_id: context.account_id.clone(),
                    session_epoch: context.session_epoch,
                    environment_key: environment_key.to_string(),
                    environment,
                    state: if reachable {
                        AvailabilityState::Available
                    } else {
                        AvailabilityState::Unavailable
                    },
                },
                now_ms,
            )
            .await
        {
            Ok(outcome) => outcome.notification.map(|record| record.id),
            Err(error) => {
                tracing::warn!(
                    code = error.code(),
                    "environment notification observation failed"
                );
                None
            }
        }
    }

    pub(crate) async fn observe_auth_failure(
        &self,
        context: &SessionContext,
        failure: AuthFailureKind,
        now_ms: u64,
    ) -> Option<String> {
        let _guard = self.account_lifecycle.read_guard().await;
        self.observe_auth_failure_guarded(context, failure, now_ms)
            .await
    }

    pub(crate) async fn observe_auth_failure_guarded(
        &self,
        context: &SessionContext,
        failure: AuthFailureKind,
        now_ms: u64,
    ) -> Option<String> {
        if failure != AuthFailureKind::SessionExpired || !self.is_current_guarded(context).await {
            return None;
        }
        let service = match self.runtime.service() {
            Ok(service) => service,
            Err(availability) => {
                tracing::warn!(
                    code = availability.code(),
                    "session notification observer unavailable"
                );
                return None;
            }
        };
        match service
            .observe_session_expired(context.account_id.clone(), context.session_epoch, now_ms)
            .await
        {
            Ok(outcome) => outcome.notification.map(|record| {
                self.emitter
                    .emit_diagnostic("NOTIFIED_SESSION_FAILURE:AUTH_SESSION_EXPIRED", false);
                record.id
            }),
            Err(error) => {
                tracing::warn!(
                    code = error.code(),
                    "session notification observation failed"
                );
                None
            }
        }
    }
}

fn notified_connection_error(error: AppError, notification_id: Option<String>) -> AppError {
    match notification_id {
        Some(notification_id) => match error {
            AppError::AuthFailure(AuthFailureKind::SessionExpired) => AppError::Notified {
                code: "AUTH_SESSION_EXPIRED",
                message: "账户会话已失效",
                notification_id,
                cause: Some(crate::error::NotificationCause::AuthFailure(
                    AuthFailureKind::SessionExpired,
                )),
            },
            _ => AppError::Notified {
                code: "CONNECTION_UNAVAILABLE",
                message: "交易连接暂时不可用",
                notification_id,
                cause: None,
            },
        },
        None => error,
    }
}

fn deliver_notified_connection_error(emitter: &EventEmitter, error: AppError) -> AppError {
    if let AppError::Notified { code, .. } = &error {
        if *code != "AUTH_SESSION_EXPIRED" {
            emitter.emit_diagnostic(&format!("NOTIFIED_CONNECTION_FAILURE:{code}"), false);
        }
    }
    error
}

pub struct ConnectionService {
    api: Arc<ApiClient>,

    ws: Arc<WsManager>,

    market: Arc<MarketService>,

    config: Arc<tokio::sync::RwLock<AppConfig>>,

    emitter: EventEmitter,

    time: Arc<TimeService>,

    status: Arc<tokio::sync::RwLock<ConnectionStatus>>,

    account_lifecycle: Arc<AccountLifecycleCoordinator>,

    notification_observer: SessionNotificationObserver,
}

#[cfg(test)]
mod tests;

impl ConnectionService {
    pub fn new(
        api: Arc<ApiClient>,

        ws: Arc<WsManager>,

        market: Arc<MarketService>,

        config: Arc<tokio::sync::RwLock<AppConfig>>,

        emitter: EventEmitter,

        time: Arc<TimeService>,

        account_lifecycle: Arc<AccountLifecycleCoordinator>,

        notification_observer: SessionNotificationObserver,
    ) -> Self {
        Self {
            api,

            ws,

            market,

            config,

            emitter,

            time,

            status: Arc::new(tokio::sync::RwLock::new(ConnectionStatus::Disconnected)),

            account_lifecycle,

            notification_observer,
        }
    }

    pub async fn status(&self) -> ConnectionStatus {
        *self.status.read().await
    }

    async fn set_status(&self, context: &SessionContext, status: ConnectionStatus) {
        *self.status.write().await = status;

        self.emitter
            .emit_connection_for_session(context, &format!("{:?}", status).to_lowercase());
    }

    pub async fn connect(
        &self,

        account_id: &str,

        start_realtime: bool,

        symbol: &str,

        credential: Option<ApiCredential>,
    ) -> AppResult<()> {
        let context = SessionContext {
            account_id: normalize_account_id(account_id),
            session_epoch: self.account_lifecycle.current_session_epoch(),
        };
        self.connect_for_session(context, start_realtime, symbol, credential)
            .await
    }

    pub(crate) async fn connect_for_session(
        &self,
        context: SessionContext,
        start_realtime: bool,
        symbol: &str,
        credential: Option<ApiCredential>,
    ) -> AppResult<()> {
        self.set_status(&context, ConnectionStatus::Connecting)
            .await;

        match self
            .connect_inner(&context, start_realtime, symbol, credential)
            .await
        {
            Ok(()) => Ok(()),

            Err(e) => {
                self.set_status(&context, ConnectionStatus::Error).await;
                let notification_id = match &e {
                    AppError::Connection(_) => {
                        self.notification_observer
                            .observe_connection_status_guarded(
                                &context,
                                ConnectionObservationSource::Api,
                                ConnectionStatus::Error,
                                local_timestamp_ms(),
                            )
                            .await
                    }
                    _ => None,
                };
                let error = notified_connection_error(e, notification_id);
                Err(deliver_notified_connection_error(&self.emitter, error))
            }
        }
    }

    async fn connect_inner(
        &self,

        context: &SessionContext,

        start_realtime: bool,

        symbol: &str,

        credential: Option<ApiCredential>,
    ) -> AppResult<()> {
        if !start_realtime {
            self.ws.stop().await;
            self.emitter
                .emit_websocket_for_session(context, "disconnected");
        }

        let credential = match credential {
            Some(mut c) => {
                c = c.normalize();

                if !c.has_secret() {
                    let stored = CredentialStore::load(&context.account_id)?
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

            None => CredentialStore::load(&context.account_id)?
                .ok_or_else(|| AppError::Auth("未找到 API 凭据".into()))?
                .normalize(),
        };

        if !credential.is_valid() {
            return Err(AppError::Auth(
                "API Secret 缺失，请在设置中重新保存完整凭据".into(),
            ));
        }

        self.api
            .set_credential_for_session(credential.clone(), context.clone())
            .await;
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

            if let Err(e) = self.ws.start(symbol, context.clone()).await {
                self.emitter.emit_websocket_for_session(context, "error");
                tracing::warn!(error_kind = ?std::mem::discriminant(&e), "WebSocket startup failed");
            }
        }

        self.set_status(context, ConnectionStatus::Connected).await;
        self.notification_observer
            .observe_connection_status_guarded(
                context,
                ConnectionObservationSource::Api,
                ConnectionStatus::Connected,
                local_timestamp_ms(),
            )
            .await;

        self.emitter.emit_log("info", "API 连接成功");

        Ok(())
    }

    pub(crate) async fn activate_committed_session(&self, context: &SessionContext) {
        if self.status().await == ConnectionStatus::Connected {
            self.notification_observer
                .observe_connection_status_guarded(
                    context,
                    ConnectionObservationSource::Api,
                    ConnectionStatus::Connected,
                    local_timestamp_ms(),
                )
                .await;
        }
    }

    pub async fn disconnect(&self) {
        let context = SessionContext {
            account_id: normalize_account_id(&self.config.read().await.active_account_id),
            session_epoch: self.account_lifecycle.current_session_epoch(),
        };
        self.ws.stop().await;
        self.emitter
            .emit_websocket_for_session(&context, "disconnected");

        self.api.clear_credential().await;

        self.set_status(&context, ConnectionStatus::Disconnected)
            .await;
    }

    pub async fn refresh_realtime(&self, symbol: &str) -> AppResult<()> {
        let (use_ws, account_id) = {
            let config = self.config.read().await;
            (
                config.use_websocket,
                normalize_account_id(&config.active_account_id),
            )
        };
        let context = OrderStreamContext {
            account_id,
            session_epoch: self.account_lifecycle.current_session_epoch(),
        };

        if !use_ws || self.status().await != ConnectionStatus::Connected {
            return Ok(());
        }

        let kline_interval = self.market.kline_interval().await;

        self.ws.subscribe_all(symbol, &kline_interval).await;

        self.ws.start(symbol, context).await?;

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

fn local_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
