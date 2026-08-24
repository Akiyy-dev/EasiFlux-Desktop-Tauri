use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::models::account::AccountSummary;
use crate::models::config::{ConnectionStatus, EnvironmentStatus};
use crate::models::market::{Depth, Kline, Ticker};
use crate::models::notification::NotificationChangedEvent;
use crate::models::time::{DailyPnlSnapshot, TimeSnapshot};
use crate::models::trading::{Order, Position, PrivatePanelsSnapshot, SessionContext};

const NOTIFICATION_CHANGED_EVENT: &str = "notification:changed";

fn emit_notification_changed_with<F>(
    event: &NotificationChangedEvent,
    emit: F,
) -> Result<(), String>
where
    F: FnOnce(&str, &NotificationChangedEvent) -> Result<(), String>,
{
    emit(NOTIFICATION_CHANGED_EVENT, event)
        .map_err(|_| "NOTIFICATION_EVENT_EMIT_FAILED".to_string())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountSessionEvent<T> {
    account_id: String,
    session_epoch: u64,
    payload: T,
}

#[derive(Clone, Default)]
struct WebsocketStatusTracker {
    status: Arc<AtomicU8>,
}

impl WebsocketStatusTracker {
    fn record(&self, status: &str) {
        let value = match status {
            "disconnected" => 0,
            "connecting" => 1,
            "connected" => 2,
            "error" => 3,
            _ => return,
        };
        self.status.store(value, Ordering::Release);
    }

    fn status(&self) -> ConnectionStatus {
        match self.status.load(Ordering::Acquire) {
            1 => ConnectionStatus::Connecting,
            2 => ConnectionStatus::Connected,
            3 => ConnectionStatus::Error,
            _ => ConnectionStatus::Disconnected,
        }
    }
}

#[derive(Clone)]
pub struct EventEmitter {
    app: AppHandle,
    websocket_status: WebsocketStatusTracker,
}

impl EventEmitter {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            websocket_status: WebsocketStatusTracker::default(),
        }
    }

    fn emit_account_session<T: serde::Serialize + ?Sized>(
        &self,
        context: &SessionContext,
        event: &str,
        payload: &T,
    ) {
        let envelope = AccountSessionEvent {
            account_id: context.account_id.clone(),
            session_epoch: context.session_epoch,
            payload,
        };
        let _ = self.app.emit(event, &envelope);
    }

    pub fn emit_app_ready(&self, version: &str) {
        let _ = self.app.emit("app:ready", version);
    }

    pub fn emit_connection_for_session(&self, context: &SessionContext, status: &str) {
        self.emit_account_session(context, "connection:status", status);
        self.emit_log("info", &format!("API 连接状态: {}", status));
    }

    pub fn emit_websocket_for_session(&self, context: &SessionContext, status: &str) {
        self.websocket_status.record(status);
        self.emit_account_session(context, "websocket:status", status);
        self.emit_log("info", &format!("WebSocket 状态: {}", status));
    }

    pub fn websocket_status(&self) -> ConnectionStatus {
        self.websocket_status.status()
    }

    pub fn emit_ticker(&self, ticker: Ticker) {
        let _ = self.app.emit("market:ticker", &ticker);
    }

    pub fn emit_depth(&self, depth: Depth) {
        let _ = self.app.emit("market:depth", &depth);
    }

    pub fn emit_klines(&self, klines: &[Kline]) {
        let _ = self.app.emit("market:kline", klines);
    }

    pub fn emit_order(&self, context: &SessionContext, order: Order) {
        self.emit_account_session(context, "order:updated", &order);
    }

    pub fn emit_position(&self, context: &SessionContext, position: Position) {
        self.emit_account_session(context, "position:updated", &position);
    }

    pub fn emit_balance(&self, context: &SessionContext, balance: crate::models::account::Balance) {
        self.emit_account_session(context, "balance:updated", &balance);
    }

    pub fn emit_time_updated(&self, snapshot: &TimeSnapshot) {
        let _ = self.app.emit("time:updated", snapshot);
    }

    pub fn emit_account_snapshot(&self, context: &SessionContext, snapshot: AccountSummary) {
        self.emit_account_session(context, "account:snapshot", &snapshot);
    }

    pub fn emit_private_panels_snapshot(
        &self,
        context: &SessionContext,
        snapshot: PrivatePanelsSnapshot,
    ) {
        self.emit_account_session(context, "private-panels:snapshot", &snapshot);
    }

    pub fn emit_daily_pnl_updated(&self, context: &SessionContext, snapshot: &DailyPnlSnapshot) {
        self.emit_account_session(context, "daily-pnl:updated", snapshot);
    }

    pub fn emit_environment_updated(&self, context: &SessionContext, status: &EnvironmentStatus) {
        self.emit_account_session(context, "environment:updated", status);
    }

    pub fn emit_notification_changed(
        &self,
        event: &NotificationChangedEvent,
    ) -> Result<(), String> {
        emit_notification_changed_with(event, |name, payload| {
            self.app
                .emit(name, payload)
                .map_err(|_| "NOTIFICATION_EVENT_EMIT_FAILED".to_string())
        })
    }

    pub fn emit_error(&self, message: &str) {
        let _ = self.app.emit("error:occurred", message);
        self.emit_log("error", message);
    }

    pub fn emit_log(&self, level: &str, message: &str) {
        let _ = self.app.emit(
            "log:entry",
            serde_json::json!({
                "level": level,
                "message": message,
                "timestamp": chrono::Utc::now().timestamp_millis(),
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use crate::models::config::ConnectionStatus;
    use crate::models::notification::{
        NotificationCategory, NotificationChange, NotificationChangedEvent, NotificationContent,
        NotificationScope, NotificationSeverity, NotificationToastCandidate,
    };

    use super::{emit_notification_changed_with, AccountSessionEvent, WebsocketStatusTracker};

    #[test]
    fn account_session_event_serializes_a_camel_case_epoch_envelope() {
        let envelope = AccountSessionEvent {
            account_id: "primary".into(),
            session_epoch: 7,
            payload: json!({ "orderId": "old-order" }),
        };

        assert_eq!(
            serde_json::to_value(envelope).unwrap(),
            json!({
                "accountId": "primary",
                "sessionEpoch": 7,
                "payload": { "orderId": "old-order" },
            })
        );
    }

    #[test]
    fn websocket_status_tracker_keeps_the_latest_recognized_runtime_state() {
        let tracker = WebsocketStatusTracker::default();
        let observer = tracker.clone();
        assert_eq!(tracker.status(), ConnectionStatus::Disconnected);

        for expected in [
            ConnectionStatus::Connecting,
            ConnectionStatus::Error,
            ConnectionStatus::Connected,
            ConnectionStatus::Disconnected,
        ] {
            tracker.record(&format!("{expected:?}").to_lowercase());
            assert_eq!(observer.status(), expected);
        }

        tracker.record("connected");
        tracker.record("unknown");
        assert_eq!(observer.status(), ConnectionStatus::Connected);
    }

    fn changed_event() -> NotificationChangedEvent {
        NotificationChangedEvent {
            previous_revision: "41".into(),
            revision: "42".into(),
            change: NotificationChange::Created,
            affected_scopes: vec![NotificationScope::Account {
                account_id: "primary".into(),
            }],
            notification_id: Some("00000000-0000-4000-8000-000000000042".into()),
            toast_candidate: Some(NotificationToastCandidate {
                id: "00000000-0000-4000-8000-000000000042".into(),
                scope: NotificationScope::Account {
                    account_id: "primary".into(),
                },
                session_epoch: Some(7),
                category: NotificationCategory::ConnectionSystem,
                severity: NotificationSeverity::Error,
                content: NotificationContent {
                    message_key: "connection.unavailable".into(),
                    params: BTreeMap::new(),
                    fallback_title: "连接不可用".into(),
                    fallback_body: "交易连接暂时不可用，请检查网络或稍后重试。".into(),
                },
                action: None,
            }),
        }
    }

    #[test]
    fn notification_changed_emits_the_exact_event_name_and_json() {
        let mut captured = None;

        emit_notification_changed_with(&changed_event(), |name, payload| {
            captured = Some((name.to_string(), serde_json::to_value(payload).unwrap()));
            Ok(())
        })
        .unwrap();

        assert_eq!(
            captured,
            Some((
                "notification:changed".into(),
                json!({
                    "previousRevision": "41",
                    "revision": "42",
                    "change": "created",
                    "affectedScopes": [
                        { "type": "account", "accountId": "primary" }
                    ],
                    "notificationId": "00000000-0000-4000-8000-000000000042",
                    "toastCandidate": {
                        "id": "00000000-0000-4000-8000-000000000042",
                        "scope": { "type": "account", "accountId": "primary" },
                        "sessionEpoch": 7,
                        "category": "connectionSystem",
                        "severity": "error",
                        "content": {
                            "messageKey": "connection.unavailable",
                            "params": {},
                            "fallbackTitle": "连接不可用",
                            "fallbackBody": "交易连接暂时不可用，请检查网络或稍后重试。"
                        }
                    }
                })
            ))
        );
    }

    #[test]
    fn notification_callback_failure_is_sanitized_and_never_recurses() {
        let calls = AtomicUsize::new(0);

        let error = emit_notification_changed_with(&changed_event(), |_, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("apiKey=raw-secret".into())
        })
        .unwrap_err();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(error, "NOTIFICATION_EVENT_EMIT_FAILED");
        assert!(!error.contains("raw-secret"));
    }
}
