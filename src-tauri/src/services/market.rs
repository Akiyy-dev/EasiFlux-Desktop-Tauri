use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::RwLock;

use crate::api::mapper::merge_ticker;
use crate::api::{ApiClient, PublicApi};
use crate::error::AppResult;
use crate::events::EventEmitter;
use crate::models::market::{Depth, Kline, Ticker};
use crate::storage::{CacheStore, KlineStore};

const MAX_KLINES: usize = 200;

pub fn interval_to_ms(interval: &str) -> i64 {
    match interval {
        "D" | "d" => 86_400_000,
        "W" | "w" => 604_800_000,
        s if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) => {
            s.parse::<i64>().unwrap_or(1) * 60_000
        }
        _ => 60_000,
    }
}

fn trim_display_klines(mut klines: Vec<Kline>) -> Vec<Kline> {
    if klines.len() > MAX_KLINES {
        let start = klines.len() - MAX_KLINES;
        klines = klines.split_off(start);
    }
    klines
}

/// Upsert WS/REST bars and detect timeline gaps needing REST backfill.
pub fn merge_kline_updates(klines: &mut Vec<Kline>, updates: &[Kline], interval_ms: i64) -> bool {
    if updates.is_empty() {
        return false;
    }

    let mut map: BTreeMap<i64, Kline> = klines
        .iter()
        .cloned()
        .map(|k| (k.open_time, k))
        .collect();
    for update in updates {
        if update.open_time <= 0 {
            continue;
        }
        map.insert(update.open_time, update.clone());
    }
    *klines = map.values().cloned().collect();
    !KlineStore::detect_gaps(klines, interval_ms).is_empty()
}

pub struct MarketService {
    api: Arc<ApiClient>,
    cache: Arc<CacheStore>,
    kline_store: Arc<KlineStore>,
    emitter: EventEmitter,
    active_symbol: Arc<RwLock<String>>,
    kline_interval: Arc<RwLock<String>>,
}

impl MarketService {
    pub fn new(
        api: Arc<ApiClient>,
        cache: Arc<CacheStore>,
        kline_store: Arc<KlineStore>,
        emitter: EventEmitter,
    ) -> Self {
        Self {
            api,
            cache,
            kline_store,
            emitter,
            active_symbol: Arc::new(RwLock::new("BTCUSDT".into())),
            kline_interval: Arc::new(RwLock::new("1".into())),
        }
    }

    pub async fn set_active_symbol(&self, symbol: &str) {
        *self.active_symbol.write().await = symbol.to_string();
        self.cache.touch_symbol(symbol);
    }

    pub async fn active_symbol(&self) -> String {
        self.active_symbol.read().await.clone()
    }

    pub async fn set_kline_interval(&self, interval: &str) {
        *self.kline_interval.write().await = interval.to_string();
    }

    pub async fn kline_interval(&self) -> String {
        self.kline_interval.read().await.clone()
    }

    fn persist_and_emit(&self, symbol: &str, interval: &str, bars: &[Kline]) -> AppResult<()> {
        let stored = self.kline_store.upsert_bars(symbol, interval, bars)?;
        let display = trim_display_klines(stored);
        self.cache.set_klines(symbol, interval, display.clone());
        self.emitter.emit_klines(&display);
        Ok(())
    }

    pub fn restore_klines(&self, symbol: &str, interval: &str) -> AppResult<()> {
        let stored = self.kline_store.load(symbol, interval)?;
        if stored.is_empty() {
            return Ok(());
        }
        let display = trim_display_klines(stored);
        self.cache.set_klines(symbol, interval, display.clone());
        self.emitter.emit_klines(&display);
        Ok(())
    }

    pub async fn backfill_gaps(&self, symbol: &str, interval: &str) -> AppResult<()> {
        let bars = PublicApi::klines(&self.api, symbol, interval, 200, None, None).await?;
        self.persist_and_emit(symbol, interval, &bars)?;
        Ok(())
    }

    pub fn merge_and_emit_ticker(&self, value: &Value, symbol: &str) {
        let sym = crate::api::response::get_str(value, &["symbol", "s"])
            .unwrap_or_else(|| symbol.to_string());
        let existing = self.cache.get_ticker(&sym);
        let merged = merge_ticker(existing.as_ref(), value, symbol);
        self.cache.set_ticker(merged.clone());
        self.emitter.emit_ticker(merged);
    }

    pub fn merge_and_emit_klines(&self, symbol: &str, interval: &str, updates: Vec<Kline>) -> bool {
        if updates.is_empty() {
            return false;
        }

        let interval_ms = interval_to_ms(interval);
        let mut klines = self
            .cache
            .get_klines(symbol, interval)
            .or_else(|| self.kline_store.load(symbol, interval).ok())
            .unwrap_or_default();
        let needs_backfill = merge_kline_updates(&mut klines, &updates, interval_ms);

        if needs_backfill {
            return true;
        }

        if let Err(e) = self.persist_and_emit(symbol, interval, &klines) {
            self.emitter
                .emit_error(&format!("K线持久化失败: {}", e));
        }
        false
    }

