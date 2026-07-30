use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::models::account::AccountSummary;
use crate::models::config::{ConnectionStatus, EnvironmentStatus};
use crate::models::market::{Depth, Kline, Ticker};
use crate::models::news::{NewsMessagesCommittedEvent, NewsStatusSnapshot};
use crate::models::time::{DailyPnlSnapshot, TimeSnapshot};
use crate::models::trading::{Order, Position, PrivatePanelsSnapshot};
use crate::services::news::ports::{NewsEventError, NewsEventSink};
use crate::services::AccountLifecycleCoordinator;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountSessionEvent<T> {
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
    account_lifecycle: Arc<AccountLifecycleCoordinator>,
    websocket_status: WebsocketStatusTracker,
}

impl EventEmitter {
    pub fn new(app: AppHandle, account_lifecycle: Arc<AccountLifecycleCoordinator>) -> Self {
        Self {
            app,
            account_lifecycle,
            websocket_status: WebsocketStatusTracker::default(),
        }
    }

    fn emit_account_session<T: serde::Serialize + ?Sized>(&self, event: &str, payload: &T) {
        let envelope = AccountSessionEvent {
            session_epoch: self.account_lifecycle.current_session_epoch(),
            payload,
        };
        let _ = self.app.emit(event, &envelope);
    }

    pub fn emit_app_ready(&self, version: &str) {
        let _ = self.app.emit("app:ready", version);
    }

    pub fn emit_connection(&self, status: &str) {
        self.emit_account_session("connection:status", status);
        self.emit_log("info", &format!("API 连接状态: {}", status));
    }

    pub fn emit_websocket(&self, status: &str) {
        self.websocket_status.record(status);
        self.emit_account_session("websocket:status", status);
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

    pub fn emit_order(&self, order: Order) {
        self.emit_order_event(&order);
    }

    fn emit_order_event(&self, order: &Order) {
        self.emit_account_session("order:updated", order);
    }

    pub fn emit_position(&self, position: Position) {
        self.emit_position_event(&position);
    }

    fn emit_position_event(&self, position: &Position) {
        self.emit_account_session("position:updated", position);
    }

    pub fn emit_balance(&self, balance: crate::models::account::Balance) {
        self.emit_account_session("balance:updated", &balance);
    }

    pub fn emit_time_updated(&self, snapshot: &TimeSnapshot) {
        let _ = self.app.emit("time:updated", snapshot);
    }

    pub fn emit_account_snapshot(&self, snapshot: AccountSummary) {
        self.emit_account_session("account:snapshot", &snapshot);
    }

    pub fn emit_private_panels_snapshot(&self, snapshot: PrivatePanelsSnapshot) {
        self.emit_account_session("private-panels:snapshot", &snapshot);
    }

    pub fn emit_daily_pnl_updated(&self, snapshot: &DailyPnlSnapshot) {
        self.emit_account_session("daily-pnl:updated", snapshot);
    }

    pub fn emit_environment_updated(&self, status: &EnvironmentStatus) {
        self.emit_account_session("environment:updated", status);
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

impl NewsEventSink for EventEmitter {
    fn emit_messages_committed(
        &self,
        event: &NewsMessagesCommittedEvent,
    ) -> Result<(), NewsEventError> {
        super::news::emit_news_payload(
            |name, payload| self.app.emit(name, payload),
            super::news::NEWS_MESSAGES_COMMITTED_EVENT,
            event,
        )
    }

    fn emit_status_changed(&self, status: &NewsStatusSnapshot) -> Result<(), NewsEventError> {
        super::news::emit_news_payload(
            |name, payload| self.app.emit(name, payload),
            super::news::NEWS_STATUS_CHANGED_EVENT,
            status,
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::models::config::ConnectionStatus;

    use super::{AccountSessionEvent, WebsocketStatusTracker};

    #[test]
    fn account_session_event_serializes_a_camel_case_epoch_envelope() {
        let envelope = AccountSessionEvent {
            session_epoch: 7,
            payload: json!({ "orderId": "old-order" }),
        };

        assert_eq!(
            serde_json::to_value(envelope).unwrap(),
            json!({
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
}
