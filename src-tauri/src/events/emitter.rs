use tauri::{AppHandle, Emitter};

use crate::models::account::Balance;
use crate::models::market::{Depth, Kline, Ticker};
use crate::models::trading::{Order, Position};

#[derive(Clone)]
pub struct EventEmitter {
    app: AppHandle,
}

impl EventEmitter {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    pub fn emit_app_ready(&self, version: &str) {
        let _ = self.app.emit("app:ready", version);
    }

    pub fn emit_connection(&self, status: &str) {
        let _ = self.app.emit("connection:status", status);
        self.emit_log("info", &format!("连接状态: {}", status));
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
    }

    pub fn emit_position(&self, position: Position) {
        let _ = self.app.emit("position:updated", &position);
    }

    pub fn emit_balance(&self, balance: Balance) {
        let _ = self.app.emit("balance:updated", &balance);
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
