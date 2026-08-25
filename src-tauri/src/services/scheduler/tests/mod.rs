use super::*;
use std::sync::{Arc, Condvar, Mutex as StdMutex};

use crate::models::time::{TimeSnapshot, TimeSource, TimeSyncStatus};

mod coordination;
mod environment_notifications;
mod notification_maintenance;
mod rescheduling;
mod round4_lifecycle;
mod round5_lifecycle;

#[test]
fn task_id_parses_frontend_names() {
    assert_eq!(TaskId::from_name("dailyPnl"), Some(TaskId::DailyPnl));
    assert_eq!(TaskId::from_name("account"), Some(TaskId::Balances));
    assert_eq!(TaskId::from_name("market"), Some(TaskId::MarketFallback));
    assert_eq!(TaskId::from_name("kline"), Some(TaskId::KlineFlush));
}

#[test]
fn kline_flush_is_five_seconds_and_not_in_connection_bootstrap() {
    assert_eq!(TaskId::KlineFlush.interval(), Some(Duration::from_secs(5)));
    assert!(!bootstrap_tasks(true).contains(&TaskId::KlineFlush));
    assert!(!bootstrap_tasks(false).contains(&TaskId::KlineFlush));
}

#[test]
fn kline_flush_bypasses_account_lifecycle_coordination() {
    assert!(!TaskId::KlineFlush.requires_account_lifecycle());
    assert!(TaskId::MarketFallback.requires_account_lifecycle());
}

#[tokio::test]
async fn kline_flush_executes_while_account_mutation_guard_is_held() {
    let coordinator = AccountLifecycleCoordinator::new();
    let _mutation_guard = coordinator.mutation_guard().await;
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ran_by_task = Arc::clone(&ran);

    tokio::time::timeout(
        Duration::from_millis(100),
        execute_task_with_account_lifecycle(&coordinator, TaskId::KlineFlush, move || async move {
            ran_by_task.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }),
    )
    .await
    .expect("KlineFlush must not wait for account lifecycle coordination")
    .unwrap();

    assert!(ran.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_outer_future_keeps_blocking_kline_work_serialized_across_restart() {
    let gate = Arc::new(tokio::sync::Mutex::new(()));
    let entered_first = Arc::new(tokio::sync::Notify::new());
    let entered_second = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new((StdMutex::new(false), Condvar::new()));

    let first = tokio::spawn(run_serialized_blocking(Arc::clone(&gate), {
        let entered = Arc::clone(&entered_first);
        let release = Arc::clone(&release);
        move || {
            entered.notify_one();
            let (released, wake) = release.as_ref();
            let mut released = released.lock().unwrap();
            while !*released {
                released = wake.wait(released).unwrap();
            }
        }
    }));
    entered_first.notified().await;
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());

    let second = tokio::spawn(run_serialized_blocking(Arc::clone(&gate), {
        let entered = Arc::clone(&entered_second);
        move || entered.notify_one()
    }));
    tokio::task::yield_now().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(50), entered_second.notified())
            .await
            .is_err()
    );

    let (released, wake) = release.as_ref();
    *released.lock().unwrap() = true;
    wake.notify_all();
    tokio::time::timeout(Duration::from_secs(1), second)
        .await
        .expect("the restarted flush should run after old blocking work exits")
        .unwrap()
        .unwrap();
}

