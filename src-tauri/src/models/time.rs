use serde::{Deserialize, Serialize};

pub const DEFAULT_TRADING_DAY_TIMEZONE: &str = "Asia/Shanghai";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeSyncStatus {
    Syncing,
    Synced,
    Failed,
    LocalFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeSource {
    Server,
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeSnapshot {
    pub server_time_ms: u64,
    pub local_time_ms: u64,
    pub offset_ms: i64,
    pub sync_status: TimeSyncStatus,
    pub source: TimeSource,
    pub last_sync_at: Option<u64>,
    pub last_attempt_at: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyPnlSnapshot {
    pub value: String,
    pub server_time: u64,
    pub updated_at: u64,
    pub record_count: u32,
    pub day_start: i64,
    pub day_end: i64,
    pub timezone: String,
}
