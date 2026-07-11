use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::models::account::AccountSummary;
use crate::models::market::{Depth, Kline, Ticker};
use crate::models::time::{DailyPnlSnapshot, TimeSnapshot};
use crate::models::trading::{Order, Position, PrivatePanelsSnapshot};
use crate::services::AnalyticsService;
use crate::models::config::EnvironmentStatus;

#[derive(Clone)]
pub struct EventEmitter {
    app: AppHandle,
    analytics: Arc<AnalyticsService>,
}

impl EventEmitter {
    pub fn new(app: AppHandle, analytics: Arc<AnalyticsService>) -> Self {
        Self { app, analytics }
    }

    pub fn emit_app_ready(&self, version: &str) {
        let _ = self.app.emit("app:ready", version);
    }

    pub fn emit_connection(&self, status: &str) {
        let _ = self.app.emit("connection:status", status);
        self.emit_log("info", &format!("API 连接状态: {}", status));
    }

    pub fn emit_websocket(&self, status: &str) {
        let _ = self.app.emit("websocket:status", status);
        self.emit_log("info", &format!("WebSocket 状态: {}", status));
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
        let _ = self.app.emit("order:updated", &order);
        let analytics = self.analytics.clone();
        let tracked = order.clone();
        tauri::async_runtime::spawn(async move {
            analytics.record_order(tracked).await;
        });
    }

    pub fn emit_position(&self, position: Position) {
        let _ = self.app.emit("position:updated", &position);
        let analytics = self.analytics.clone();
        let tracked = position.clone();
        tauri::async_runtime::spawn(async move {
            analytics.record_position(tracked).await;
        });
    }

    pub fn emit_balance(&self, balance: crate::models::account::Balance) {
        let _ = self.app.emit("balance:updated", &balance);
    }

    pub fn emit_time_updated(&self, snapshot: &TimeSnapshot) {
        let _ = self.app.emit("time:updated", snapshot);
    }

    pub fn emit_account_snapshot(&self, snapshot: AccountSummary) {
        let _ = self.app.emit("account:snapshot", &snapshot);
    }

    pub fn emit_private_panels_snapshot(&self, snapshot: PrivatePanelsSnapshot) {
        let _ = self.app.emit("private-panels:snapshot", &snapshot);
    }

    pub fn emit_daily_pnl_updated(&self, snapshot: &DailyPnlSnapshot) {
        let _ = self.app.emit("daily-pnl:updated", snapshot);
    }

    pub fn emit_environment_updated(&self, status: &EnvironmentStatus) {
        let _ = self.app.emit("environment:updated", status);
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
