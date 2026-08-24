use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use tokio::sync::RwLock;

use crate::events::EventEmitter;
use crate::models::config::AppConfig;
use crate::models::notification::{
    ClientNotificationFailedStep, ClientNotificationKind, CreateClientNotificationRequest,
    ListNotificationsRequest, NotificationPage, NotificationRecord, NotificationScope,
    NotificationSummary,
};
use crate::services::notification::{
    NotificationAvailability, NotificationError, NotificationPolicy, NotificationRuntime,
    ViewContext,
};
use crate::services::AccountLifecycleCoordinator;
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationCommandError {
    pub code: &'static str,
    pub message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_id: Option<String>,
}

impl NotificationCommandError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            notification_id: None,
        }
    }
}

impl From<NotificationError> for NotificationCommandError {
    fn from(error: NotificationError) -> Self {
        Self::new(error.code(), error.message())
    }
}

impl From<NotificationAvailability> for NotificationCommandError {
    fn from(availability: NotificationAvailability) -> Self {
        Self::new(availability.code(), availability.message())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkNotificationReadResult {
    pub notification: NotificationRecord,
    pub unread_count: u64,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkVisibleNotificationsReadResult {
    pub affected_count: u64,
    pub affected_scopes: Vec<NotificationScope>,
    pub unread_count: u64,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteNotificationResult {
    pub unread_count: u64,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearAccountNotificationsResult {
    pub affected_count: u64,
    pub unread_count: u64,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateClientNotificationResult {
    pub notification: NotificationRecord,
    pub unread_count: u64,
    pub revision: String,
}

fn configured_account_exists(config: &AppConfig, requested: &str) -> bool {
    config
        .accounts
        .iter()
        .map(|account_id| account_id.trim())
        .any(|account_id| account_id == requested)
}

fn resolve_context(
    config: &AppConfig,
    requested_account_id: Option<&str>,
) -> Result<ViewContext, NotificationCommandError> {
    let Some(requested) = requested_account_id else {
        return Ok(ViewContext::global());
    };
    let active_account_id = crate::models::config::normalize_account_id(&config.active_account_id);
    if requested != active_account_id {
        return Err(NotificationCommandError::new(
            "NOTIFICATION_SCOPE_MISMATCH",
            "通知账户范围不匹配",
        ));
    }
    ViewContext::account(requested).map_err(Into::into)
}

async fn list_notifications_inner(
    runtime: &Arc<NotificationRuntime>,
    config: &Arc<RwLock<AppConfig>>,
    lifecycle: &AccountLifecycleCoordinator,
    request: ListNotificationsRequest,
    now_ms: u64,
) -> Result<NotificationPage, NotificationCommandError> {
    let _guard = lifecycle.read_guard().await;
    let config = config.read().await;
    let context = resolve_context(&config, request.account_id.as_deref())?;
    drop(config);
    let service = runtime.service()?;
    service
        .list(context, request, now_ms)
        .await
        .map_err(Into::into)
}

async fn get_notification_summary_inner(
    runtime: &Arc<NotificationRuntime>,
    config: &Arc<RwLock<AppConfig>>,
    lifecycle: &AccountLifecycleCoordinator,
    account_id: Option<String>,
    now_ms: u64,
) -> Result<NotificationSummary, NotificationCommandError> {
    let _guard = lifecycle.read_guard().await;
    let config = config.read().await;
    let context = resolve_context(&config, account_id.as_deref())?;
    drop(config);
    runtime
        .service()?
        .summary(context, now_ms)
        .await
        .map_err(Into::into)
}

async fn mark_notification_read_inner(
    runtime: &Arc<NotificationRuntime>,
    config: &Arc<RwLock<AppConfig>>,
    lifecycle: &AccountLifecycleCoordinator,
    context_account_id: Option<String>,
    id: String,
    now_ms: u64,
) -> Result<MarkNotificationReadResult, NotificationCommandError> {
    let _guard = lifecycle.read_guard().await;
    let config = config.read().await;
    let context = resolve_context(&config, context_account_id.as_deref())?;
    drop(config);
    let result = runtime.service()?.mark_read(context, &id, now_ms).await?;
    let notification = result.notification.ok_or_else(|| {
        NotificationCommandError::new("NOTIFICATION_STATE_CONFLICT", "通知状态发生冲突")
    })?;
    Ok(MarkNotificationReadResult {
        notification,
        unread_count: result.unread_count,
        revision: result.revision,
    })
}

async fn mark_visible_notifications_read_inner(
    runtime: &Arc<NotificationRuntime>,
    config: &Arc<RwLock<AppConfig>>,
    lifecycle: &AccountLifecycleCoordinator,
    context_account_id: Option<String>,
    now_ms: u64,
) -> Result<MarkVisibleNotificationsReadResult, NotificationCommandError> {
    let _guard = lifecycle.read_guard().await;
    let config = config.read().await;
    let context = resolve_context(&config, context_account_id.as_deref())?;
    drop(config);
    let result = runtime
        .service()?
        .mark_visible_read(context, now_ms)
        .await?;
    Ok(MarkVisibleNotificationsReadResult {
        affected_count: result.affected_count,
        affected_scopes: result.affected_scopes,
        unread_count: result.unread_count,
        revision: result.revision,
    })
}

async fn delete_notification_inner(
    runtime: &Arc<NotificationRuntime>,
    config: &Arc<RwLock<AppConfig>>,
    lifecycle: &AccountLifecycleCoordinator,
    context_account_id: Option<String>,
    id: String,
    now_ms: u64,
) -> Result<DeleteNotificationResult, NotificationCommandError> {
    let _guard = lifecycle.read_guard().await;
    let config = config.read().await;
    let context = resolve_context(&config, context_account_id.as_deref())?;
    drop(config);
    let result = runtime
        .service()?
        .delete_visible(context, &id, now_ms)
        .await?;
    Ok(DeleteNotificationResult {
        unread_count: result.unread_count,
        revision: result.revision,
    })
}

async fn clear_account_notifications_inner(
    runtime: &Arc<NotificationRuntime>,
    config: &Arc<RwLock<AppConfig>>,
    lifecycle: &AccountLifecycleCoordinator,
    account_id: String,
    now_ms: u64,
) -> Result<ClearAccountNotificationsResult, NotificationCommandError> {
    let _guard = lifecycle.read_guard().await;
    let config = config.read().await;
    if !configured_account_exists(&config, &account_id) {
        return Err(NotificationCommandError::new(
            "NOTIFICATION_ACCOUNT_NOT_FOUND",
            "通知账户不存在",
        ));
    }
    let _ = resolve_context(&config, Some(&account_id))?;
    let active_account_id = crate::models::config::normalize_account_id(&config.active_account_id);
    drop(config);
    let result = runtime
        .service()?
        .clear_account(&active_account_id, &account_id, now_ms)
        .await?;
    Ok(ClearAccountNotificationsResult {
        affected_count: result.affected_count,
        unread_count: result.unread_count,
        revision: result.revision,
    })
}

fn validate_client_notification_envelope(
    body: &tauri::ipc::InvokeBody,
) -> Result<(), NotificationCommandError> {
    let tauri::ipc::InvokeBody::Json(serde_json::Value::Object(args)) = body else {
        return Err(NotificationCommandError::new(
            "INVALID_NOTIFICATION_REQUEST",
            "客户端通知请求无效",
        ));
    };
    if args.len() != 1 || !args.contains_key("request") {
        return Err(NotificationCommandError::new(
            "INVALID_NOTIFICATION_REQUEST",
            "客户端通知请求无效",
        ));
    }
    Ok(())
}

async fn create_client_notification_inner(
    runtime: &Arc<NotificationRuntime>,
    config: &Arc<RwLock<AppConfig>>,
    lifecycle: &AccountLifecycleCoordinator,
    request: CreateClientNotificationRequest,
    now_ms: u64,
) -> Result<CreateClientNotificationResult, NotificationCommandError> {
    request
        .validate()
        .map_err(|error| NotificationCommandError::new(error.code(), "客户端通知请求无效"))?;
    let canonical_request = crate::models::config::normalize_account_id(&request.account_id);
    if canonical_request != request.account_id {
        return Err(NotificationCommandError::new(
            "INVALID_NOTIFICATION_REQUEST",
            "客户端通知请求无效",
        ));
    }

    let _guard = lifecycle.read_guard().await;
    let active_account_id = {
        let config = config.read().await;
        crate::models::config::normalize_account_id(&config.active_account_id)
    };
    if request.account_id != active_account_id {
        return Err(NotificationCommandError::new(
            "NOTIFICATION_SCOPE_MISMATCH",
            "通知账户范围不匹配",
        ));
    }
    if request.session_epoch != lifecycle.current_session_epoch() {
        return Err(NotificationCommandError::new(
            "NOTIFICATION_SESSION_MISMATCH",
            "通知会话代次不匹配",
        ));
    }

    let input = NotificationPolicy::default().client_account_failure(request)?;
    let result = runtime
        .service()?
        .publish_client_account_failure(input, now_ms)
        .await?;
    Ok(CreateClientNotificationResult {
        notification: result.notification,
        unread_count: result.unread_count,
        revision: result.revision,
    })
}

fn client_failure_diagnostic(request: &CreateClientNotificationRequest) -> String {
    let kind = match request.kind {
        ClientNotificationKind::AccountRecoveryFailed => "accountRecoveryFailed",
        ClientNotificationKind::AccountReconciliationFailed => "accountReconciliationFailed",
    };
    let steps = [
        (ClientNotificationFailedStep::Config, "config"),
        (ClientNotificationFailedStep::Profiles, "profiles"),
        (ClientNotificationFailedStep::Connection, "connection"),
        (ClientNotificationFailedStep::Bootstrap, "bootstrap"),
    ]
    .into_iter()
    .filter_map(|(step, label)| request.failed_steps.contains(&step).then_some(label))
    .collect::<Vec<_>>()
    .join(",");
    format!("CLIENT_ACCOUNT_FAILURE:{kind}:{steps}")
}

async fn create_client_notification_with_diagnostic_inner(
    runtime: &Arc<NotificationRuntime>,
    config: &Arc<RwLock<AppConfig>>,
    lifecycle: &AccountLifecycleCoordinator,
    emitter: &EventEmitter,
    request: CreateClientNotificationRequest,
    now_ms: u64,
) -> Result<CreateClientNotificationResult, NotificationCommandError> {
    let diagnostic = client_failure_diagnostic(&request);
    let result =
        create_client_notification_inner(runtime, config, lifecycle, request, now_ms).await?;
    emitter.emit_diagnostic(&diagnostic, false);
    Ok(result)
}

#[tauri::command]
pub async fn list_notifications(
    state: State<'_, AppState>,
    request: ListNotificationsRequest,
) -> Result<NotificationPage, NotificationCommandError> {
    list_notifications_inner(
        &state.notification,
        &state.config,
        state.account_lifecycle.as_ref(),
        request,
        state.time.local_now_ms(),
    )
    .await
}

#[tauri::command]
pub async fn get_notification_summary(
    state: State<'_, AppState>,
    account_id: Option<String>,
) -> Result<NotificationSummary, NotificationCommandError> {
    get_notification_summary_inner(
        &state.notification,
        &state.config,
        state.account_lifecycle.as_ref(),
        account_id,
        state.time.local_now_ms(),
    )
    .await
}

#[tauri::command]
pub async fn mark_notification_read(
    state: State<'_, AppState>,
    context_account_id: Option<String>,
    id: String,
) -> Result<MarkNotificationReadResult, NotificationCommandError> {
    mark_notification_read_inner(
        &state.notification,
        &state.config,
        state.account_lifecycle.as_ref(),
        context_account_id,
        id,
        state.time.local_now_ms(),
    )
    .await
}

#[tauri::command]
pub async fn mark_visible_notifications_read(
    state: State<'_, AppState>,
    context_account_id: Option<String>,
) -> Result<MarkVisibleNotificationsReadResult, NotificationCommandError> {
    mark_visible_notifications_read_inner(
        &state.notification,
        &state.config,
        state.account_lifecycle.as_ref(),
        context_account_id,
        state.time.local_now_ms(),
    )
    .await
}

#[tauri::command]
pub async fn delete_notification(
    state: State<'_, AppState>,
    context_account_id: Option<String>,
    id: String,
) -> Result<DeleteNotificationResult, NotificationCommandError> {
    delete_notification_inner(
        &state.notification,
        &state.config,
        state.account_lifecycle.as_ref(),
        context_account_id,
        id,
        state.time.local_now_ms(),
    )
    .await
}

#[tauri::command]
pub async fn clear_account_notifications(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<ClearAccountNotificationsResult, NotificationCommandError> {
    clear_account_notifications_inner(
        &state.notification,
        &state.config,
        state.account_lifecycle.as_ref(),
        account_id,
        state.time.local_now_ms(),
    )
    .await
}

#[tauri::command]
pub async fn create_client_notification(
    raw_request: tauri::ipc::Request<'_>,
    state: State<'_, AppState>,
    request: CreateClientNotificationRequest,
) -> Result<CreateClientNotificationResult, NotificationCommandError> {
    validate_client_notification_envelope(raw_request.body())?;
    create_client_notification_with_diagnostic_inner(
        &state.notification,
        &state.config,
        state.account_lifecycle.as_ref(),
        &state.emitter,
        request,
        state.time.local_now_ms(),
    )
    .await
}

#[cfg(test)]
mod tests;
