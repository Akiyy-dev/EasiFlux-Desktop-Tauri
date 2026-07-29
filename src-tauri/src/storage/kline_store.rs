use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use crate::error::{AppError, AppResult};
use crate::models::chart_workspace::ChartWorkspaceKey;
use crate::models::config::APP_NAME;
use crate::models::market::Kline;

pub const MAX_STORED_BARS: usize = 10_000;
const COMPACT_PHYSICAL_RECORDS: usize = 12_000;

#[derive(Debug, Clone)]
pub struct KlineMergeResult {
    #[allow(dead_code)]
    pub revision: u64,
    pub changed: bool,
}

#[derive(Debug, Clone)]
pub struct KlineFlushOutcome {
    #[allow(dead_code)]
    pub revision: u64,
    pub wrote: bool,
    #[allow(dead_code)]
    pub compacted: bool,
}

#[cfg(test)]
pub(crate) type FlushHook = Arc<dyn Fn(&ChartWorkspaceKey, FlushPhase) + Send + Sync>;

pub struct KlineStore {
    dir: PathBuf,
    entries: RwLock<HashMap<ChartWorkspaceKey, Arc<BufferedKeyState>>>,
    #[cfg(test)]
    flush_hook: Option<FlushHook>,
}

struct BufferedKeyState {
    series: Mutex<BufferedSeries>,
    flush_lock: Mutex<()>,
}

#[derive(Default)]
struct BufferedSeries {
    loaded: bool,
    bars: BTreeMap<i64, Kline>,
    pending: BTreeMap<i64, PendingEntry>,
    revision: u64,
    physical_records: usize,
    compaction_revision: Option<u64>,
}

#[derive(Debug, Clone)]
struct PendingEntry {
    kline: Kline,
    revision: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct FlushBatch {
    key: ChartWorkspaceKey,
    entries: Vec<PendingEntry>,
    snapshot: Vec<Kline>,
    snapshot_revision: u64,
    compact: bool,
}

struct LoadedSeries {
    bars: BTreeMap<i64, Kline>,
    physical_records: usize,
    recovered_fallback: bool,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlushPhase {
    DiskWriteStarted,
}

impl KlineStore {
    pub fn new() -> Self {
        let dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(APP_NAME)
            .join("klines");
        let _ = fs::create_dir_all(&dir);
        Self::with_dir(dir)
    }

