use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{Mutex, RwLock};

use crate::api::mapper::merge_ticker;
use crate::api::{ApiClient, PublicApi};
use crate::error::{AppError, AppResult};
use crate::events::EventEmitter;
use crate::models::chart_workspace::ChartWorkspaceKey;
use crate::models::config::DEFAULT_KLINE_LIMIT;
use crate::models::market::{Depth, Kline, Ticker};
use crate::services::account_profiles::run_account_public_operation;
use crate::services::{AccountLifecycleCoordinator, TimeService};
use crate::storage::{CacheStore, KlineStore};

const MAX_DISPLAY_KLINES: usize = 200;
const MAX_RANGE_FETCH_KLINES: u32 = 500;

struct BufferedKlineUpdate {
    display: Vec<Kline>,
    changed: bool,
    needs_backfill: bool,
}

fn requested_kline_limit(limit: Option<u32>) -> u32 {
    limit
        .unwrap_or(DEFAULT_KLINE_LIMIT)
        .clamp(1, MAX_RANGE_FETCH_KLINES)
}

fn chart_key(symbol: &str, interval: &str) -> AppResult<ChartWorkspaceKey> {
    ChartWorkspaceKey::parse(symbol, interval).map_err(AppError::Storage)
}

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

fn buffer_display_klines(
    store: &KlineStore,
    key: &ChartWorkspaceKey,
    bars: &[Kline],
) -> AppResult<BufferedKlineUpdate> {
    let merge = store.upsert_bars(key, bars)?;
    let display = store.load_range(key, None, None, MAX_DISPLAY_KLINES)?;
    let needs_backfill =
        !KlineStore::detect_gaps(&display, interval_to_ms(&key.interval)).is_empty();
    Ok(BufferedKlineUpdate {
        display,
        changed: merge.changed,
        needs_backfill,
    })
}

/// Upsert WS/REST bars and detect timeline gaps needing REST backfill.
#[allow(dead_code)]
pub fn merge_kline_updates(klines: &mut Vec<Kline>, updates: &[Kline], interval_ms: i64) -> bool {
    if updates.is_empty() {
        return false;
    }

    let mut map: BTreeMap<i64, Kline> = klines.iter().cloned().map(|k| (k.open_time, k)).collect();
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
    time: Arc<TimeService>,
    account_lifecycle: Arc<AccountLifecycleCoordinator>,
    chart_context: Arc<RwLock<ChartWorkspaceKey>>,
    context_mutation: Mutex<()>,
}

impl MarketService {
    pub fn new(
        api: Arc<ApiClient>,
        cache: Arc<CacheStore>,
        kline_store: Arc<KlineStore>,
        emitter: EventEmitter,
        time: Arc<TimeService>,
        account_lifecycle: Arc<AccountLifecycleCoordinator>,
        initial_chart_context: ChartWorkspaceKey,
    ) -> Self {
        Self {
            api,
            cache,
            kline_store,
            emitter,
            time,
            account_lifecycle,
            chart_context: Arc::new(RwLock::new(initial_chart_context)),
            context_mutation: Mutex::new(()),
        }
    }

