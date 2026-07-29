use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::models::chart_workspace::{
    ChartWorkspaceKey, SaveChartWorkspaceRequest, CHART_WORKSPACE_SCHEMA_VERSION,
};
use crate::storage::chart_state_store::StateFileKind;

mod support;

use support::{preferences_only_request, request, view_only_request, Failures, Fixture};

#[test]
fn load_without_range_returns_latest_two_hundred_local_bars() {
    let fixture = Fixture::new("default-range").with_kline_count(240);
    let snapshot = fixture
        .service
        .load(&fixture.key, None, None, None)
        .unwrap();

    assert_eq!(snapshot.klines.len(), 200);
    assert_eq!(snapshot.klines.first().unwrap().open_time, 41);
}

#[test]
fn load_with_range_reads_the_full_local_history() {
    let fixture = Fixture::new("explicit-range").with_kline_count(600);
    let snapshot = fixture
        .service
        .load(&fixture.key, Some(101), Some(550), Some(500))
        .unwrap();

    assert_eq!(snapshot.klines.len(), 450);
    assert_eq!(snapshot.klines.first().unwrap().open_time, 101);
    assert_eq!(snapshot.klines.last().unwrap().open_time, 550);
}

#[test]
fn missing_state_returns_version_one_defaults() {
    let fixture = Fixture::new("defaults");
    let snapshot = fixture
        .service
        .load(&fixture.key, None, None, None)
        .unwrap();

    assert_eq!(snapshot.view_state.schema_version, 1);
    assert_eq!(snapshot.view_state.revision, 0);
    assert_eq!(snapshot.preferences.main_indicators, vec!["MA", "EMA"]);
}

#[test]
fn rust_replaces_client_saved_at_and_rejects_stale_revisions() {
    let fixture = Fixture::new("revision");
    let first = fixture
        .service
        .save(request(&fixture.key, 2, 3, 1))
        .unwrap();
    let stale = fixture
        .service
        .save(request(&fixture.key, 1, 2, 999_999))
        .unwrap();

    assert!(first.saved_at_ms > 0);
    assert!(!stale.view_state_saved);
    assert!(!stale.preferences_saved);
    assert_eq!(stale.view_revision, 2);
    assert_eq!(stale.preferences_revision, 3);
    assert_eq!(fixture.load_view().saved_at_ms, first.saved_at_ms);
}

#[test]
fn revision_first_matrix_covers_conflict_retry_and_newer_ack() {
    struct Row {
        name: &'static str,
        revision: u64,
        marker: i64,
        saved: bool,
        durable_revision: u64,
        writes: usize,
    }

    for row in [
        Row {
            name: "lower-equal-content-conflicts",
            revision: 3,
            marker: 10,
            saved: false,
            durable_revision: 4,
            writes: 0,
        },
        Row {
            name: "same-equal-content-is-no-op",
            revision: 4,
            marker: 10,
            saved: true,
            durable_revision: 4,
            writes: 0,
        },
        Row {
            name: "same-different-content-conflicts",
            revision: 4,
            marker: 20,
            saved: false,
            durable_revision: 4,
            writes: 0,
        },
        Row {
            name: "newer-equal-content-is-written-and-acked",
            revision: 5,
            marker: 10,
            saved: true,
            durable_revision: 5,
            writes: 1,
        },
    ] {
        let hook_calls = Arc::new(AtomicUsize::new(0));
        let hook_calls_for_hook = Arc::clone(&hook_calls);
        let fixture = Fixture::with_state_write_hook(
            row.name,
            Arc::new(move |kind| {
                if matches!(kind, StateFileKind::View(_)) {
                    hook_calls_for_hook.fetch_add(1, Ordering::SeqCst);
                }
            }),
        );
        fixture
            .state_store
            .save_view(
                view_only_request(&fixture.key, 4, 10)
                    .view_state
                    .as_ref()
                    .unwrap(),
            )
            .unwrap();
        hook_calls.store(0, Ordering::SeqCst);

        let result = fixture
            .service
            .save(view_only_request(&fixture.key, row.revision, row.marker))
            .unwrap();

        assert_eq!(result.view_state_saved, row.saved, "{}", row.name);
        assert_eq!(result.view_revision, row.durable_revision, "{}", row.name);
        assert_eq!(
            hook_calls.load(Ordering::SeqCst),
            row.writes,
            "{}",
            row.name
        );
    }
}

