use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use tokio::sync::RwLock;

use crate::models::config::AppConfig;
use crate::models::notification::{
    ListNotificationsRequest, NotificationFilter, NotificationPage, NotificationRecord,
    NotificationScope, NotificationSummary, DEFAULT_NOTIFICATION_PAGE_LIMIT,
};
use crate::services::notification::{
    NotificationAvailability, NotificationError, NotificationRuntime, ViewContext,
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
    account_id: Option<String>,
    filter: Option<NotificationFilter>,
    cursor: Option<String>,
    limit: Option<u32>,
    now_ms: u64,
) -> Result<NotificationPage, NotificationCommandError> {
    let _guard = lifecycle.read_guard().await;
    let config = config.read().await;
    let context = resolve_context(&config, account_id.as_deref())?;
    drop(config);
    let service = runtime.service()?;
    service
        .list(
            context,
            ListNotificationsRequest {
                account_id,
                filter: filter.unwrap_or_default(),
                cursor,
                limit: limit.unwrap_or(DEFAULT_NOTIFICATION_PAGE_LIMIT),
            },
            now_ms,
        )
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

#[tauri::command]
pub async fn list_notifications(
    state: State<'_, AppState>,
    account_id: Option<String>,
    filter: Option<NotificationFilter>,
    cursor: Option<String>,
    limit: Option<u32>,
) -> Result<NotificationPage, NotificationCommandError> {
    list_notifications_inner(
        &state.notification,
        &state.config,
        state.account_lifecycle.as_ref(),
        account_id,
        filter,
        cursor,
        limit,
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

#[cfg(test)]
mod tests;