    pub fn schedule_kline_backfill(self: &Arc<Self>, symbol: &str, interval: &str) {
        let market = Arc::clone(self);
        let symbol = symbol.to_string();
        let interval = interval.to_string();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = market.backfill_gaps(&symbol, &interval).await {
                market
                    .emitter
                    .emit_error(&format!("K线回填失败: {}", e));
            }
        });
    }

    pub async fn fetch_ticker(&self, symbol: &str) -> AppResult<Ticker> {
        let ticker = PublicApi::ticker(&self.api, symbol).await?;
        self.cache.set_ticker(ticker.clone());
        self.emitter.emit_ticker(ticker.clone());
        Ok(ticker)
    }

    pub async fn fetch_depth(&self, symbol: &str) -> AppResult<Depth> {
        let depth = PublicApi::depth(&self.api, symbol, 20).await?;
        self.emitter.emit_depth(depth.clone());
        Ok(depth)
    }

    pub async fn fetch_klines(&self, symbol: &str, interval: &str) -> AppResult<Vec<Kline>> {
        let rest = PublicApi::klines(&self.api, symbol, interval, 200, None, None).await?;
        self.persist_and_emit(symbol, interval, &rest)?;
        Ok(self
            .cache
            .get_klines(symbol, interval)
            .unwrap_or_else(|| trim_display_klines(rest)))
    }

    pub async fn refresh_ticker_depth(&self, symbol: &str) -> AppResult<()> {
        let mut failures = Vec::new();

        if let Err(e) = self.fetch_ticker(symbol).await {
            let message = format!("Ticker 刷新失败: {}", e);
            self.emitter.emit_error(&message);
            failures.push(message);
        }
        if let Err(e) = self.fetch_depth(symbol).await {
            let message = format!("深度刷新失败: {}", e);
            self.emitter.emit_error(&message);
            failures.push(message);
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(crate::error::AppError::Internal(failures.join("; ")))
        }
    }

    pub async fn refresh_klines(&self, symbol: &str) -> AppResult<()> {
        let interval = self.kline_interval.read().await.clone();
        if let Err(e) = self.backfill_gaps(symbol, &interval).await {
            let message = format!("K线刷新失败: {}", e);
            self.emitter.emit_error(&message);
            return Err(crate::error::AppError::Internal(message));
        }
        Ok(())
    }

    pub async fn refresh_snapshot(&self, symbol: &str) -> AppResult<()> {
        let mut failures = Vec::new();

        if let Err(e) = self.refresh_ticker_depth(symbol).await {
            failures.push(e.to_string());
        }
        if let Err(e) = self.refresh_klines(symbol).await {
            failures.push(e.to_string());
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(crate::error::AppError::Internal(failures.join("; ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::market::Kline;

    fn sample_kline(open_time: i64, close: &str) -> Kline {
        Kline {
            symbol: "BTCUSDT".into(),
            interval: "1".into(),
            open_time,
            open: close.into(),
            high: close.into(),
            low: close.into(),
            close: close.into(),
            volume: "1".into(),
        }
    }

    #[test]
    fn interval_to_ms_parses_minute_and_day() {
        assert_eq!(interval_to_ms("1"), 60_000);
        assert_eq!(interval_to_ms("5"), 300_000);
        assert_eq!(interval_to_ms("D"), 86_400_000);
    }

    #[test]
    fn merge_klines_appends_next_interval_bar() {
        let mut klines = vec![
            sample_kline(1_000, "1"),
            sample_kline(61_000, "2"),
            sample_kline(121_000, "3"),
        ];
        let needs_backfill = merge_kline_updates(
            &mut klines,
            &[sample_kline(181_000, "4")],
            interval_to_ms("1"),
        );
        assert!(!needs_backfill);
        assert_eq!(klines.len(), 4);
        assert_eq!(klines.last().unwrap().close, "4");
    }

    #[test]
    fn merge_klines_inserts_middle_bar_without_backfill() {
        let mut klines = vec![sample_kline(1_000, "1"), sample_kline(121_000, "3")];
        let needs_backfill = merge_kline_updates(
            &mut klines,
            &[sample_kline(61_000, "2")],
            interval_to_ms("1"),
        );
        assert!(!needs_backfill);
        assert_eq!(klines.len(), 3);
        assert_eq!(klines[1].open_time, 61_000);
    }

    #[test]
    fn merge_klines_gap_triggers_backfill() {
        let mut klines = vec![
            sample_kline(1_000, "1"),
            sample_kline(61_000, "2"),
            sample_kline(121_000, "3"),
        ];
        let needs_backfill = merge_kline_updates(
            &mut klines,
            &[sample_kline(301_000, "gap")],
            interval_to_ms("1"),
        );
        assert!(needs_backfill);
        assert_eq!(klines.len(), 4);
    }
}