#[test]
fn kline_flush_delays_its_first_tick_while_existing_tasks_remain_immediate() {
    assert_eq!(first_tick_delay(TaskId::KlineFlush), Duration::from_secs(5));
    assert_eq!(first_tick_delay(TaskId::MarketFallback), Duration::ZERO);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kline_flush_attempts_every_dirty_key_before_returning_a_joined_storage_error() {
    use crate::models::chart_workspace::ChartWorkspaceKey;
    use crate::models::market::Kline;
    use crate::services::ChartWorkspaceService;
    use crate::storage::{ChartStateStore, KlineStore};

    let root = std::env::temp_dir().join(format!(
        "easiflux-scheduler-kline-flush-{}",
        uuid::Uuid::new_v4()
    ));
    let kline_dir = root.join("klines");
    let kline_store = Arc::new(KlineStore::with_dir(kline_dir.clone()));
    let bad = ChartWorkspaceKey::parse("AAA", "1").unwrap();
    let good = ChartWorkspaceKey::parse("BBB", "1").unwrap();
    let sample = |key: &ChartWorkspaceKey| Kline {
        symbol: key.symbol.clone(),
        interval: key.interval.clone(),
        open_time: 1,
        open: "1".into(),
        high: "2".into(),
        low: "0.5".into(),
        close: "1.5".into(),
        volume: "10".into(),
    };
    kline_store.upsert_bars(&bad, &[sample(&bad)]).unwrap();
    kline_store.upsert_bars(&good, &[sample(&good)]).unwrap();
    std::fs::create_dir_all(kline_dir.join("AAA_1.jsonl")).unwrap();
    let service = Arc::new(ChartWorkspaceService::new(
        kline_store,
        Arc::new(ChartStateStore::with_root(root.join("state"))),
        Arc::new(|_| {}),
    ));

    let result = execute_kline_flush(
        service,
        Arc::new(KlineFlushCoordinator::new()),
        KlineFlushOrigin::Background,
    )
    .await;

    assert!(matches!(result, Err(AppError::Storage(_))));
    assert!(kline_dir.join("BBB_1.jsonl").is_file());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn environment_is_always_part_of_bootstrap() {
    assert_eq!(
        bootstrap_tasks(false),
        vec![
            TaskId::TimeSync,
            TaskId::MarketFallback,
            TaskId::FundingRate,
            TaskId::Environment,
        ]
    );
    assert_eq!(
        bootstrap_tasks(true),
        vec![
            TaskId::TimeSync,
            TaskId::MarketFallback,
            TaskId::FundingRate,
            TaskId::Environment,
            TaskId::Balances,
            TaskId::PrivatePanels,
            TaskId::DailyPnl,
        ]
    );
}

#[tokio::test]
async fn bootstrap_runs_every_applicable_task_and_collects_failures() {
    let tasks = bootstrap_tasks(true);
    let visited = Arc::new(StdMutex::new(Vec::new()));
    let visited_by_run = visited.clone();

    let failed = run_bootstrap_tasks(&tasks, move |task| {
        let visited = visited_by_run.clone();
        async move {
            visited.lock().unwrap().push(task);
            if matches!(task, TaskId::TimeSync | TaskId::Environment) {
                Err(crate::error::AppError::Internal(
                    "apiKey=raw-key keyring=raw-secret".into(),
                ))
            } else {
                Ok(())
            }
        }
    })
    .await;

    assert_eq!(*visited.lock().unwrap(), tasks);
    assert_eq!(failed, vec![TaskId::TimeSync, TaskId::Environment]);

    let visible = aggregate_bootstrap_failure_error(&failed).to_string();
    assert!(visible.contains("时间同步"));
    assert!(visible.contains("环境检测"));
    assert!(!visible.contains("raw-key"));
    assert!(!visible.contains("raw-secret"));
    assert!(!visible.contains("apiKey"));
    assert!(!visible.contains("keyring"));
}

#[tokio::test]
async fn queued_bootstrap_snapshots_connection_status_only_after_switch_commits() {
    let coordinator = Arc::new(AccountLifecycleCoordinator::new());
    let connected = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mutation = coordinator.mutation_guard().await;
    let phase = run_bootstrap_account_phase(
        coordinator.as_ref(),
        {
            let connected = Arc::clone(&connected);
            move || async move { connected.load(std::sync::atomic::Ordering::SeqCst) }
        },
        |_| async { Ok(()) },
    );
    tokio::pin!(phase);
    assert!(matches!(
        futures_util::poll!(&mut phase),
        std::task::Poll::Pending
    ));

    connected.store(true, std::sync::atomic::Ordering::SeqCst);
    drop(mutation);
    let (tasks, failed) = phase.await;

    assert!(failed.is_empty());
    assert!(tasks.contains(&TaskId::Balances));
    assert!(tasks.contains(&TaskId::PrivatePanels));
    assert!(tasks.contains(&TaskId::DailyPnl));
}

#[test]
fn bootstrap_suppresses_only_owned_markers_and_delivers_bare_session_failure_once() {
    for error in [
        AppError::Notified {
            code: "AUTH_SESSION_EXPIRED",
            message: "账户会话已失效",
            notification_id: "committed-id".into(),
            cause: Some(crate::error::NotificationCause::AuthFailure(
                crate::api::response::AuthFailureKind::SessionExpired,
            )),
        },
        AppError::Observed("环境检测失败"),
    ] {
        assert!(!bootstrap_failure_needs_generic_error(&error));
    }
    let bare_session = AppError::AuthFailure(crate::api::response::AuthFailureKind::SessionExpired);
    assert!(bootstrap_failure_needs_generic_error(&bare_session));
    assert!(bootstrap_failure_needs_generic_error(
        &AppError::Connection("ordinary failure".into())
    ));

    let sink = Arc::new(StdMutex::new(Vec::new()));
    let emitter = EventEmitter::new_test(Arc::clone(&sink));
    emit_bootstrap_failure_diagnostics(
        &emitter,
        BootstrapDeliveryOwnership::Background,
        &[(TaskId::Balances, bare_session)],
    );
    let events = sink.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0, "log:entry");
    assert_eq!(events[1].0, "error:occurred");
    assert_eq!(events[0].1["eventId"], events[1].1["eventId"]);
}

#[test]
fn reconciliation_bootstrap_suppresses_child_delivery_while_background_keeps_it() {
    let sink = Arc::new(StdMutex::new(Vec::new()));
    let emitter = EventEmitter::new_test(Arc::clone(&sink));
    let failures = vec![(
        TaskId::TimeSync,
        AppError::Internal("apiKey=raw-secret".into()),
    )];

    emit_bootstrap_failure_diagnostics(
        &emitter,
        BootstrapDeliveryOwnership::Reconciliation,
        &failures,
    );
    assert!(sink.lock().unwrap().is_empty());

    emit_bootstrap_failure_diagnostics(&emitter, BootstrapDeliveryOwnership::Background, &failures);
    let events = sink.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0, "log:entry");
    assert_eq!(events[1].0, "error:occurred");
    assert_eq!(events[0].1["eventId"], events[1].1["eventId"]);
    assert!(!events[0].1.to_string().contains("raw-secret"));
}