#[test]
fn one_failed_part_does_not_roll_back_successful_parts() {
    let fixture = Fixture::with_failures("partial", Failures::View);
    let result = fixture
        .service
        .save(request(&fixture.key, 4, 5, 0))
        .unwrap();

    assert!(result.kline_saved);
    assert!(!result.view_state_saved);
    assert!(result.preferences_saved);
    assert!(result.view_state_error.is_some());
    assert!(result.kline_error.is_none());
    assert!(result.preferences_error.is_none());
}

#[test]
fn same_key_saves_enter_the_view_write_hook_serially() {
    let entered = Arc::new(AtomicUsize::new(0));
    let entered_for_hook = Arc::clone(&entered);
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let release_rx = Arc::new(Mutex::new(release_rx));
    let release_for_hook = Arc::clone(&release_rx);
    let fixture = Arc::new(Fixture::with_state_write_hook(
        "same-key-serial",
        Arc::new(move |kind| {
            if matches!(kind, StateFileKind::View(_)) {
                let call = entered_for_hook.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    started_tx.send(()).unwrap();
                    release_for_hook.lock().unwrap().recv().unwrap();
                }
            }
        }),
    ));
    let first_fixture = Arc::clone(&fixture);
    let first = thread::spawn(move || {
        first_fixture
            .service
            .save(view_only_request(&first_fixture.key, 1, 10))
    });
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let second_fixture = Arc::clone(&fixture);
    let second = thread::spawn(move || {
        second_fixture
            .service
            .save(view_only_request(&second_fixture.key, 2, 20))
    });

    thread::sleep(Duration::from_millis(50));
    assert_eq!(entered.load(Ordering::SeqCst), 1);
    release_tx.send(()).unwrap();
    first.join().unwrap().unwrap();
    second.join().unwrap().unwrap();
    assert_eq!(entered.load(Ordering::SeqCst), 2);
}

#[test]
fn different_keys_keep_independent_view_files() {
    let fixture = Fixture::new("different-keys");
    let eth = ChartWorkspaceKey::parse("ETHUSDT", "1").unwrap();

    fixture
        .service
        .save(view_only_request(&fixture.key, 2, 101))
        .unwrap();
    fixture
        .service
        .save(view_only_request(&eth, 7, 202))
        .unwrap();

    let btc_state = fixture
        .state_store
        .load_view(&fixture.key)
        .unwrap()
        .unwrap();
    let eth_state = fixture.state_store.load_view(&eth).unwrap().unwrap();
    assert_eq!(btc_state.revision, 2);
    assert_eq!(btc_state.viewport.right_timestamp, Some(101));
    assert_eq!(eth_state.revision, 7);
    assert_eq!(eth_state.viewport.right_timestamp, Some(202));
}

#[test]
fn preferences_serialize_across_different_keys() {
    let entered = Arc::new(AtomicUsize::new(0));
    let entered_for_hook = Arc::clone(&entered);
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let release_rx = Arc::new(Mutex::new(release_rx));
    let release_for_hook = Arc::clone(&release_rx);
    let fixture = Arc::new(Fixture::with_state_write_hook(
        "preferences-serial",
        Arc::new(move |kind| {
            if kind == StateFileKind::Preferences {
                let call = entered_for_hook.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    started_tx.send(()).unwrap();
                    release_for_hook.lock().unwrap().recv().unwrap();
                }
            }
        }),
    ));
    let eth = ChartWorkspaceKey::parse("ETHUSDT", "1").unwrap();
    let first_fixture = Arc::clone(&fixture);
    let first = thread::spawn(move || {
        first_fixture
            .service
            .save(preferences_only_request(&first_fixture.key, 1, "BTC"))
    });
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let second_fixture = Arc::clone(&fixture);
    let second = thread::spawn(move || {
        second_fixture
            .service
            .save(preferences_only_request(&eth, 2, "ETH"))
    });

    thread::sleep(Duration::from_millis(50));
    assert_eq!(entered.load(Ordering::SeqCst), 1);
    release_tx.send(()).unwrap();
    first.join().unwrap().unwrap();
    second.join().unwrap().unwrap();
    assert_eq!(entered.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.load_preferences().revision, 2);
}