    pub(crate) fn with_dir(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        Self {
            dir,
            entries: RwLock::new(HashMap::new()),
            #[cfg(test)]
            flush_hook: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_dir_and_flush_hook(dir: PathBuf, hook: FlushHook) -> Self {
        let _ = fs::create_dir_all(&dir);
        Self {
            dir,
            entries: RwLock::new(HashMap::new()),
            flush_hook: Some(hook),
        }
    }

    fn file_path(&self, key: &ChartWorkspaceKey) -> PathBuf {
        self.dir
            .join(format!("{}_{}.jsonl", key.symbol, key.interval))
    }

    fn state_for(&self, key: &ChartWorkspaceKey) -> Arc<BufferedKeyState> {
        if let Some(state) = self.entries.read().unwrap().get(key) {
            return Arc::clone(state);
        }
        let mut entries = self.entries.write().unwrap();
        Arc::clone(entries.entry(key.clone()).or_insert_with(|| {
            Arc::new(BufferedKeyState {
                series: Mutex::new(BufferedSeries::default()),
                flush_lock: Mutex::new(()),
            })
        }))
    }

    fn ensure_loaded(&self, key: &ChartWorkspaceKey, state: &BufferedKeyState) -> AppResult<()> {
        let mut series = state.series.lock().unwrap();
        if series.loaded {
            return Ok(());
        }
        let loaded = self.load_from_disk(key)?;
        series.bars = loaded.bars;
        series.physical_records = loaded.physical_records;
        if loaded.recovered_fallback || series.physical_records > COMPACT_PHYSICAL_RECORDS {
            series.compaction_revision = Some(series.revision);
        }
        series.loaded = true;
        Ok(())
    }

    pub fn upsert_bars(
        &self,
        key: &ChartWorkspaceKey,
        bars: &[Kline],
    ) -> AppResult<KlineMergeResult> {
        if bars
            .iter()
            .any(|bar| bar.symbol != key.symbol || bar.interval != key.interval)
        {
            return Err(AppError::Storage(format!(
                "kline key mismatch for {}_{}",
                key.symbol, key.interval
            )));
        }

        let state = self.state_for(key);
        self.ensure_loaded(key, &state)?;
        let mut series = state.series.lock().unwrap();
        let mut changed = false;
        for bar in bars {
            if bar.open_time <= 0 || series.bars.get(&bar.open_time) == Some(bar) {
                continue;
            }
            series.revision = series.revision.saturating_add(1);
            let revision = series.revision;
            series.bars.insert(bar.open_time, bar.clone());
            series.pending.insert(
                bar.open_time,
                PendingEntry {
                    kline: bar.clone(),
                    revision,
                },
            );
            changed = true;
        }

        if series.bars.len() > MAX_STORED_BARS {
            series.compaction_revision = Some(series.revision);
            while series.bars.len() > MAX_STORED_BARS {
                let Some(oldest) = series.bars.keys().next().copied() else {
                    break;
                };
                series.bars.remove(&oldest);
                series.pending.remove(&oldest);
            }
        }

        Ok(KlineMergeResult {
            revision: series.revision,
            changed,
        })
    }

    pub fn load_range(
        &self,
        key: &ChartWorkspaceKey,
        from: Option<i64>,
        to: Option<i64>,
        limit: usize,
    ) -> AppResult<Vec<Kline>> {
        if matches!((from, to), (Some(from), Some(to)) if from > to) {
            return Err(AppError::Storage("kline range start exceeds end".into()));
        }
        let state = self.state_for(key);
        self.ensure_loaded(key, &state)?;
        let series = state.series.lock().unwrap();
        let mut bars = series
            .bars
            .range(from.unwrap_or(i64::MIN)..=to.unwrap_or(i64::MAX))
            .map(|(_, bar)| bar.clone())
            .collect::<Vec<_>>();
        let limit = limit.clamp(1, MAX_STORED_BARS);
        if bars.len() > limit {
            bars = bars.split_off(bars.len() - limit);
        }
        Ok(bars)
    }

    pub fn last_open_time(&self, key: &ChartWorkspaceKey) -> AppResult<Option<i64>> {
        let state = self.state_for(key);
        self.ensure_loaded(key, &state)?;
        let open_time = state
            .series
            .lock()
            .unwrap()
            .bars
            .keys()
            .next_back()
            .copied();
        Ok(open_time)
    }

    pub fn flush_key(&self, key: &ChartWorkspaceKey) -> AppResult<KlineFlushOutcome> {
        let state = self.state_for(key);
        self.ensure_loaded(key, &state)?;
        let _flush_guard = state.flush_lock.lock().unwrap();
        let Some(batch) = self.prepare_flush(key, &state) else {
            return Ok(KlineFlushOutcome {
                revision: state.series.lock().unwrap().revision,
                wrote: false,
                compacted: false,
            });
        };

        #[cfg(test)]
        if let Some(hook) = &self.flush_hook {
            hook(key, FlushPhase::DiskWriteStarted);
        }

        if batch.compact {
            self.compact_batch(&batch)?;
        } else {
            self.append_batch(&batch)?;
        }
        self.acknowledge_flush(&state, &batch);
        Ok(KlineFlushOutcome {
            revision: batch.snapshot_revision,
            wrote: !batch.entries.is_empty(),
            compacted: batch.compact,
        })
    }

    pub fn flush_dirty(&self) -> Vec<(ChartWorkspaceKey, AppResult<KlineFlushOutcome>)> {
        let entries = self.entries.read().unwrap();
        let mut keys = entries
            .iter()
            .filter_map(|(key, state)| {
                let series = state.series.lock().unwrap();
                (!series.pending.is_empty() || series.compaction_revision.is_some())
                    .then(|| key.clone())
            })
            .collect::<Vec<_>>();
        drop(entries);
        keys.sort_by(|left, right| {
            (&left.symbol, &left.interval).cmp(&(&right.symbol, &right.interval))
        });
        keys.into_iter()
            .map(|key| {
                let outcome = self.flush_key(&key);
                (key, outcome)
            })
            .collect()
    }

    fn prepare_flush(
        &self,
        key: &ChartWorkspaceKey,
        state: &BufferedKeyState,
    ) -> Option<FlushBatch> {
        let series = state.series.lock().unwrap();
        if series.pending.is_empty() && series.compaction_revision.is_none() {
            return None;
        }
        let entries = series.pending.values().cloned().collect::<Vec<_>>();
        let compact = series.compaction_revision.is_some()
            || series.physical_records.saturating_add(entries.len()) > COMPACT_PHYSICAL_RECORDS;
        Some(FlushBatch {
            key: key.clone(),
            entries,
            snapshot: series.bars.values().cloned().collect(),
            snapshot_revision: series.revision,
            compact,
        })
    }

    fn acknowledge_flush(&self, state: &BufferedKeyState, batch: &FlushBatch) {
        let mut series = state.series.lock().unwrap();
        series
            .pending
            .retain(|_, pending| pending.revision > batch.snapshot_revision);
        if batch.compact {
            series.physical_records = batch.snapshot.len();
            if series
                .compaction_revision
                .is_some_and(|revision| revision <= batch.snapshot_revision)
            {
                series.compaction_revision = None;
            }
        } else {
            series.physical_records = series.physical_records.saturating_add(batch.entries.len());
        }
    }

    fn append_batch(&self, batch: &FlushBatch) -> AppResult<()> {
        let path = self.file_path(&batch.key);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(storage_error)?;
        for pending in &batch.entries {
            serde_json::to_writer(&mut file, &pending.kline)
                .map_err(|error| AppError::Storage(error.to_string()))?;
            file.write_all(b"\n").map_err(storage_error)?;
        }
        file.sync_all().map_err(storage_error)
    }

    fn compact_batch(&self, batch: &FlushBatch) -> AppResult<()> {
        let path = self.file_path(&batch.key);
        let tmp = path.with_extension("jsonl.tmp");
        let backup = path.with_extension("jsonl.bak");
        let mut file = File::create(&tmp).map_err(storage_error)?;
        for kline in &batch.snapshot {
            serde_json::to_writer(&mut file, kline)
                .map_err(|error| AppError::Storage(error.to_string()))?;
            file.write_all(b"\n").map_err(storage_error)?;
        }
        file.sync_all().map_err(storage_error)?;
        drop(file);

        let main_is_valid = self.is_fully_parseable_file(&path, &batch.key)?;
        if main_is_valid {
            if backup.exists() {
                if backup.is_file() {
                    fs::remove_file(&backup).map_err(storage_error)?;
                } else {
                    return Err(AppError::Storage(format!(
                        "backup path is not a file: {}",
                        backup.display()
                    )));
                }
            }
            fs::rename(&path, &backup).map_err(storage_error)?;
        } else if path.exists() && path.is_file() {
            fs::remove_file(&path).map_err(storage_error)?;
        }

        if let Err(promote_error) = fs::rename(&tmp, &path) {
            if main_is_valid && backup.exists() {
                if let Err(restore_error) = fs::copy(&backup, &path) {
                    return Err(AppError::Storage(format!(
                        "failed to promote compacted kline file: {promote_error}; failed to restore backup: {restore_error}"
                    )));
                }
            }
            return Err(storage_error(promote_error));
        }
        Ok(())
    }

    fn load_from_disk(&self, key: &ChartWorkspaceKey) -> AppResult<LoadedSeries> {
        let path = self.file_path(key);
        if path.is_file() && fs::metadata(&path).map_err(storage_error)?.len() > 0 {
            let (bars, physical_records) = self.parse_main_file(&path, key)?;
            return Ok(LoadedSeries {
                bars: crop_bars(bars),
                physical_records,
                recovered_fallback: false,
            });
        }

        for fallback in [
            path.with_extension("jsonl.tmp"),
            path.with_extension("jsonl.bak"),
        ] {
            if let Some((bars, physical_records)) = self.parse_fallback_file(&fallback, key)? {
                return Ok(LoadedSeries {
                    bars: crop_bars(bars),
                    physical_records,
                    recovered_fallback: true,
                });
            }
        }

        Ok(LoadedSeries {
            bars: BTreeMap::new(),
            physical_records: 0,
            recovered_fallback: false,
        })
    }

    fn parse_main_file(
        &self,
        path: &std::path::Path,
        key: &ChartWorkspaceKey,
    ) -> AppResult<(BTreeMap<i64, Kline>, usize)> {
        let file = File::open(path).map_err(storage_error)?;
        let reader = BufReader::new(file);
        let mut bars = BTreeMap::new();
        let mut physical_records = 0;
        for line in reader.lines() {
            physical_records += 1;
            let line = line.map_err(storage_error)?;
            if line.trim().is_empty() {
                continue;
            }
            let kline: Kline = match serde_json::from_str(&line) {
                Ok(kline) => kline,
                Err(error) => {
                    tracing::warn!(
                        "skip corrupt kline cache line for {}_{}: {error}",
                        key.symbol,
                        key.interval
                    );
                    continue;
                }
            };
            if kline.open_time > 0 && kline.symbol == key.symbol && kline.interval == key.interval {
                bars.insert(kline.open_time, kline);
            }
        }
        Ok((bars, physical_records))
    }

    fn parse_fallback_file(
        &self,
        path: &std::path::Path,
        key: &ChartWorkspaceKey,
    ) -> AppResult<Option<(BTreeMap<i64, Kline>, usize)>> {
        if !path.is_file() || fs::metadata(path).map_err(storage_error)?.len() == 0 {
            return Ok(None);
        }
        let file = File::open(path).map_err(storage_error)?;
        let reader = BufReader::new(file);
        let mut bars = BTreeMap::new();
        let mut physical_records = 0;
        for line in reader.lines() {
            physical_records += 1;
            let line = line.map_err(storage_error)?;
            if line.trim().is_empty() {
                return Ok(None);
            }
            let Ok(kline) = serde_json::from_str::<Kline>(&line) else {
                return Ok(None);
            };
            if kline.open_time <= 0 || kline.symbol != key.symbol || kline.interval != key.interval
            {
                return Ok(None);
            }
            bars.insert(kline.open_time, kline);
        }
        Ok((physical_records > 0).then_some((bars, physical_records)))
    }

    fn is_fully_parseable_file(
        &self,
        path: &std::path::Path,
        key: &ChartWorkspaceKey,
    ) -> AppResult<bool> {
        Ok(self.parse_fallback_file(path, key)?.is_some())
    }

    #[cfg(test)]
    pub(crate) fn prepare_flush_for_test(
        &self,
        key: &ChartWorkspaceKey,
    ) -> AppResult<Option<FlushBatch>> {
        let state = self.state_for(key);
        self.ensure_loaded(key, &state)?;
        Ok(self.prepare_flush(key, &state))
    }

    #[cfg(test)]
    pub(crate) fn acknowledge_flush_for_test(&self, batch: &FlushBatch) {
        let state = self.state_for(&batch.key);
        self.acknowledge_flush(&state, batch);
    }

    #[cfg(test)]
    pub(crate) fn is_dirty_for_test(&self, key: &ChartWorkspaceKey) -> bool {
        let state = self.state_for(key);
        let series = state.series.lock().unwrap();
        !series.pending.is_empty() || series.compaction_revision.is_some()
    }

    pub fn detect_gaps(series: &[Kline], interval_ms: i64) -> Vec<(i64, i64)> {
        if series.len() < 2 || interval_ms <= 0 {
            return Vec::new();
        }
        let mut gaps = Vec::new();
        for window in series.windows(2) {
            let prev = &window[0];
            let next = &window[1];
            let expected = prev.open_time + interval_ms;
            if next.open_time > expected {
                gaps.push((expected, next.open_time - interval_ms));
            }
        }
        gaps
    }
}

impl Default for KlineStore {
    fn default() -> Self {
        Self::new()
    }
}

fn crop_bars(mut bars: BTreeMap<i64, Kline>) -> BTreeMap<i64, Kline> {
    while bars.len() > MAX_STORED_BARS {
        let Some(oldest) = bars.keys().next().copied() else {
            break;
        };
        bars.remove(&oldest);
    }
    bars
}

fn storage_error(error: std::io::Error) -> AppError {
    AppError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use crate::models::chart_workspace::ChartWorkspaceKey;

    fn key(symbol: &str, interval: &str) -> ChartWorkspaceKey {
        ChartWorkspaceKey::parse(symbol, interval).unwrap()
    }

    fn sample(open_time: i64, close: &str) -> Kline {
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

    fn sample_for(symbol: &str, interval: &str, open_time: i64, close: &str) -> Kline {
        Kline {
            symbol: symbol.into(),
            interval: interval.into(),
            open_time,
            open: close.into(),
            high: close.into(),
            low: close.into(),
            close: close.into(),
            volume: "1".into(),
        }
    }

    fn test_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("easiflux-kline-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn seeded_store(name: &str, count: i64) -> KlineStore {
        let store = KlineStore::with_dir(test_dir(name));
        let bars = (1..=count)
            .map(|time| sample(time * 1_000, "1"))
            .collect::<Vec<_>>();
        store.upsert_bars(&key("BTCUSDT", "1"), &bars).unwrap();
        store
    }

    fn append_raw(path: &std::path::Path, lines: &[String]) {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        file.sync_all().unwrap();
    }

    fn serialized(bar: &Kline) -> String {
        serde_json::to_string(bar).unwrap()
    }

    fn physical_lines(path: &std::path::Path) -> usize {
        fs::read_to_string(path).unwrap().lines().count()
    }

    struct BlockingFlushFixture {
        store: Arc<KlineStore>,
        key: ChartWorkspaceKey,
        dir: PathBuf,
        started_rx: Mutex<mpsc::Receiver<()>>,
        release_tx: mpsc::SyncSender<()>,
        hook_calls: Arc<AtomicUsize>,
    }

    impl BlockingFlushFixture {
        fn spawn_flush(&self) -> thread::JoinHandle<AppResult<KlineFlushOutcome>> {
            let store = Arc::clone(&self.store);
            let key = self.key.clone();
            thread::spawn(move || store.flush_key(&key))
        }

        fn wait_until_first_flush_reaches_disk_hook(&self) {
            self.started_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
        }

        fn second_flush_reached_disk_hook(&self) -> bool {
            thread::sleep(Duration::from_millis(50));
            self.hook_calls.load(Ordering::SeqCst) > 1
        }

        fn release_first_flush(&self) {
            self.release_tx.send(()).unwrap();
        }

        fn physical_valid_lines(&self) -> usize {
            let path = self.dir.join("BTCUSDT_1.jsonl");
            fs::read_to_string(path)
                .unwrap()
                .lines()
                .filter(|line| serde_json::from_str::<Kline>(line).is_ok())
                .count()
        }
    }

    fn blocking_flush_fixture(name: &str) -> BlockingFlushFixture {
        let dir = test_dir(name);
        let key = key("BTCUSDT", "1");
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let release_rx = Arc::new(Mutex::new(release_rx));
        let hook_calls = Arc::new(AtomicUsize::new(0));
        let hook_calls_for_hook = Arc::clone(&hook_calls);
        let hook = Arc::new(move |_key: &ChartWorkspaceKey, phase: FlushPhase| {
            if phase == FlushPhase::DiskWriteStarted {
                let call = hook_calls_for_hook.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    started_tx.send(()).unwrap();
                    release_rx.lock().unwrap().recv().unwrap();
                }
            }
        });
        let store = Arc::new(KlineStore::with_dir_and_flush_hook(dir.clone(), hook));
        BlockingFlushFixture {
            store,
            key,
            dir,
            started_rx: Mutex::new(started_rx),
            release_tx,
            hook_calls,
        }
    }

    #[test]
    fn detect_gaps_finds_missing_intervals() {
        let series = vec![sample(1_000, "1"), sample(121_000, "1")];
        let gaps = KlineStore::detect_gaps(&series, 60_000);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0], (61_000, 61_000));
    }

