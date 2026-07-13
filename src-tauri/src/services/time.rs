use std::sync::Arc;

use tokio::sync::RwLock;

use crate::api::{ApiClient, PublicApi};
use crate::auth::TimeSync;
use crate::error::AppResult;
use crate::events::EventEmitter;
use crate::models::time::{TimeSnapshot, TimeSource, TimeSyncStatus};

use crate::models::time::DEFAULT_TRADING_DAY_TIMEZONE;

pub fn normalize_epoch_ms(value: u64) -> Option<u64> {
    if value >= 10_u64.pow(12) {
        Some(value)
    } else if value >= 10_u64.pow(9) {
        Some(value * 1000)
    } else {
        None
    }
}

pub fn normalize_epoch_ms_i64(value: i64) -> Option<i64> {
    if value <= 0 {
        return None;
    }
    normalize_epoch_ms(value as u64).map(|v| v as i64)
}

pub fn is_valid_iana_timezone(tz: &str) -> bool {
    tz.parse::<chrono_tz::Tz>().is_ok()
}

pub fn resolve_trading_day_timezone(tz: &str) -> String {
    if is_valid_iana_timezone(tz) {
        tz.to_string()
    } else {
        DEFAULT_TRADING_DAY_TIMEZONE.to_string()
    }
}

pub fn trading_day_bounds(now_ms: u64, timezone: &str) -> (i64, i64) {
    use chrono::{TimeZone, Utc};
    use chrono_tz::Tz;

    let tz: Tz = timezone
        .parse()
        .unwrap_or(chrono_tz::Asia::Shanghai);
    let now = chrono::DateTime::<Utc>::from_timestamp_millis(now_ms as i64)
        .unwrap_or_else(Utc::now);
    let local = now.with_timezone(&tz);
    let start_naive = local.date_naive().and_hms_opt(0, 0, 0).unwrap();
    let start = tz
        .from_local_datetime(&start_naive)
        .single()
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0);
    (start, start + 86_400_000)
}

#[derive(Debug, Clone)]
struct TimeMeta {
    sync_status: TimeSyncStatus,
    source: TimeSource,
    last_sync_at: Option<u64>,
    last_attempt_at: Option<u64>,
    last_error: Option<String>,
}

impl Default for TimeMeta {
    fn default() -> Self {
        Self {
            sync_status: TimeSyncStatus::LocalFallback,
            source: TimeSource::Local,
            last_sync_at: None,
            last_attempt_at: None,
            last_error: None,
        }
    }
}

pub struct TimeService {
    time_sync: Arc<TimeSync>,
    api: Arc<ApiClient>,
    emitter: EventEmitter,
    meta: RwLock<TimeMeta>,
}

impl TimeService {
    pub fn new(time_sync: Arc<TimeSync>, api: Arc<ApiClient>, emitter: EventEmitter) -> Self {
        Self {
            time_sync,
            api,
            emitter,
            meta: RwLock::new(TimeMeta::default()),
        }
    }

    pub fn now_ms(&self) -> u64 {
        self.time_sync.timestamp_ms()
    }

    pub fn local_now_ms(&self) -> u64 {
        self.time_sync.local_timestamp_ms()
    }

    pub fn offset_ms(&self) -> i64 {
        self.time_sync.offset_ms()
    }

    pub async fn snapshot(&self) -> TimeSnapshot {
        let meta = self.meta.read().await;
        TimeSnapshot {
            server_time_ms: self.now_ms(),
            local_time_ms: self.local_now_ms(),
            offset_ms: self.offset_ms(),
            sync_status: meta.sync_status,
            source: meta.source,
            last_sync_at: meta.last_sync_at,
            last_attempt_at: meta.last_attempt_at,
            last_error: meta.last_error.clone(),
        }
    }

    pub async fn emit_snapshot(&self) {
        let snapshot = self.snapshot().await;
        self.emitter.emit_time_updated(&snapshot);
    }

    pub async fn sync(&self) -> AppResult<TimeSnapshot> {
        {
            let mut meta = self.meta.write().await;
            meta.sync_status = TimeSyncStatus::Syncing;
            meta.last_attempt_at = Some(self.local_now_ms());
        }
        self.emit_snapshot().await;

        let started = self.local_now_ms();
        let result = PublicApi::server_time(&self.api).await;
        let finished = self.local_now_ms();

        let mut meta = self.meta.write().await;
        meta.last_attempt_at = Some(finished);

        match result {
            Ok(server_ms) => {
                self.time_sync
                    .set_server_time_midpoint(server_ms, started, finished);
                meta.sync_status = TimeSyncStatus::Synced;
                meta.source = TimeSource::Server;
                meta.last_sync_at = Some(finished);
                meta.last_error = None;
            }
            Err(error) => {
                meta.sync_status = TimeSyncStatus::Failed;
                meta.source = TimeSource::Local;
                meta.last_error = Some(error.user_message());
            }
        }

        // Release the write lock before snapshot() acquires a read lock.
        drop(meta);
        let snapshot = self.snapshot().await;
        self.emitter.emit_time_updated(&snapshot);
        Ok(snapshot)
    }

    pub async fn apply_server_time(&self, server_ms: u64) -> TimeSnapshot {
        let local = self.local_now_ms();
        self.time_sync.set_server_time_midpoint(server_ms, local, local);
        {
            let mut meta = self.meta.write().await;
            meta.sync_status = TimeSyncStatus::Synced;
            meta.source = TimeSource::Server;
            meta.last_sync_at = Some(local);
            meta.last_attempt_at = Some(local);
            meta.last_error = None;
        }
        let snapshot = self.snapshot().await;
        self.emitter.emit_time_updated(&snapshot);
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_epoch_ms_handles_seconds_and_millis() {
        assert_eq!(normalize_epoch_ms(1_700_000_000), Some(1_700_000_000_000));
        assert_eq!(normalize_epoch_ms(1_700_000_000_000), Some(1_700_000_000_000));
        assert_eq!(normalize_epoch_ms(42), None);
    }

    #[test]
    fn trading_day_bounds_use_configured_timezone() {
        let now = 1_735_689_600_000_i64 as u64;
        let (start, end) = trading_day_bounds(now, "Asia/Shanghai");
        assert_eq!(end - start, 86_400_000);
        assert!(start <= now as i64);
        assert!(end > now as i64);
    }

    #[test]
    fn invalid_timezone_falls_back_to_shanghai() {
        assert_eq!(
            resolve_trading_day_timezone("Invalid/Zone"),
            DEFAULT_TRADING_DAY_TIMEZONE
        );
        assert!(is_valid_iana_timezone("Asia/Shanghai"));
    }
}
