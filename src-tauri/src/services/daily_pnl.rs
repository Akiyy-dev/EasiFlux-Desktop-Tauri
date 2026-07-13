use std::collections::HashSet;
use std::sync::Arc;

use rust_decimal::Decimal;
use serde_json::Value;
use std::str::FromStr;
use tokio::sync::RwLock;

use crate::api::response::extract_list;
use crate::api::PrivateApi;
use crate::api::response::get_str;
use crate::error::AppResult;
use crate::events::EventEmitter;
use crate::models::config::AppConfig;
use crate::models::time::DailyPnlSnapshot;
use crate::services::time::{normalize_epoch_ms_i64, resolve_trading_day_timezone, trading_day_bounds, TimeService};

pub struct DailyPnlService {
    api: Arc<crate::api::ApiClient>,
    time: Arc<TimeService>,
    config: Arc<RwLock<AppConfig>>,
    emitter: EventEmitter,
}

impl DailyPnlService {
    pub fn new(
        api: Arc<crate::api::ApiClient>,
        time: Arc<TimeService>,
        config: Arc<RwLock<AppConfig>>,
        emitter: EventEmitter,
    ) -> Self {
        Self {
            api,
            time,
            config,
            emitter,
        }
    }

    pub async fn refresh(&self) -> AppResult<DailyPnlSnapshot> {
        let (timezone, server_time) = {
            let cfg = self.config.read().await;
            (
                resolve_trading_day_timezone(&cfg.trading_day_timezone),
                self.time.now_ms(),
            )
        };
        let (day_start, day_end) = trading_day_bounds(server_time, &timezone);
        let payload = PrivateApi::closed_pnl(
            &self.api,
            None,
            None,
            Some(day_start),
            Some(day_end - 1),
            Some(100),
            None,
        )
        .await?;
        let snapshot = build_snapshot(&payload, server_time, day_start, day_end, &timezone);
        self.emitter.emit_daily_pnl_updated(&snapshot);
        Ok(snapshot)
    }
}

fn build_snapshot(
    payload: &Value,
    server_time: u64,
    day_start: i64,
    day_end: i64,
    timezone: &str,
) -> DailyPnlSnapshot {
    let rows = extract_list(payload);
    let mut seen = HashSet::new();
    let mut total = Decimal::ZERO;

    for row in rows {
        let key = record_key(row);
        if !seen.insert(key) {
            continue;
        }
        let closed_time = closed_time_ms(row);
        if closed_time < day_start || closed_time >= day_end {
            continue;
        }
        let pnl_raw = get_str(row, &[
            "closedPnl",
            "closed_pnl",
            "realisedPnl",
            "realised_pnl",
            "realizedPnl",
            "realized_pnl",
            "pnl",
        ])
        .unwrap_or_else(|| "0".into());
        total += Decimal::from_str(&pnl_raw).unwrap_or(Decimal::ZERO);
    }

    DailyPnlSnapshot {
        value: format!("{:.4}", total),
        server_time,
        updated_at: server_time,
        record_count: seen.len() as u32,
        day_start,
        day_end,
        timezone: timezone.to_string(),
    }
}

fn record_key(row: &Value) -> String {
    if let Some(id) = get_str(
        row,
        &[
            "id",
            "closedPnlId",
            "closed_pnl_id",
            "orderId",
            "order_id",
        ],
    ) {
        return id;
    }
    let symbol = get_str(row, &["symbol", "s"]).unwrap_or_default();
    let side = get_str(row, &["side"]).unwrap_or_default();
    let closed_time = closed_time_ms(row);
    let size = get_str(row, &["closedSize", "closed_size", "qty", "size"]).unwrap_or_default();
    let pnl = get_str(
        row,
        &["closedPnl", "closed_pnl", "pnl", "realisedPnl", "realised_pnl"],
    )
    .unwrap_or_default();
    format!("{symbol}:{side}:{closed_time}:{size}:{pnl}")
}

fn closed_time_ms(row: &Value) -> i64 {
    let raw = get_str(
        row,
        &[
            "closedTime",
            "closed_time",
            "updatedTime",
            "updated_time",
            "time",
            "timestamp",
        ],
    )
    .and_then(|s| s.parse::<i64>().ok())
    .or_else(|| {
        row.get("closedTime")
            .or_else(|| row.get("closed_time"))
            .or_else(|| row.get("time"))
            .and_then(|v| v.as_i64())
    })
    .unwrap_or(0);
    normalize_epoch_ms_i64(raw).unwrap_or(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_closed_pnl_records() {
        let payload = serde_json::json!({
            "data": {
                "list": [
                    {
                        "id": "a1",
                        "closedPnl": "1.5",
                        "closedTime": "1700000000000"
                    },
                    {
                        "id": "a1",
                        "closedPnl": "1.5",
                        "closedTime": "1700000000000"
                    },
                    {
                        "id": "a2",
                        "closedPnl": "-0.5",
                        "closedTime": "1700000000000"
                    }
                ]
            }
        });
        let snapshot = build_snapshot(
            &payload,
            1_700_000_000_000,
            1_699_948_800_000,
            1_700_035_200_000,
            "UTC",
        );
        assert_eq!(snapshot.record_count, 2);
        assert_eq!(snapshot.value, "1.0000");
    }
}
