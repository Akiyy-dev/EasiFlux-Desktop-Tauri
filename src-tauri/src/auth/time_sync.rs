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

pub fn parse_server_time(payload: &serde_json::Value) -> AppResult<u64> {
    if let Some(ts) = payload.as_u64() {
        return Ok(ts);
    }
    if let Some(ts) = payload.as_i64() {
        return Ok(ts.max(0) as u64);
    }
    if let Some(obj) = payload.as_object() {
        for key in ["serverTime", "server_time", "time", "timestamp"] {
            if let Some(val) = obj.get(key) {
                if let Some(ts) = val.as_u64() {
                    return Ok(ts);
                }
                if let Some(ts) = val.as_i64() {
                    return Ok(ts.max(0) as u64);
                }
                if let Some(s) = val.as_str() {
                    if let Ok(ts) = s.parse::<u64>() {
                        return Ok(ts);
                    }
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
}