#[test]
fn omitted_parts_are_successful_durable_no_ops() {
    let hook_calls = Arc::new(AtomicUsize::new(0));
    let hook_calls_for_hook = Arc::clone(&hook_calls);
    let fixture = Fixture::with_state_write_hook(
        "omitted-no-op",
        Arc::new(move |_| {
            hook_calls_for_hook.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let mut seed = request(&fixture.key, 4, 5, 123);
    seed.view_state.as_mut().unwrap().saved_at_ms = 111;
    seed.preferences.as_mut().unwrap().saved_at_ms = 222;
    fixture
        .state_store
        .save_view(seed.view_state.as_ref().unwrap())
        .unwrap();
    fixture
        .state_store
        .save_preferences(seed.preferences.as_ref().unwrap())
        .unwrap();
    hook_calls.store(0, Ordering::SeqCst);

    let result = fixture
        .service
        .save(SaveChartWorkspaceRequest {
            key: fixture.key.clone(),
            view_state: None,
            preferences: None,
        })
        .unwrap();

    assert!(result.view_state_saved);
    assert!(result.preferences_saved);
    assert_eq!(result.view_revision, 4);
    assert_eq!(result.preferences_revision, 5);
    assert_eq!(result.saved_at_ms, 222);
    assert_eq!(hook_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn all_parts_are_attempted_after_one_failure() {
    for (label, failure) in [
        ("kline", Failures::Kline),
        ("view", Failures::View),
        ("preferences", Failures::Preferences),
    ] {
        let fixture = Fixture::with_failures(label, failure);
        let result = fixture
            .service
            .save(request(&fixture.key, 1, 1, 0))
            .unwrap();

        match failure {
            Failures::Kline => {
                assert!(!result.kline_saved);
                assert!(result.view_state_saved);
                assert!(result.preferences_saved);
                assert_eq!(fixture.load_view().revision, 1);
                assert_eq!(fixture.load_preferences().revision, 1);
            }
            Failures::View => {
                assert!(result.kline_saved);
                assert!(!result.view_state_saved);
                assert!(result.preferences_saved);
                assert!(fixture.root.join("klines/BTCUSDT_1.jsonl").is_file());
                assert_eq!(fixture.load_preferences().revision, 1);
            }
            Failures::Preferences => {
                assert!(result.kline_saved);
                assert!(result.view_state_saved);
                assert!(!result.preferences_saved);
                assert!(fixture.root.join("klines/BTCUSDT_1.jsonl").is_file());
                assert_eq!(fixture.load_view().revision, 1);
            }
        }
    }
}

#[test]
fn save_repairs_all_corrupt_state_candidates() {
    let fixture = Fixture::new("repair-corrupt");
    fixture.corrupt_all_view_candidates();

    let result = fixture
        .service
        .save(view_only_request(&fixture.key, 1, 101))
        .unwrap();

    assert!(result.view_state_saved);
    let loaded = fixture
        .service
        .load(&fixture.key, None, None, None)
        .unwrap();
    assert_eq!(loaded.view_state.revision, 1);
}

#[test]
fn response_lost_retry_is_content_equal_no_op() {
    let hook_calls = Arc::new(AtomicUsize::new(0));
    let hook_calls_for_hook = Arc::clone(&hook_calls);
    let fixture = Fixture::with_state_write_hook(
        "response-lost",
        Arc::new(move |_| {
            hook_calls_for_hook.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let first = fixture
        .service
        .save(request(&fixture.key, 4, 4, 999))
        .unwrap();
    let writes_after_first = hook_calls.load(Ordering::SeqCst);

    let retry = fixture
        .service
        .save(request(&fixture.key, 4, 4, 0))
        .unwrap();

    assert!(retry.view_state_saved);
    assert!(retry.preferences_saved);
    assert_eq!(retry.view_revision, 4);
    assert_eq!(retry.preferences_revision, 4);
    assert_eq!(retry.saved_at_ms, first.saved_at_ms);
    assert_eq!(hook_calls.load(Ordering::SeqCst), writes_after_first);
}

#[test]
fn save_revalidates_serde_constructed_key_before_disk() {
    let fixture = Fixture::new("invalid-key").with_kline_count(1);
    let invalid = ChartWorkspaceKey {
        symbol: "../BTCUSDT".into(),
        interval: "1".into(),
    };
    let result = fixture.service.save(request(&invalid, 1, 1, 0));

    assert!(result.is_err());
    assert!(!fixture.root.join("klines/BTCUSDT_1.jsonl").exists());
    assert!(fixture
        .state_store
        .load_view(&fixture.key)
        .unwrap()
        .is_none());
    assert!(fixture.state_store.load_preferences().unwrap().is_none());
}

#[test]
fn save_validates_all_payloads_before_attempting_disk() {
    let fixture = Fixture::new("invalid-payload").with_kline_count(1);
    let mut invalid = request(&fixture.key, 1, 1, 0);
    invalid.view_state.as_mut().unwrap().schema_version = CHART_WORKSPACE_SCHEMA_VERSION + 1;

    assert!(fixture.service.save(invalid).is_err());
    assert!(!fixture.root.join("klines/BTCUSDT_1.jsonl").exists());
    assert!(fixture.state_store.load_preferences().unwrap().is_none());
}

#[test]
fn kline_diagnostic_is_limited_until_a_successful_retry_rearms_it() {
    let reports = Arc::new(Mutex::new(Vec::new()));
    let reports_for_callback = Arc::clone(&reports);
    let fixture = Fixture::with_reporter(
        "diagnostic-rearm",
        Arc::new(move |message| reports_for_callback.lock().unwrap().push(message)),
    );
    fixture
        .kline_store
        .upsert_bars(&fixture.key, &[support::sample_kline(1)])
        .unwrap();
    let path = fixture.root.join("klines/BTCUSDT_1.jsonl");
    std::fs::create_dir_all(&path).unwrap();

    assert!(fixture.service.flush_kline_key(&fixture.key).is_err());
    assert!(fixture.service.flush_kline_key(&fixture.key).is_err());
    assert_eq!(reports.lock().unwrap().len(), 1);

    std::fs::remove_dir(&path).unwrap();
    fixture.service.flush_kline_key(&fixture.key).unwrap();
    fixture
        .kline_store
        .upsert_bars(&fixture.key, &[support::sample_kline(2)])
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(fixture.service.flush_kline_key(&fixture.key).is_err());
    assert_eq!(reports.lock().unwrap().len(), 2);
}

#[test]
fn corrupt_load_defaults_are_reported_once_and_successful_repair_clears_the_latch() {
    let reports = Arc::new(Mutex::new(Vec::new()));
    let reports_for_callback = Arc::clone(&reports);
    let fixture = Fixture::with_reporter(
        "corrupt-load-diagnostic",
        Arc::new(move |message| reports_for_callback.lock().unwrap().push(message)),
    );
    fixture.corrupt_all_view_candidates();

    let first = fixture
        .service
        .load(&fixture.key, None, None, None)
        .unwrap();
    let second = fixture
        .service
        .load(&fixture.key, None, None, None)
        .unwrap();
    assert_eq!(first.view_state.revision, 0);
    assert_eq!(second.view_state.revision, 0);
    assert_eq!(reports.lock().unwrap().len(), 1);

    fixture
        .service
        .save(view_only_request(&fixture.key, 1, 100))
        .unwrap();
    fixture.corrupt_all_view_candidates();
    fixture
        .service
        .load(&fixture.key, None, None, None)
        .unwrap();
    assert_eq!(reports.lock().unwrap().len(), 2);
}
