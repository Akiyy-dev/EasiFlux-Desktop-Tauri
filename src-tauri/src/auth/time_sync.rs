use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{AppError, AppResult};

pub struct TimeSync {
    offset_ms: AtomicI64,
}

impl TimeSync {
    pub fn new() -> Self {
        Self {
            offset_ms: AtomicI64::new(0),
        }
    }

    pub fn local_timestamp_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn timestamp_ms(&self) -> u64 {
        let local = self.local_timestamp_ms() as i64;
        let offset = self.offset_ms.load(Ordering::Relaxed);
        (local + offset).max(0) as u64
    }

    pub fn set_server_time(&self, server_ms: u64) {
        let local = self.local_timestamp_ms() as i64;
        self.offset_ms
            .store(server_ms as i64 - local, Ordering::Relaxed);
    }

    pub fn set_server_time_midpoint(&self, server_ms: u64, request_start_ms: u64, request_end_ms: u64) {
        let midpoint = ((request_start_ms + request_end_ms) / 2) as i64;
        self.offset_ms
            .store(server_ms as i64 - midpoint, Ordering::Relaxed);
    }

    pub fn offset_ms(&self) -> i64 {
        self.offset_ms.load(Ordering::Relaxed)
    }
}

impl Default for TimeSync {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn sync_from_server(
    time_sync: &TimeSync,
    fetch_server_time: impl std::future::Future<Output = AppResult<u64>>,
) -> AppResult<()> {
    let server_ms = fetch_server_time.await?;
    time_sync.set_server_time(server_ms);
    Ok(())
}

fn normalize_timestamp_ms(value: u64) -> Option<u64> {
    if value >= 10_u64.pow(12) {
        Some(value)
    } else if value >= 10_u64.pow(9) {
        Some(value * 1000)
    } else {
        None
    }
}

fn parse_timestamp_value(value: &serde_json::Value) -> Option<u64> {
    let raw = if let Some(ts) = value.as_u64() {
        ts
    } else if let Some(ts) = value.as_i64() {
        ts.max(0) as u64
    } else if let Some(s) = value.as_str() {
        s.parse::<u64>().ok()?
    } else {
        return None;
    };
    normalize_timestamp_ms(raw)
}

pub fn parse_server_time(payload: &serde_json::Value) -> AppResult<u64> {
    if let Some(time) = payload.get("time") {
        if let Some(ts) = parse_timestamp_value(time) {
            return Ok(ts);
        }
    }
    if let Some(data) = payload.get("data") {
        if let Some(time) = data.get("time") {
            if let Some(ts) = parse_timestamp_value(time) {
                return Ok(ts);
            }
        }
    }
    if let Some(ts) = parse_timestamp_value(payload) {
        return Ok(ts);
    }
    if let Some(obj) = payload.as_object() {
        for key in ["serverTime", "server_time", "time", "timestamp"] {
            if let Some(val) = obj.get(key) {
                if let Some(ts) = parse_timestamp_value(val) {
                    return Ok(ts);
                }
            }
        }
    }
    Err(AppError::Connection("无法解析服务器时间".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_applied() {
        let sync = TimeSync::new();
        let before = sync.timestamp_ms();
        sync.set_server_time(before + 1000);
        assert_eq!(sync.offset_ms(), 1000);
    }

    #[test]
    fn midpoint_offset_reduces_rtt_bias() {
        let sync = TimeSync::new();
        sync.set_server_time_midpoint(10_000, 4_000, 6_000);
        assert_eq!(sync.offset_ms(), 5_000);
    }

    #[test]
    fn parse_server_time_seconds_to_ms() {
        let payload = serde_json::json!({"time": "1782850580"});
        let ts = parse_server_time(&payload).unwrap();
        assert_eq!(ts, 1_782_850_580_000);
    }

    #[test]
    fn parse_server_time_from_data_envelope() {
        let payload = serde_json::json!({
            "code": 0,
            "data": {"time": "1782997445"}
        });
        let ts = parse_server_time(&payload).unwrap();
        assert_eq!(ts, 1_782_997_445_000);
    }
}