    pub(crate) async fn chart_context_mutation_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.context_mutation.lock().await
    }

    pub async fn chart_context(&self) -> ChartWorkspaceKey {
        self.chart_context.read().await.clone()
    }

    pub async fn replace_runtime_chart_context(&self, key: ChartWorkspaceKey) {
        self.cache.touch_symbol(&key.symbol);
        *self.chart_context.write().await = key;
    }

    #[allow(dead_code)]
    pub async fn set_active_symbol(&self, symbol: &str) -> AppResult<()> {
        let current = self.chart_context().await;
        let key = chart_key(symbol, &current.interval)?;
        self.replace_runtime_chart_context(key).await;
        Ok(())
    }

    pub async fn active_symbol(&self) -> String {
        self.chart_context.read().await.symbol.clone()
    }

    #[allow(dead_code)]
    pub async fn set_kline_interval(&self, interval: &str) -> AppResult<()> {
        let current = self.chart_context().await;
        let key = chart_key(&current.symbol, interval)?;
        self.replace_runtime_chart_context(key).await;
        Ok(())
    }

    pub async fn kline_interval(&self) -> String {
        self.chart_context.read().await.interval.clone()
    }

    pub async fn load_local_klines(&self, key: &ChartWorkspaceKey) -> AppResult<Vec<Kline>> {
        let store = Arc::clone(&self.kline_store);
        let key = key.clone();
        tauri::async_runtime::spawn_blocking(move || {
            store.load_range(&key, None, None, MAX_DISPLAY_KLINES)
        })
        .await
        .map_err(|error| AppError::Internal(format!("K线本地读取任务失败: {error}")))?
    }

    pub fn publish_local_klines(&self, key: &ChartWorkspaceKey, bars: Vec<Kline>) {
        self.cache
            .set_klines(&key.symbol, &key.interval, bars.clone());
        self.emitter.emit_klines(&bars);
    }

    pub async fn restore_klines(&self, symbol: &str, interval: &str) -> AppResult<()> {
        let key = chart_key(symbol, interval)?;
        let stored = self.load_local_klines(&key).await?;
        if stored.is_empty() {
            return Ok(());
        }
        self.publish_local_klines(&key, stored);
        Ok(())
    }

    pub async fn backfill_gaps(&self, symbol: &str, interval: &str) -> AppResult<()> {
        let key = chart_key(symbol, interval)?;
        self.fetch_kline_range(&key, None, None, Some(DEFAULT_KLINE_LIMIT))
            .await?;
        Ok(())
    }

    pub fn merge_and_emit_ticker(&self, value: &Value, symbol: &str) {
        let sym = crate::api::response::get_str(value, &["symbol", "s"])
            .unwrap_or_else(|| symbol.to_string());
        let existing = self.cache.get_ticker(&sym);
        let mut merged = merge_ticker(existing.as_ref(), value, symbol);
        if let Some(previous) = existing {
            merged.funding_rate = previous.funding_rate;
            merged.funding_rate_updated_at = previous.funding_rate_updated_at;
            merged.funding_rate_error = previous.funding_rate_error;
            merged.next_funding_time = previous.next_funding_time;
        }
        self.cache.set_ticker(merged.clone());
        self.emitter.emit_ticker(merged);
    }

    pub fn merge_and_emit_klines(&self, symbol: &str, interval: &str, updates: Vec<Kline>) -> bool {
        if updates.is_empty() {
            return false;
        }

        let key = match chart_key(symbol, interval) {
            Ok(key) => key,
            Err(error) => {
                self.emitter.emit_error(&error.to_string());
                return false;
            }
        };
        match buffer_display_klines(&self.kline_store, &key, &updates) {
            Ok(update) => {
                if update.changed {
                    self.cache
                        .set_klines(symbol, interval, update.display.clone());
                    self.emitter.emit_klines(&update.display);
                }
                update.needs_backfill
            }
            Err(error) => {
                self.emitter
                    .emit_error(&format!("K线内存缓冲失败: {error}"));
                false
            }
        }
    }

    pub fn schedule_kline_backfill(self: &Arc<Self>, symbol: &str, interval: &str) {
        let market = Arc::clone(self);
        let account_lifecycle = Arc::clone(&self.account_lifecycle);
        let symbol = symbol.to_string();
        let interval = interval.to_string();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = run_guarded_kline_backfill(account_lifecycle.as_ref(), || {
                market.backfill_gaps(&symbol, &interval)
            })
            .await
            {
                market.emitter.emit_error(&format!("K线回填失败: {}", e));
            }
        });
    }

    pub async fn fetch_ticker(&self, symbol: &str) -> AppResult<Ticker> {
        let mut ticker = PublicApi::ticker(&self.api, symbol).await?;
        if let Some(previous) = self.cache.get_ticker(symbol) {
            ticker.funding_rate = previous.funding_rate;
            ticker.funding_rate_updated_at = previous.funding_rate_updated_at;
            ticker.funding_rate_error = previous.funding_rate_error;
            ticker.next_funding_time = previous.next_funding_time;
        } else if !ticker.funding_rate.is_empty() {
            ticker.funding_rate_updated_at = Some(self.time.now_ms());
        } else {
            ticker.funding_rate_error = Some("资金费率缺失或超出合理范围".into());
        }
        self.cache.set_ticker(ticker.clone());
        self.emitter.emit_ticker(ticker.clone());
        Ok(ticker)
    }

    pub async fn refresh_funding_rate(&self, symbol: &str) -> AppResult<()> {
        match PublicApi::ticker(&self.api, symbol).await {
            Ok(incoming) if !incoming.funding_rate.is_empty() => {
                let mut ticker = self.cache.get_ticker(symbol).unwrap_or(incoming.clone());
                ticker.funding_rate = incoming.funding_rate;
                ticker.next_funding_time = incoming.next_funding_time;
                ticker.funding_rate_updated_at = Some(self.time.now_ms());
                ticker.funding_rate_error = None;
                self.cache.set_ticker(ticker.clone());
                self.emitter.emit_ticker(ticker);
                Ok(())
            }
            Ok(_) => {
                let message = "资金费率缺失或超出合理范围".to_string();
                if let Some(mut ticker) = self.cache.get_ticker(symbol) {
                    ticker.funding_rate_error = Some(message.clone());
                    self.cache.set_ticker(ticker.clone());
                    self.emitter.emit_ticker(ticker);
                }
                Err(crate::error::AppError::Internal(message))
            }
            Err(error) => {
                if let Some(mut ticker) = self.cache.get_ticker(symbol) {
                    ticker.funding_rate_error = Some(error.user_message());
                    self.cache.set_ticker(ticker.clone());
                    self.emitter.emit_ticker(ticker);
                }
                Err(error)
            }
        }
    }

    pub async fn fetch_depth(&self, symbol: &str) -> AppResult<Depth> {
        let depth = PublicApi::depth(&self.api, symbol, 20).await?;
        self.emitter.emit_depth(depth.clone());
        Ok(depth)
    }

    pub async fn fetch_kline_range(
        &self,
        key: &ChartWorkspaceKey,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> AppResult<Vec<Kline>> {
        let requested = requested_kline_limit(limit);
        let rest =
            PublicApi::klines(&self.api, &key.symbol, &key.interval, requested, start, end).await?;
        let store = Arc::clone(&self.kline_store);
        let key_for_store = key.clone();
        let (update, result) = tauri::async_runtime::spawn_blocking(move || {
            let update = buffer_display_klines(&store, &key_for_store, &rest)?;
            let result = store.load_range(&key_for_store, start, end, requested as usize)?;
            Ok::<_, AppError>((update, result))
        })
        .await
        .map_err(|error| AppError::Internal(format!("K线范围读取任务失败: {error}")))??;
        if update.changed {
            self.cache
                .set_klines(&key.symbol, &key.interval, update.display.clone());
            self.emitter.emit_klines(&update.display);
        }
        Ok(result)
    }

    #[allow(dead_code)]
    pub async fn fetch_klines(&self, symbol: &str, interval: &str) -> AppResult<Vec<Kline>> {
        let key = chart_key(symbol, interval)?;
        self.fetch_kline_range(&key, None, None, Some(DEFAULT_KLINE_LIMIT))
            .await
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
        let interval = self.chart_context.read().await.interval.clone();
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

async fn run_guarded_kline_backfill<F, Fut>(
    coordinator: &AccountLifecycleCoordinator,
    backfill: F,
) -> AppResult<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    run_account_public_operation(coordinator, backfill).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::market::Kline;
    use crate::services::AccountLifecycleCoordinator;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "easiflux-market-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    }

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
    fn market_update_marks_store_dirty_without_immediate_disk_write() {
        let dir = test_dir("market-buffer");
        let store = KlineStore::with_dir(dir.clone());
        let key = ChartWorkspaceKey::parse("BTCUSDT", "1").unwrap();

        let update = buffer_display_klines(&store, &key, &[sample_kline(1_000, "2")]).unwrap();

        assert_eq!(update.display.len(), 1);
        assert!(update.changed);
        assert!(!dir.join("BTCUSDT_1.jsonl").exists());
    }

    #[test]
    fn market_display_remains_limited_to_latest_two_hundred() {
        let store = KlineStore::with_dir(test_dir("display-limit"));
        let key = ChartWorkspaceKey::parse("BTCUSDT", "1").unwrap();
        let updates = (1..=240)
            .map(|time| sample_kline(time, &time.to_string()))
            .collect::<Vec<_>>();

        let update = buffer_display_klines(&store, &key, &updates).unwrap();

        assert_eq!(update.display.len(), 200);
        assert_eq!(update.display.first().unwrap().open_time, 41);
    }

    #[test]
    fn unchanged_market_update_does_not_request_an_identical_snapshot() {
        let store = KlineStore::with_dir(test_dir("unchanged-update"));
        let key = ChartWorkspaceKey::parse("BTCUSDT", "1").unwrap();
        let bar = sample_kline(1_000, "2");

        assert!(
            buffer_display_klines(&store, &key, std::slice::from_ref(&bar))
                .unwrap()
                .changed
        );
        assert!(!buffer_display_klines(&store, &key, &[bar]).unwrap().changed);
    }

    #[test]
    fn ranged_history_limit_defaults_and_clamps_to_supported_bounds() {
        assert_eq!(requested_kline_limit(None), 200);
        assert_eq!(requested_kline_limit(Some(0)), 1);
        assert_eq!(requested_kline_limit(Some(501)), 500);
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

    #[tokio::test]
    async fn detached_backfill_waits_for_the_account_lifecycle_guard() {
        let coordinator = AccountLifecycleCoordinator::new();
        let held_guard = coordinator.mutation_guard().await;
        let called = AtomicBool::new(false);

        let backfill = run_guarded_kline_backfill(&coordinator, || async {
            called.store(true, Ordering::SeqCst);
            Ok(())
        });
        tokio::pin!(backfill);

        assert!(matches!(
            futures_util::poll!(&mut backfill),
            std::task::Poll::Pending
        ));
        assert!(!called.load(Ordering::SeqCst));

        drop(held_guard);
        tokio::time::timeout(std::time::Duration::from_secs(1), backfill.as_mut())
            .await
            .expect("detached backfill should run after the lifecycle guard is released")
            .unwrap();
        assert!(called.load(Ordering::SeqCst));
    }
}
