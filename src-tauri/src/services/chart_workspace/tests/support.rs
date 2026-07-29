use std::path::PathBuf;
use std::sync::Arc;

use crate::models::chart_workspace::{
    ChartPreferencesV1, ChartViewStateV1, ChartViewportSnapshot, ChartWorkspaceKey,
    SaveChartWorkspaceRequest, CHART_WORKSPACE_SCHEMA_VERSION,
};
use crate::models::market::Kline;
use crate::storage::chart_state_store::StateFileKind;
use crate::storage::{ChartStateStore, KlineStore};

use super::super::ChartWorkspaceService;

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(super) enum Failures {
    Kline,
    View,
    Preferences,
}

pub(super) struct Fixture {
    pub root: PathBuf,
    pub key: ChartWorkspaceKey,
    pub kline_store: Arc<KlineStore>,
    pub state_store: Arc<ChartStateStore>,
    pub service: ChartWorkspaceService,
}

impl Fixture {
    pub fn new(label: &str) -> Self {
        Self::build(label, None, Arc::new(|_| {}))
    }

    pub fn with_reporter(label: &str, reporter: Arc<dyn Fn(String) + Send + Sync>) -> Self {
        Self::build(label, None, reporter)
    }

    pub fn with_state_write_hook(
        label: &str,
        hook: Arc<dyn Fn(StateFileKind) + Send + Sync>,
    ) -> Self {
        Self::build(label, Some(hook), Arc::new(|_| {}))
    }

    fn build(
        label: &str,
        write_hook: Option<Arc<dyn Fn(StateFileKind) + Send + Sync>>,
        reporter: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Self {
        let root = test_root(label);
        let key = ChartWorkspaceKey::parse("BTCUSDT", "1").unwrap();
        let kline_store = Arc::new(KlineStore::with_dir(root.join("klines")));
        let state_root = root.join("state");
        let state_store = Arc::new(match write_hook {
            Some(hook) => ChartStateStore::with_root_and_write_hook(state_root, hook),
            None => ChartStateStore::with_root(state_root),
        });
        let service = ChartWorkspaceService::new(
            Arc::clone(&kline_store),
            Arc::clone(&state_store),
            reporter,
        );
        Self {
            root,
            key,
            kline_store,
            state_store,
            service,
        }
    }

    pub fn with_kline_count(self, count: i64) -> Self {
        let bars = (1..=count).map(sample_kline).collect::<Vec<_>>();
        self.kline_store.upsert_bars(&self.key, &bars).unwrap();
        self
    }

    #[allow(dead_code)]
    pub fn with_failures(label: &str, failure: Failures) -> Self {
        let fixture = Self::new(label);
        fixture
            .kline_store
            .upsert_bars(&fixture.key, &[sample_kline(1)])
            .unwrap();
        let path = match failure {
            Failures::Kline => fixture.root.join("klines").join("BTCUSDT_1.jsonl"),
            Failures::View => fixture.state_store.view_path_for_test(&fixture.key),
            Failures::Preferences => fixture.state_store.preferences_path_for_test(),
        };
        std::fs::create_dir_all(path).unwrap();
        fixture
    }

    #[allow(dead_code)]
    pub fn load_view(&self) -> ChartViewStateV1 {
        self.state_store.load_view(&self.key).unwrap().unwrap()
    }

    pub fn load_preferences(&self) -> ChartPreferencesV1 {
        self.state_store.load_preferences().unwrap().unwrap()
    }

    pub fn corrupt_all_view_candidates(&self) {
        for path in [
            self.state_store.view_path_for_test(&self.key),
            self.state_store.view_temp_path_for_test(&self.key),
            self.state_store.view_backup_path_for_test(&self.key),
        ] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "not json").unwrap();
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn test_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "easiflux-chart-workspace-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

pub(super) fn sample_kline(open_time: i64) -> Kline {
    Kline {
        symbol: "BTCUSDT".into(),
        interval: "1".into(),
        open_time,
        open: "1".into(),
        high: "2".into(),
        low: "0.5".into(),
        close: "1.5".into(),
        volume: "10".into(),
    }
}

pub(super) fn request(
    key: &ChartWorkspaceKey,
    view_revision: u64,
    preferences_revision: u64,
    saved_at_ms: i64,
) -> SaveChartWorkspaceRequest {
    SaveChartWorkspaceRequest {
        key: key.clone(),
        view_state: Some(ChartViewStateV1 {
            schema_version: CHART_WORKSPACE_SCHEMA_VERSION,
            symbol: key.symbol.clone(),
            interval: key.interval.clone(),
            revision: view_revision,
            saved_at_ms,
            overlays: Vec::new(),
            viewport: ChartViewportSnapshot::default(),
        }),
        preferences: Some(ChartPreferencesV1 {
            schema_version: CHART_WORKSPACE_SCHEMA_VERSION,
            revision: preferences_revision,
            saved_at_ms,
            main_indicators: vec!["MA".into(), "EMA".into()],
            sub_indicators: vec!["VOL".into(), "MACD".into()],
        }),
    }
}

pub(super) fn view_only_request(
    key: &ChartWorkspaceKey,
    revision: u64,
    marker: i64,
) -> SaveChartWorkspaceRequest {
    let mut request = request(key, revision, 0, 0);
    request.preferences = None;
    request
        .view_state
        .as_mut()
        .unwrap()
        .viewport
        .right_timestamp = Some(marker);
    request
}

pub(super) fn preferences_only_request(
    key: &ChartWorkspaceKey,
    revision: u64,
    marker: &str,
) -> SaveChartWorkspaceRequest {
    let mut request = request(key, 0, revision, 0);
    request.view_state = None;
    request
        .preferences
        .as_mut()
        .unwrap()
        .main_indicators
        .push(marker.into());
    request
}
