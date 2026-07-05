use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use crate::error::{AppError, AppResult};
use crate::models::config::APP_NAME;
use crate::models::market::Kline;

const MAX_STORED_BARS: usize = 2000;

pub struct KlineStore {
    dir: PathBuf,
}

impl KlineStore {
    pub fn new() -> Self {
        let dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(APP_NAME)
            .join("klines");
        let _ = fs::create_dir_all(&dir);
        Self { dir }
    }

    fn file_path(&self, symbol: &str, interval: &str) -> PathBuf {
        self.dir.join(format!("{symbol}_{interval}.jsonl"))
    }

    pub fn load(&self, symbol: &str, interval: &str) -> AppResult<Vec<Kline>> {
        let path = self.file_path(symbol, interval);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&path).map_err(|e| AppError::Internal(e.to_string()))?;
        let reader = BufReader::new(file);
        let mut map: BTreeMap<i64, Kline> = BTreeMap::new();
        for line in reader.lines() {
            let line = line.map_err(|e| AppError::Internal(e.to_string()))?;
            if line.trim().is_empty() {
                continue;
            }
            let kline: Kline =
                serde_json::from_str(&line).map_err(|e| AppError::Internal(e.to_string()))?;
            if kline.open_time > 0 {
                map.insert(kline.open_time, kline);
            }
        }
        Ok(map.into_values().collect())
    }

    pub fn upsert_bars(&self, symbol: &str, interval: &str, bars: &[Kline]) -> AppResult<Vec<Kline>> {
        let mut map: BTreeMap<i64, Kline> = self
            .load(symbol, interval)?
            .into_iter()
            .map(|k| (k.open_time, k))
            .collect();
        for bar in bars {
            if bar.open_time > 0 {
                map.insert(bar.open_time, bar.clone());
            }
        }
        let mut series: Vec<Kline> = map.into_values().collect();
        if series.len() > MAX_STORED_BARS {
            let start = series.len() - MAX_STORED_BARS;
            series = series.split_off(start);
        }
        self.write_series(symbol, interval, &series)?;
        Ok(series)
    }

    fn write_series(&self, symbol: &str, interval: &str, series: &[Kline]) -> AppResult<()> {
        let path = self.file_path(symbol, interval);
        let mut file = File::create(&path).map_err(|e| AppError::Internal(e.to_string()))?;
        for kline in series {
            let line =
                serde_json::to_string(kline).map_err(|e| AppError::Internal(e.to_string()))?;
            writeln!(file, "{line}").map_err(|e| AppError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    pub fn last_open_time(&self, symbol: &str, interval: &str) -> Option<i64> {
        self.load(symbol, interval)
            .ok()?
            .last()
            .map(|k| k.open_time)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(open_time: i64) -> Kline {
        Kline {
            symbol: "BTCUSDT".into(),
            interval: "1".into(),
            open_time,
            open: "1".into(),
            high: "1".into(),
            low: "1".into(),
            close: "1".into(),
            volume: "1".into(),
        }
    }

    #[test]
    fn detect_gaps_finds_missing_intervals() {
        let series = vec![sample(1_000), sample(121_000)];
        let gaps = KlineStore::detect_gaps(&series, 60_000);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0], (61_000, 61_000));
    }
}