    #[test]
    fn equal_bar_does_not_advance_revision_or_write_before_flush() {
        let dir = test_dir("no-op");
        let store = KlineStore::with_dir(dir.clone());
        let key = key("BTCUSDT", "1");
        let first = store.upsert_bars(&key, &[sample(1_000, "1")]).unwrap();
        let second = store.upsert_bars(&key, &[sample(1_000, "1")]).unwrap();
        assert!(first.changed);
        assert!(!second.changed);
        assert_eq!(first.revision, second.revision);
        assert!(!dir.join("BTCUSDT_1.jsonl").exists());
    }

    #[test]
    fn load_range_filters_sorts_and_keeps_the_newest_limit() {
        let store = seeded_store("range", 20);
        let bars = store
            .load_range(&key("BTCUSDT", "1"), Some(5_000), Some(15_000), 4)
            .unwrap();
        assert_eq!(
            bars.iter().map(|bar| bar.open_time).collect::<Vec<_>>(),
            vec![12_000, 13_000, 14_000, 15_000]
        );
    }

    #[test]
    fn store_keeps_latest_ten_thousand_unique_bars() {
        let store = KlineStore::with_dir(test_dir("limit"));
        let bars = (1..=10_250)
            .map(|time| sample(time, "1"))
            .collect::<Vec<_>>();
        store.upsert_bars(&key("BTCUSDT", "1"), &bars).unwrap();
        let loaded = store
            .load_range(&key("BTCUSDT", "1"), None, None, 20_000)
            .unwrap();
        assert_eq!(loaded.len(), 10_000);
        assert_eq!(loaded.first().unwrap().open_time, 251);
    }