#[test]
fn direct_bootstrap_preserves_session_ownership_and_excludes_it_from_mixed_failures() {
    let session = AppError::Notified {
        code: "AUTH_SESSION_EXPIRED",
        message: "账户会话已失效",
        notification_id: "committed-session-id".into(),
        cause: Some(crate::error::NotificationCause::AuthFailure(
            crate::api::response::AuthFailureKind::SessionExpired,
        )),
    };
    let session_only = vec![(TaskId::Balances, session.clone())];
    let sink = Arc::new(StdMutex::new(Vec::new()));
    let emitter = EventEmitter::new_test(Arc::clone(&sink));

    emit_bootstrap_failure_diagnostics(
        &emitter,
        BootstrapDeliveryOwnership::Background,
        &session_only,
    );
    assert!(sink.lock().unwrap().is_empty());
    assert!(matches!(
        bootstrap_failure_error(&session_only),
        AppError::Notified {
            code: "AUTH_SESSION_EXPIRED",
            notification_id,
            ..
        } if notification_id == "committed-session-id"
    ));

    let mixed = vec![
        (TaskId::Balances, session),
        (
            TaskId::Environment,
            AppError::Connection("ordinary environment failure".into()),
        ),
    ];
    let mixed_error = bootstrap_failure_error(&mixed);
    let visible = mixed_error.user_message();
    assert!(matches!(mixed_error, AppError::Connection(_)));
    assert!(visible.contains("环境检测"));
    assert!(!visible.contains("账户资产"));
    assert!(!visible.contains("AUTH_SESSION_EXPIRED"));
}

#[test]
fn detached_bootstrap_does_not_log_an_owned_session_failure_again() {
    let delivered = Arc::new(StdMutex::new(Vec::new()));
    let session = AppError::Notified {
        code: "AUTH_SESSION_EXPIRED",
        message: "账户会话已失效",
        notification_id: "committed-session-id".into(),
        cause: Some(crate::error::NotificationCause::AuthFailure(
            crate::api::response::AuthFailureKind::SessionExpired,
        )),
    };

    deliver_detached_bootstrap_failure(&session, {
        let delivered = Arc::clone(&delivered);
        move |message| delivered.lock().unwrap().push(message)
    });
    assert!(delivered.lock().unwrap().is_empty());

    deliver_detached_bootstrap_failure(&AppError::Connection("ordinary".into()), {
        let delivered = Arc::clone(&delivered);
        move |message| delivered.lock().unwrap().push(message)
    });
    assert_eq!(delivered.lock().unwrap().as_slice(), ["连接错误: ordinary"]);
}

#[tokio::test]
async fn environment_probe_client_uses_reported_base_url() {
    let client = environment_probe_client("https://sandbox.example.test/").await;

    assert_eq!(client.base_url().await, "https://sandbox.example.test");
}

fn time_snapshot(sync_status: TimeSyncStatus, last_error: Option<&str>) -> TimeSnapshot {
    TimeSnapshot {
        server_time_ms: 1_700_000_000_000,
        local_time_ms: 1_700_000_000_000,
        offset_ms: 0,
        sync_status,
        source: if sync_status == TimeSyncStatus::Synced {
            TimeSource::Server
        } else {
            TimeSource::Local
        },
        last_sync_at: None,
        last_attempt_at: Some(1_700_000_000_000),
        last_error: last_error.map(str::to_string),
    }
}

