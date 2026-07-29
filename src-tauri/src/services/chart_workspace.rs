use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{AppError, AppResult};
use crate::models::chart_workspace::{
    ChartPreferencesV1, ChartViewStateV1, ChartViewportSnapshot, ChartWorkspaceKey,
    ChartWorkspaceSaveResult, ChartWorkspaceSnapshot, SaveChartWorkspaceRequest,
    CHART_WORKSPACE_SCHEMA_VERSION, DEFAULT_CHART_LOAD_LIMIT, MAX_CHART_KLINES,
};
use crate::storage::kline_store::KlineFlushOutcome;
use crate::storage::{ChartStateStore, KlineStore};

mod diagnostics;

use diagnostics::{ChartDiagnosticKey, ChartWorkspaceDiagnostics};

pub struct ChartWorkspaceService {
    kline_store: Arc<KlineStore>,
    state_store: Arc<ChartStateStore>,
    view_locks: Mutex<HashMap<ChartWorkspaceKey, Arc<Mutex<()>>>>,
    preferences_lock: Mutex<()>,
    diagnostics: ChartWorkspaceDiagnostics,
}

impl ChartWorkspaceService {
    pub fn new(
        kline_store: Arc<KlineStore>,
        state_store: Arc<ChartStateStore>,
        reporter: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Self {
        Self {
            kline_store,
            state_store,
            view_locks: Mutex::new(HashMap::new()),
            preferences_lock: Mutex::new(()),
            diagnostics: ChartWorkspaceDiagnostics::new(reporter),
        }
    }

    pub fn load(
        &self,
        key: &ChartWorkspaceKey,
        from: Option<i64>,
        to: Option<i64>,
        limit: Option<u32>,
    ) -> AppResult<ChartWorkspaceSnapshot> {
        let key =
            ChartWorkspaceKey::parse(&key.symbol, &key.interval).map_err(AppError::Storage)?;
        if matches!((from, to), (Some(from), Some(to)) if from > to) {
            return Err(AppError::Storage("kline range start exceeds end".into()));
        }
        let default_limit = if from.is_none() && to.is_none() {
            DEFAULT_CHART_LOAD_LIMIT
        } else {
            MAX_CHART_KLINES
        };
        let limit = limit
            .map(|value| value.clamp(1, MAX_CHART_KLINES as u32) as usize)
            .unwrap_or(default_limit);
        let klines = self.kline_store.load_range(&key, from, to, limit)?;
        let view_state = self.load_view_or_default(&key);
        let preferences = self.load_preferences_or_default();

        Ok(ChartWorkspaceSnapshot {
            key,
            klines,
            view_state,
            preferences,
        })
    }

    pub fn save(
        &self,
        mut request: SaveChartWorkspaceRequest,
    ) -> AppResult<ChartWorkspaceSaveResult> {
        let key = ChartWorkspaceKey::parse(&request.key.symbol, &request.key.interval)
            .map_err(AppError::Storage)?;
        validate_save_request(&request, &key)?;
        request.key = key.clone();
        let now_ms = now_ms();

        let kline_result = self.flush_kline_key(&key);
        let kline_wrote = kline_result.as_ref().is_ok_and(|outcome| outcome.wrote);
        let (kline_saved, kline_error) = match kline_result {
            Ok(_) => (true, None),
            Err(error) => (false, Some(error.to_string())),
        };

        let view = self.save_view_part(&key, request.view_state, now_ms);
        let preferences = self.save_preferences_part(request.preferences, now_ms);
        let wrote_bytes = kline_wrote || view.wrote || preferences.wrote;
        let saved_at_ms = if wrote_bytes {
            now_ms
        } else {
            view.saved_at_ms.max(preferences.saved_at_ms)
        };

        Ok(ChartWorkspaceSaveResult {
            key,
            view_revision: view.revision,
            preferences_revision: preferences.revision,
            saved_at_ms,
            kline_saved,
            view_state_saved: view.saved,
            preferences_saved: preferences.saved,
            kline_error,
            view_state_error: view.error,
            preferences_error: preferences.error,
        })
    }

    pub fn flush_kline_key(&self, key: &ChartWorkspaceKey) -> AppResult<KlineFlushOutcome> {
        let result = self.kline_store.flush_key(key);
        self.observe_kline_result(key, &result);
        result
    }

    pub fn flush_dirty_klines(&self) -> Vec<(ChartWorkspaceKey, AppResult<KlineFlushOutcome>)> {
        let outcomes = self.kline_store.flush_dirty();
        for (key, result) in &outcomes {
            self.observe_kline_result(key, result);
        }
        outcomes
    }

    fn load_view_or_default(&self, key: &ChartWorkspaceKey) -> ChartViewStateV1 {
        let diagnostic_key = ChartDiagnosticKey::View(key.clone());
        match self.state_store.load_view(key) {
            Ok(state) => {
                self.diagnostics.clear(&diagnostic_key);
                state.unwrap_or_else(|| default_view_state(key))
            }
            Err(error) => {
                self.diagnostics.report_once(
                    diagnostic_key,
                    format!(
                        "chart view state could not be read for {}_{}: {error}",
                        key.symbol, key.interval
                    ),
                );
                default_view_state(key)
            }
        }
    }

    fn load_preferences_or_default(&self) -> ChartPreferencesV1 {
        let diagnostic_key = ChartDiagnosticKey::Preferences;
        match self.state_store.load_preferences() {
            Ok(preferences) => {
                self.diagnostics.clear(&diagnostic_key);
                preferences.unwrap_or_else(default_preferences)
            }
            Err(error) => {
                self.diagnostics.report_once(
                    diagnostic_key,
                    format!("chart preferences could not be read: {error}"),
                );
                default_preferences()
            }
        }
    }

    fn save_view_part(
        &self,
        key: &ChartWorkspaceKey,
        requested: Option<ChartViewStateV1>,
        now_ms: i64,
    ) -> PartSaveResult {
        let view_lock = {
            let mut locks = self
                .view_locks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Arc::clone(
                locks
                    .entry(key.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _view_guard = view_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let diagnostic_key = ChartDiagnosticKey::View(key.clone());
        let durable = match self.state_store.load_view(key) {
            Ok(durable) => {
                self.diagnostics.clear(&diagnostic_key);
                durable
            }
            Err(error) => {
                self.diagnostics.report_once(
                    diagnostic_key.clone(),
                    format!(
                        "chart view state could not be read for {}_{}: {error}",
                        key.symbol, key.interval
                    ),
                );
                None
            }
        };
        let durable_revision = durable.as_ref().map_or(0, |state| state.revision);
        let durable_saved_at = durable.as_ref().map_or(0, |state| state.saved_at_ms);
        let Some(mut requested) = requested else {
            return PartSaveResult::success(durable_revision, durable_saved_at, false);
        };

        if let Some(durable) = durable.as_ref() {
            if requested.revision < durable.revision {
                return PartSaveResult::conflict(
                    durable_revision,
                    durable_saved_at,
                    revision_conflict("view", durable.revision, requested.revision),
                );
            }
            if requested.revision == durable.revision {
                if same_view_content(durable, &requested) {
                    return PartSaveResult::success(durable_revision, durable_saved_at, false);
                }
                return PartSaveResult::conflict(
                    durable_revision,
                    durable_saved_at,
                    revision_conflict("view", durable.revision, requested.revision),
                );
            }
        }

        requested.saved_at_ms = now_ms;
        match self.state_store.save_view(&requested) {
            Ok(()) => {
                self.diagnostics.clear(&diagnostic_key);
                PartSaveResult::success(requested.revision, now_ms, true)
            }
            Err(error) => {
                self.diagnostics.report_once(
                    diagnostic_key,
                    format!(
                        "chart view state could not be saved for {}_{}: {error}",
                        key.symbol, key.interval
                    ),
                );
                PartSaveResult::failed(durable_revision, durable_saved_at, error.to_string())
            }
        }
    }

    fn save_preferences_part(
        &self,
        requested: Option<ChartPreferencesV1>,
        now_ms: i64,
    ) -> PartSaveResult {
        let _preferences_guard = self
            .preferences_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let diagnostic_key = ChartDiagnosticKey::Preferences;
        let durable = match self.state_store.load_preferences() {
            Ok(durable) => {
                self.diagnostics.clear(&diagnostic_key);
                durable
            }
            Err(error) => {
                self.diagnostics.report_once(
                    diagnostic_key.clone(),
                    format!("chart preferences could not be read: {error}"),
                );
                None
            }
        };
        let durable_revision = durable
            .as_ref()
            .map_or(0, |preferences| preferences.revision);
        let durable_saved_at = durable
            .as_ref()
            .map_or(0, |preferences| preferences.saved_at_ms);
        let Some(mut requested) = requested else {
            return PartSaveResult::success(durable_revision, durable_saved_at, false);
        };

        if let Some(durable) = durable.as_ref() {
            if requested.revision < durable.revision {
                return PartSaveResult::conflict(
                    durable_revision,
                    durable_saved_at,
                    revision_conflict("preferences", durable.revision, requested.revision),
                );
            }
            if requested.revision == durable.revision {
                if same_preferences_content(durable, &requested) {
                    return PartSaveResult::success(durable_revision, durable_saved_at, false);
                }
                return PartSaveResult::conflict(
                    durable_revision,
                    durable_saved_at,
                    revision_conflict("preferences", durable.revision, requested.revision),
                );
            }
        }

        requested.saved_at_ms = now_ms;
        match self.state_store.save_preferences(&requested) {
            Ok(()) => {
                self.diagnostics.clear(&diagnostic_key);
                PartSaveResult::success(requested.revision, now_ms, true)
            }
            Err(error) => {
                self.diagnostics.report_once(
                    diagnostic_key,
                    format!("chart preferences could not be saved: {error}"),
                );
                PartSaveResult::failed(durable_revision, durable_saved_at, error.to_string())
            }
        }
    }

    fn observe_kline_result(&self, key: &ChartWorkspaceKey, result: &AppResult<KlineFlushOutcome>) {
        let diagnostic_key = ChartDiagnosticKey::Kline(key.clone());
        match result {
            Ok(_) => self.diagnostics.clear(&diagnostic_key),
            Err(error) => self.diagnostics.report_once(
                diagnostic_key,
                format!(
                    "chart kline buffer could not be flushed for {}_{}: {error}",
                    key.symbol, key.interval
                ),
            ),
        }
    }
}

struct PartSaveResult {
    saved: bool,
    revision: u64,
    saved_at_ms: i64,
    wrote: bool,
    error: Option<String>,
}

impl PartSaveResult {
    fn success(revision: u64, saved_at_ms: i64, wrote: bool) -> Self {
        Self {
            saved: true,
            revision,
            saved_at_ms,
            wrote,
            error: None,
        }
    }

    fn conflict(revision: u64, saved_at_ms: i64, error: String) -> Self {
        Self {
            saved: false,
            revision,
            saved_at_ms,
            wrote: false,
            error: Some(error),
        }
    }

    fn failed(revision: u64, saved_at_ms: i64, error: String) -> Self {
        Self::conflict(revision, saved_at_ms, error)
    }
}

fn validate_save_request(
    request: &SaveChartWorkspaceRequest,
    key: &ChartWorkspaceKey,
) -> AppResult<()> {
    if let Some(view_state) = &request.view_state {
        validate_schema(view_state.schema_version)?;
        if view_state.symbol != key.symbol || view_state.interval != key.interval {
            return Err(AppError::Storage(format!(
                "chart state key mismatch: expected {}_{}, found {}_{}",
                key.symbol, key.interval, view_state.symbol, view_state.interval
            )));
        }
    }
    if let Some(preferences) = &request.preferences {
        validate_schema(preferences.schema_version)?;
    }
    Ok(())
}

fn validate_schema(schema_version: u32) -> AppResult<()> {
    if schema_version == CHART_WORKSPACE_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(AppError::Storage(format!(
            "unsupported chart state schema version {schema_version}"
        )))
    }
}

fn same_view_content(left: &ChartViewStateV1, right: &ChartViewStateV1) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.revision = 0;
    left.saved_at_ms = 0;
    right.revision = 0;
    right.saved_at_ms = 0;
    left == right
}

fn same_preferences_content(left: &ChartPreferencesV1, right: &ChartPreferencesV1) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.revision = 0;
    left.saved_at_ms = 0;
    right.revision = 0;
    right.saved_at_ms = 0;
    left == right
}

fn revision_conflict(part: &str, durable: u64, requested: u64) -> String {
    format!(
        "chart {part} revision conflict: durable revision {durable}, requested revision {requested}"
    )
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn default_view_state(key: &ChartWorkspaceKey) -> ChartViewStateV1 {
    ChartViewStateV1 {
        schema_version: CHART_WORKSPACE_SCHEMA_VERSION,
        symbol: key.symbol.clone(),
        interval: key.interval.clone(),
        revision: 0,
        saved_at_ms: 0,
        overlays: Vec::new(),
        viewport: ChartViewportSnapshot::default(),
    }
}

fn default_preferences() -> ChartPreferencesV1 {
    ChartPreferencesV1 {
        schema_version: CHART_WORKSPACE_SCHEMA_VERSION,
        revision: 0,
        saved_at_ms: 0,
        main_indicators: vec!["MA".into(), "EMA".into()],
        sub_indicators: vec!["VOL".into(), "MACD".into()],
    }
}

#[cfg(test)]
mod tests;