    #[test]
    fn update_arriving_after_flush_snapshot_remains_dirty() {
        let store = KlineStore::with_dir(test_dir("racing-update"));
        let key = key("BTCUSDT", "1");
        store.upsert_bars(&key, &[sample(1_000, "1")]).unwrap();
        let batch = store.prepare_flush_for_test(&key).unwrap().unwrap();
        store.upsert_bars(&key, &[sample(1_000, "2")]).unwrap();
        store.acknowledge_flush_for_test(&batch);
        assert!(store.is_dirty_for_test(&key));
    }

    #[test]
    fn concurrent_flushes_for_same_key_write_one_batch() {
        let fixture = blocking_flush_fixture("same-key-single-flight");
        fixture
            .store
            .upsert_bars(&fixture.key, &[sample(1_000, "1")])
            .unwrap();
        let first = fixture.spawn_flush();
        fixture.wait_until_first_flush_reaches_disk_hook();
        let second = fixture.spawn_flush();
        assert!(!fixture.second_flush_reached_disk_hook());
        fixture.release_first_flush();
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
        assert_eq!(fixture.physical_valid_lines(), 1);
    }

    #[test]
    fn malformed_lines_are_skipped_but_counted() {
        let dir = test_dir("malformed");
        let path = dir.join("BTCUSDT_1.jsonl");
        append_raw(
            &path,
            &[
                serialized(&sample(1_000, "1")),
                "not-json".into(),
                String::new(),
            ],
        );
        let store = KlineStore::with_dir(dir);
        let bars = store
            .load_range(&key("BTCUSDT", "1"), None, None, 10)
            .unwrap();
        assert_eq!(bars, vec![sample(1_000, "1")]);
        assert_eq!(physical_lines(&path), 3);
    }