fn environment_status(reachable: bool, error: Option<&str>) -> EnvironmentStatus {
    EnvironmentStatus {
        base_url: "https://api.example.test".into(),
        label: "test".into(),
        reachable,
        checked_at: 1_700_000_000_000,
        error: error.map(str::to_string),
    }
}

#[test]
fn scheduler_time_task_rejects_failed_snapshot_without_leaking_raw_error() {
    let result = time_sync_task_result(&time_snapshot(
        TimeSyncStatus::Failed,
        Some("https://secret.example.test apiKey=raw-key"),
    ));

    let error = result.expect_err("failed time sync must fail the scheduler task");
    let visible = error.to_string();
    assert!(!visible.contains("secret.example.test"));
    assert!(!visible.contains("raw-key"));
    assert!(!visible.contains("apiKey"));
}

#[test]
fn scheduler_time_task_accepts_synced_and_intentional_local_fallback_snapshots() {
    assert!(time_sync_task_result(&time_snapshot(TimeSyncStatus::Synced, None)).is_ok());
    assert!(time_sync_task_result(&time_snapshot(TimeSyncStatus::LocalFallback, None)).is_ok());
}

#[test]
fn scheduler_environment_task_uses_reachability_not_result_shape() {
    assert!(environment_task_result(&environment_status(true, None)).is_ok());

    let result = environment_task_result(&environment_status(
        false,
        Some("https://secret.example.test apiSecret=raw-secret"),
    ));
    let error = result.expect_err("unreachable environment must fail the scheduler task");
    let visible = error.to_string();
    assert!(!visible.contains("secret.example.test"));
    assert!(!visible.contains("raw-secret"));
    assert!(!visible.contains("apiSecret"));
}

#[tokio::test]
async fn unreachable_environment_status_is_published_before_task_returns_error() {
    let initial = environment_status(true, None);
    let stored = Arc::new(tokio::sync::RwLock::new(initial));
    let emitted = Arc::new(StdMutex::new(Vec::new()));
    let emitted_by_publish = emitted.clone();
    let failed = environment_status(false, Some("connection refused"));

    let result = publish_environment_task_status(&stored, failed.clone(), move |status| {
        emitted_by_publish.lock().unwrap().push(status.clone());
    })
    .await;

    assert!(result.is_err());
    let persisted = stored.read().await.clone();
    assert!(!persisted.reachable);
    assert_eq!(persisted.error.as_deref(), Some("connection refused"));
    let emitted = emitted.lock().unwrap();
    assert_eq!(emitted.len(), 1);
    assert!(!emitted[0].reachable);
    assert_eq!(emitted[0].error.as_deref(), Some("connection refused"));
}

#[tokio::test]
async fn bootstrap_collects_failures_from_status_semantics() {
    let tasks = [TaskId::TimeSync, TaskId::Environment];

    let failed = run_bootstrap_tasks(&tasks, |task| async move {
        match task {
            TaskId::TimeSync => time_sync_task_result(&time_snapshot(
                TimeSyncStatus::Failed,
                Some("raw time failure"),
            )),
            TaskId::Environment => {
                environment_task_result(&environment_status(false, Some("raw environment failure")))
            }
            _ => Ok(()),
        }
    })
    .await;

    assert_eq!(failed, vec![TaskId::TimeSync, TaskId::Environment]);
    let visible = aggregate_bootstrap_failure_error(&failed).to_string();
    assert!(!visible.contains("raw time failure"));
    assert!(!visible.contains("raw environment failure"));
}

#[tokio::test]
async fn bootstrap_snapshot_executes_rest_even_when_matching_ws_domains_are_fresh() {
    let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let operation_runs = runs.clone();

    run_rest_snapshot_if_needed(ExecutionMode::Bootstrap, true, move || {
        let runs = operation_runs.clone();
        async move {
            runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    })
    .await
    .unwrap();

    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn periodic_snapshot_skips_rest_only_when_matching_ws_domains_are_fresh() {
    let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let operation_runs = runs.clone();

    run_rest_snapshot_if_needed(ExecutionMode::Periodic, true, move || {
        let runs = operation_runs.clone();
        async move {
            runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    })
    .await
    .unwrap();

    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn periodic_snapshot_executes_rest_when_matching_ws_domain_is_stale() {
    let runs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let operation_runs = runs.clone();

    run_rest_snapshot_if_needed(ExecutionMode::Periodic, false, move || {
        let runs = operation_runs.clone();
        async move {
            runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    })
    .await
    .unwrap();

    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
}
