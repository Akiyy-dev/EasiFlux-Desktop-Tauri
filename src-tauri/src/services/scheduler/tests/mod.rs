use super::*;
use std::sync::{Arc, Mutex as StdMutex};

use crate::models::time::{TimeSnapshot, TimeSource, TimeSyncStatus};

mod coordination;

#[test]
fn task_id_parses_frontend_names() {
    assert_eq!(TaskId::from_name("dailyPnl"), Some(TaskId::DailyPnl));
    assert_eq!(TaskId::from_name("account"), Some(TaskId::Balances));
    assert_eq!(TaskId::from_name("market"), Some(TaskId::MarketFallback));
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

    let visible = bootstrap_failure_error(&failed).to_string();
    assert!(visible.contains("时间同步"));
    assert!(visible.contains("环境检测"));
    assert!(!visible.contains("raw-key"));
    assert!(!visible.contains("raw-secret"));
    assert!(!visible.contains("apiKey"));
    assert!(!visible.contains("keyring"));
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
    let visible = bootstrap_failure_error(&failed).to_string();
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