    #[test]
    fn flush_appends_only_changed_bars() {
        let dir = test_dir("append-changed");
        let store = KlineStore::with_dir(dir.clone());
        let key = key("BTCUSDT", "1");
        store.upsert_bars(&key, &[sample(1_000, "1")]).unwrap();
        assert!(store.flush_key(&key).unwrap().wrote);
        store.upsert_bars(&key, &[sample(1_000, "1")]).unwrap();
        assert!(!store.flush_key(&key).unwrap().wrote);
        store.upsert_bars(&key, &[sample(1_000, "2")]).unwrap();
        assert!(store.flush_key(&key).unwrap().wrote);
        assert_eq!(physical_lines(&dir.join("BTCUSDT_1.jsonl")), 2);
    }

    #[test]
    fn physical_threshold_triggers_compaction() {
        let dir = test_dir("physical-threshold");
        let path = dir.join("BTCUSDT_1.jsonl");
        let lines = (1..=12_001)
            .map(|time| serialized(&sample(time, "1")))
            .collect::<Vec<_>>();
        append_raw(&path, &lines);
        let store = KlineStore::with_dir(dir);
        store
            .load_range(&key("BTCUSDT", "1"), None, None, 10)
            .unwrap();
        let outcome = store.flush_key(&key("BTCUSDT", "1")).unwrap();
        assert!(outcome.compacted);
        assert_eq!(physical_lines(&path), 10_000);
    }

