use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::{ChartStateStore, StateFileKind};
use crate::error::AppError;
use crate::models::chart_workspace::{
    ChartPreferencesV1, ChartViewStateV1, ChartViewportSnapshot, ChartWorkspaceKey,
    CHART_WORKSPACE_SCHEMA_VERSION,
};

static TEST_ROOT_ID: AtomicU64 = AtomicU64::new(0);

fn test_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "easiflux-chart-state-{}-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed),
        label,
    ))
}

fn cleanup(root: &Path) {
    let _ = std::fs::remove_dir_all(root);
}

fn key(symbol: &str, interval: &str) -> ChartWorkspaceKey {
    ChartWorkspaceKey::parse(symbol, interval).unwrap()
}

fn sample_view(symbol: &str, interval: &str, revision: u64) -> ChartViewStateV1 {
    let key = key(symbol, interval);
    ChartViewStateV1 {
        schema_version: CHART_WORKSPACE_SCHEMA_VERSION,
        symbol: key.symbol,
        interval: key.interval,
        revision,
        saved_at_ms: 0,
        overlays: Vec::new(),
        viewport: ChartViewportSnapshot::default(),
    }
}

fn sample_preferences(revision: u64) -> ChartPreferencesV1 {
    ChartPreferencesV1 {
        schema_version: CHART_WORKSPACE_SCHEMA_VERSION,
        revision,
        saved_at_ms: 0,
        main_indicators: vec!["MA".into(), "EMA".into()],
        sub_indicators: vec!["VOL".into(), "MACD".into()],
    }
}

fn corrupt_main_temp_and_backup(store: &ChartStateStore, key: &ChartWorkspaceKey) {
    for path in [
        store.view_path_for_test(key),
        store.view_temp_path_for_test(key),
        store.view_backup_path_for_test(key),
    ] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "not json").unwrap();
    }
}

#[test]
fn view_state_round_trips_under_the_normalized_key() {
    let root = test_root("view-round-trip");
    let store = ChartStateStore::with_root(root.clone());
    let state = sample_view("BTCUSDT", "15", 3);
    store.save_view(&state).unwrap();
    assert_eq!(store.load_view(&key("BTCUSDT", "15")).unwrap(), Some(state));
    cleanup(&root);
}

#[test]
fn preferences_are_global_and_independent_of_view_files() {
    let root = test_root("preferences");
    let store = ChartStateStore::with_root(root.clone());
    let preferences = sample_preferences(4);
    store.save_preferences(&preferences).unwrap();
    assert_eq!(store.load_preferences().unwrap(), Some(preferences));
    assert_eq!(
        store.preferences_path_for_test(),
        root.join("preferences.v1.json")
    );
    assert!(!root.join("views").join("preferences.v1.json").exists());
    cleanup(&root);
}

#[test]
fn corrupt_main_recovers_valid_backup() {
    let root = test_root("backup-recovery");
    let store = ChartStateStore::with_root(root.clone());
    store.save_view(&sample_view("BTCUSDT", "15", 1)).unwrap();
    store.save_view(&sample_view("BTCUSDT", "15", 2)).unwrap();
    std::fs::write(store.view_path_for_test(&key("BTCUSDT", "15")), "not json").unwrap();
    assert_eq!(
        store
            .load_view(&key("BTCUSDT", "15"))
            .unwrap()
            .unwrap()
            .revision,
        1
    );
    cleanup(&root);
}

#[test]
fn all_corrupt_candidates_return_storage_error() {
    let root = test_root("all-corrupt");
    let store = ChartStateStore::with_root(root.clone());
    corrupt_main_temp_and_backup(&store, &key("BTCUSDT", "15"));
    assert!(matches!(
        store.load_view(&key("BTCUSDT", "15")),
        Err(AppError::Storage(_))
    ));
    cleanup(&root);
}

#[test]
fn save_after_backup_recovery_preserves_a_last_good_candidate() {
    let root = test_root("save-after-recovery");
    let store = ChartStateStore::with_root(root.clone());
    store.save_view(&sample_view("BTCUSDT", "15", 1)).unwrap();
    store.save_view(&sample_view("BTCUSDT", "15", 2)).unwrap();
    std::fs::write(store.view_path_for_test(&key("BTCUSDT", "15")), "not json").unwrap();
    store.save_view(&sample_view("BTCUSDT", "15", 3)).unwrap();
    assert_eq!(
        store
            .load_view(&key("BTCUSDT", "15"))
            .unwrap()
            .unwrap()
            .revision,
        3
    );
    assert_eq!(
        store
            .load_backup_view_for_test(&key("BTCUSDT", "15"))
            .unwrap()
            .revision,
        1
    );
    cleanup(&root);
}

#[test]
fn missing_files_return_none() {
    let root = test_root("missing");
    let store = ChartStateStore::with_root(root.clone());

    assert_eq!(store.load_view(&key("BTCUSDT", "15")).unwrap(), None);
    assert_eq!(store.load_preferences().unwrap(), None);

    cleanup(&root);
}

#[test]
fn unknown_schema_is_rejected() {
    let root = test_root("unknown-schema");
    let store = ChartStateStore::with_root(root.clone());
    let mut state = sample_view("BTCUSDT", "15", 1);
    state.schema_version = CHART_WORKSPACE_SCHEMA_VERSION + 1;
    let path = store.view_path_for_test(&key("BTCUSDT", "15"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    assert!(matches!(
        store.load_view(&key("BTCUSDT", "15")),
        Err(AppError::Storage(_))
    ));

    cleanup(&root);
}

#[test]
fn embedded_key_mismatch_is_rejected() {
    let root = test_root("embedded-key-mismatch");
    let store = ChartStateStore::with_root(root.clone());
    let path = store.view_path_for_test(&key("BTCUSDT", "15"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&sample_view("ETHUSDT", "15", 1)).unwrap(),
    )
    .unwrap();

    assert!(matches!(
        store.load_view(&key("BTCUSDT", "15")),
        Err(AppError::Storage(_))
    ));

    cleanup(&root);
}

#[test]
fn failed_main_promotion_keeps_a_readable_candidate() {
    let root = test_root("failed-promotion");
    let initial_store = ChartStateStore::with_root(root.clone());
    let workspace_key = key("BTCUSDT", "15");
    initial_store
        .save_view(&sample_view("BTCUSDT", "15", 1))
        .unwrap();
    initial_store
        .save_view(&sample_view("BTCUSDT", "15", 2))
        .unwrap();

    let backup_path = initial_store.view_backup_path_for_test(&workspace_key);
    let hooked_key = workspace_key.clone();
    let hooked_store = ChartStateStore::with_root_and_write_hook(
        root.clone(),
        Arc::new(move |kind| {
            if let StateFileKind::View(key) = kind {
                assert_eq!(key, hooked_key);
                std::fs::remove_file(&backup_path).unwrap();
                std::fs::create_dir(&backup_path).unwrap();
            }
        }),
    );

    assert!(matches!(
        hooked_store.save_view(&sample_view("BTCUSDT", "15", 3)),
        Err(AppError::Storage(_))
    ));
    let recovered = initial_store.load_view(&workspace_key).unwrap().unwrap();
    assert!([1, 2].contains(&recovered.revision));

    cleanup(&root);
}