    #[test]
    fn compaction_keeps_newest_ten_thousand_unique_bars() {
        let dir = test_dir("compact-limit");
        let store = KlineStore::with_dir(dir.clone());
        let key = key("BTCUSDT", "1");
        let bars = (1..=10_250)
            .map(|time| sample(time, "1"))
            .collect::<Vec<_>>();
        store.upsert_bars(&key, &bars).unwrap();
        let outcome = store.flush_key(&key).unwrap();
        assert!(outcome.compacted);
        let persisted = fs::read_to_string(dir.join("BTCUSDT_1.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Kline>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(persisted.len(), 10_000);
        assert_eq!(persisted.first().unwrap().open_time, 251);
        assert_eq!(persisted.last().unwrap().open_time, 10_250);
    }

    #[test]
    fn explicit_and_periodic_compaction_serialize_per_key() {
        let dir = test_dir("compaction-single-flight");
        let path = dir.join("BTCUSDT_1.jsonl");
        append_raw(
            &path,
            &(1..=12_001)
                .map(|time| serialized(&sample(time, "1")))
                .collect::<Vec<_>>(),
        );
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let active_hook = Arc::clone(&active);
        let maximum_hook = Arc::clone(&maximum);
        let hook = Arc::new(move |_key: &ChartWorkspaceKey, _phase: FlushPhase| {
            let current = active_hook.fetch_add(1, Ordering::SeqCst) + 1;
            maximum_hook.fetch_max(current, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(30));
            active_hook.fetch_sub(1, Ordering::SeqCst);
        });
        let store = Arc::new(KlineStore::with_dir_and_flush_hook(dir, hook));
        let key = key("BTCUSDT", "1");
        store.load_range(&key, None, None, 1).unwrap();
        let first_store = Arc::clone(&store);
        let first_key = key.clone();
        let first = thread::spawn(move || first_store.flush_key(&first_key));
        let second_store = Arc::clone(&store);
        let second_key = key.clone();
        let second = thread::spawn(move || second_store.flush_key(&second_key));
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn flush_dirty_attempts_keys_after_a_failure() {
        let dir = test_dir("flush-dirty-errors");
        let store = KlineStore::with_dir(dir.clone());
        let bad = key("AAA", "1");
        let good = key("BBB", "1");
        store
            .upsert_bars(&bad, &[sample_for("AAA", "1", 1_000, "1")])
            .unwrap();
        store
            .upsert_bars(&good, &[sample_for("BBB", "1", 1_000, "1")])
            .unwrap();
        fs::create_dir(dir.join("AAA_1.jsonl")).unwrap();
        let outcomes = store.flush_dirty();
        assert!(outcomes
            .iter()
            .find(|(key, _)| key == &bad)
            .unwrap()
            .1
            .is_err());
        assert!(outcomes
            .iter()
            .find(|(key, _)| key == &good)
            .unwrap()
            .1
            .is_ok());
        assert!(dir.join("BBB_1.jsonl").is_file());
    }

    #[test]
    fn mismatched_bar_key_is_rejected_without_mutation() {
        let store = KlineStore::with_dir(test_dir("reject-mismatch"));
        let key = key("BTCUSDT", "1");
        store.upsert_bars(&key, &[sample(1_000, "1")]).unwrap();
        assert!(store
            .upsert_bars(&key, &[sample_for("ETHUSDT", "15", 2_000, "2")])
            .is_err());
        assert_eq!(
            store.load_range(&key, None, None, 10).unwrap(),
            vec![sample(1_000, "1")]
        );
    }

    #[test]
    fn mismatched_persisted_lines_are_skipped() {
        let dir = test_dir("skip-mismatch");
        append_raw(
            &dir.join("BTCUSDT_1.jsonl"),
            &[
                serialized(&sample(1_000, "1")),
                serialized(&sample_for("ETHUSDT", "15", 2_000, "2")),
            ],
        );
        let store = KlineStore::with_dir(dir);
        assert_eq!(
            store
                .load_range(&key("BTCUSDT", "1"), None, None, 10)
                .unwrap(),
            vec![sample(1_000, "1")]
        );
    }

    #[test]
    fn absent_main_recovers_fully_parseable_tmp_and_marks_it_for_compaction() {
        let dir = test_dir("recover-tmp");
        append_raw(
            &dir.join("BTCUSDT_1.jsonl.tmp"),
            &[serialized(&sample(1_000, "1"))],
        );
        let store = KlineStore::with_dir(dir.clone());
        let key = key("BTCUSDT", "1");
        assert_eq!(store.load_range(&key, None, None, 10).unwrap().len(), 1);
        assert!(store.flush_key(&key).unwrap().compacted);
        assert!(dir.join("BTCUSDT_1.jsonl").is_file());
    }

    #[test]
    fn empty_main_ignores_invalid_tmp_and_recovers_backup() {
        let dir = test_dir("recover-backup");
        fs::write(dir.join("BTCUSDT_1.jsonl"), "").unwrap();
        fs::write(dir.join("BTCUSDT_1.jsonl.tmp"), "not-json\n").unwrap();
        append_raw(
            &dir.join("BTCUSDT_1.jsonl.bak"),
            &[serialized(&sample(2_000, "2"))],
        );
        let store = KlineStore::with_dir(dir.clone());
        let key = key("BTCUSDT", "1");
        assert_eq!(
            store.load_range(&key, None, None, 10).unwrap(),
            vec![sample(2_000, "2")]
        );
        assert!(store.flush_key(&key).unwrap().compacted);
        assert_eq!(
            serde_json::from_str::<Kline>(
                fs::read_to_string(dir.join("BTCUSDT_1.jsonl"))
                    .unwrap()
                    .trim()
            )
            .unwrap(),
            sample(2_000, "2")
        );
    }

    #[test]
    fn valid_main_wins_over_tmp_and_backup() {
        let dir = test_dir("main-wins");
        append_raw(
            &dir.join("BTCUSDT_1.jsonl"),
            &[serialized(&sample(3_000, "3"))],
        );
        append_raw(
            &dir.join("BTCUSDT_1.jsonl.tmp"),
            &[serialized(&sample(2_000, "2"))],
        );
        append_raw(
            &dir.join("BTCUSDT_1.jsonl.bak"),
            &[serialized(&sample(1_000, "1"))],
        );
        let store = KlineStore::with_dir(dir);
        assert_eq!(
            store
                .load_range(&key("BTCUSDT", "1"), None, None, 10)
                .unwrap(),
            vec![sample(3_000, "3")]
        );
    }

    #[test]
    fn evicted_pending_bars_are_not_appended_before_compaction() {
        let dir = test_dir("pending-crop");
        let store = KlineStore::with_dir(dir.clone());
        let key = key("BTCUSDT", "1");
        store
            .upsert_bars(
                &key,
                &(1..=10_250)
                    .map(|time| sample(time, "1"))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        store.flush_key(&key).unwrap();
        let persisted = fs::read_to_string(dir.join("BTCUSDT_1.jsonl")).unwrap();
        assert!(!persisted.lines().any(|line| {
            serde_json::from_str::<Kline>(line)
                .map(|bar| bar.open_time <= 250)
                .unwrap_or(false)
        }));
    }

    #[test]
    fn load_range_rejects_reversed_bounds_and_clamps_zero_limit() {
        let store = seeded_store("range-validation", 3);
        let key = key("BTCUSDT", "1");
        assert!(store
            .load_range(&key, Some(3_000), Some(2_000), 10)
            .is_err());
        assert_eq!(store.load_range(&key, None, None, 0).unwrap().len(), 1);
    }

    #[test]
    fn last_open_time_uses_the_buffered_series() {
        let store = seeded_store("last-open", 3);
        assert_eq!(
            store.last_open_time(&key("BTCUSDT", "1")).unwrap(),
            Some(3_000)
        );
    }
}
