use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use tokio::sync::{Mutex, RwLock};

use super::super::*;
use crate::error::{AppError, AppResult};
use crate::models::notification::{
    NotificationCategory, NotificationChange, NotificationContent, NotificationKind,
    NotificationRecord, NotificationScope, NotificationSeverity,
};
use crate::services::notification::{
    NotificationAvailability, NotificationEmitter, NotificationRuntime, NotificationService,
};
use crate::storage::notification_store::{
    NotificationFileV1, NotificationPartition, NotificationPersistence,
};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const NOW_MS: u64 = 1_700_000_000_000;

#[derive(Default)]
struct RecordingPersistence {
    fail: bool,
    saves: StdMutex<Vec<NotificationFileV1>>,
}

impl NotificationPersistence for RecordingPersistence {
    fn save(&self, file: &NotificationFileV1) -> AppResult<()> {
        if self.fail {
            Err(AppError::Storage("NOTIFICATION_STORAGE_UNAVAILABLE".into()))
        } else {
            self.saves.lock().unwrap().push(file.clone());
            Ok(())
        }
    }
}

fn orphan_record() -> NotificationRecord {
    NotificationRecord {
        id: "00000000-0000-4000-8000-000000000091".into(),
        scope: NotificationScope::Account {
            account_id: "orphan".into(),
        },
        category: NotificationCategory::ConnectionSystem,
        kind: NotificationKind::ConnectionUnavailable,
        severity: NotificationSeverity::Error,
        content: NotificationContent {
            message_key: "connection.unavailable".into(),
            params: BTreeMap::new(),
            fallback_title: "连接不可用".into(),
            fallback_body: "交易连接暂时不可用，请检查网络或稍后重试。".into(),
        },
        entity: None,
        action: None,
        source_event_id: None,
        dedupe_key: "orphan-record".into(),
        occurrence_count: 1,
        created_at_ms: NOW_MS - 1,
        updated_at_ms: NOW_MS - 1,
        read_at_ms: None,
    }
}

fn notification_runtime(
    persistence: Arc<RecordingPersistence>,
) -> (
    Arc<NotificationRuntime>,
    Arc<StdMutex<Vec<crate::models::notification::NotificationChangedEvent>>>,
) {
    let events = Arc::new(StdMutex::new(Vec::new()));
    let captured = Arc::clone(&events);
    let emitter: NotificationEmitter = Arc::new(move |event| {
        captured.lock().unwrap().push(event.clone());
        Ok(())
    });
    let service = NotificationService::from_snapshot(
        NotificationFileV1 {
            schema_version: 1,
            revision: 4,
            source_event_index: Vec::new(),
            partitions: vec![NotificationPartition {
                scope: NotificationScope::Account {
                    account_id: "orphan".into(),
                },
                items: vec![orphan_record()],
            }],
        },
        persistence,
        emitter,
        NOW_MS - DAY.as_millis() as u64,
    );
    (
        Arc::new(NotificationRuntime::Available(Arc::new(service))),
        events,
    )
}

#[test]
fn notification_maintenance_is_the_unique_24_hour_task() {
    let all = TaskId::all();
    assert_eq!(
        all.iter()
            .filter(|task| **task == TaskId::NotificationMaintenance)
            .count(),
        1
    );
    assert_eq!(
        configured_task_interval(TaskId::NotificationMaintenance, &AppConfig::default()),
        Some(DAY)
    );
    assert_eq!(first_tick_delay(TaskId::NotificationMaintenance), DAY);
    assert!(!bootstrap_tasks(true).contains(&TaskId::NotificationMaintenance));
    assert!(TaskId::NotificationMaintenance.requires_account_lifecycle());
}

#[tokio::test(start_paused = true)]
async fn notification_maintenance_runs_once_per_24_hour_period() {
    let running = Arc::new(AtomicBool::new(true));
    let run_state = Arc::new(Mutex::new(TaskRunState::default()));
    let runs = Arc::new(AtomicUsize::new(0));
    let runs_for_loop = Arc::clone(&runs);
    let handle = tokio::spawn(run_fixed_periodic(
        Arc::clone(&running),
        run_state,
        DAY,
        DAY,
        move || {
            let runs = Arc::clone(&runs_for_loop);
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        },
    ));

    tokio::task::yield_now().await;
    tokio::time::advance(DAY - Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(runs.load(Ordering::SeqCst), 0);

    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    tokio::time::advance(DAY).await;
    tokio::task::yield_now().await;
    assert_eq!(runs.load(Ordering::SeqCst), 2);

    running.store(false, Ordering::SeqCst);
    handle.abort();
}

fn generation_handles(
    generation: u64,
    running: Arc<AtomicBool>,
    maintenance_runs: Arc<[AtomicUsize; 3]>,
    maintenance_started: Arc<[tokio::sync::Notify; 3]>,
    maintenance_ticked: Arc<[tokio::sync::Notify; 3]>,
) -> Vec<(TaskId, tauri::async_runtime::JoinHandle<()>)> {
    periodic_task_ids()
        .iter()
        .copied()
        .map(|task| {
            let handle = if task == TaskId::NotificationMaintenance {
                let runs = Arc::clone(&maintenance_runs);
                let started = Arc::clone(&maintenance_started);
                let ticked = Arc::clone(&maintenance_ticked);
                let running = Arc::clone(&running);
                tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
                    started[generation as usize].notify_one();
                    run_fixed_periodic(
                        Arc::clone(&running),
                        Arc::new(Mutex::new(TaskRunState::default())),
                        DAY,
                        DAY,
                        move || {
                            let runs = Arc::clone(&runs);
                            let ticked = Arc::clone(&ticked);
                            async move {
                                runs[generation as usize].fetch_add(1, Ordering::SeqCst);
                                ticked[generation as usize].notify_one();
                                Ok(())
                            }
                        },
                    )
                    .await;
                }))
            } else {
                tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(std::future::pending()))
            };
            (task, handle)
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn concurrent_start_stop_restart_serializes_generations_and_owns_every_handle() {
    let lifecycle = Arc::new(Mutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let maintenance_runs = Arc::new([
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    ]);
    let maintenance_started = Arc::new([
        tokio::sync::Notify::new(),
        tokio::sync::Notify::new(),
        tokio::sync::Notify::new(),
    ]);
    let maintenance_ticked = Arc::new([
        tokio::sync::Notify::new(),
        tokio::sync::Notify::new(),
        tokio::sync::Notify::new(),
    ]);
    let start_entered = Arc::new(tokio::sync::Notify::new());
    let release_start = Arc::new(tokio::sync::Notify::new());

    let first_start = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let maintenance_runs = Arc::clone(&maintenance_runs);
        let maintenance_started = Arc::clone(&maintenance_started);
        let maintenance_ticked = Arc::clone(&maintenance_ticked);
        let start_entered = Arc::clone(&start_entered);
        let release_start = Arc::clone(&release_start);
        let running_for_install = Arc::clone(&running);
        async move {
            start_scheduler_lifecycle(&lifecycle, &running, move |generation| async move {
                let handles = generation_handles(
                    generation,
                    Arc::clone(&running_for_install),
                    Arc::clone(&maintenance_runs),
                    Arc::clone(&maintenance_started),
                    Arc::clone(&maintenance_ticked),
                );
                let finish_start = async move {
                    start_entered.notify_one();
                    release_start.notified().await;
                };
                (handles, finish_start)
            })
            .await
        }
    });
    start_entered.notified().await;

    let stop = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        async move { stop_scheduler_lifecycle(&lifecycle, &running).await }
    });
    tokio::task::yield_now().await;
    assert!(!stop.is_finished());

    let restart = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let maintenance_runs = Arc::clone(&maintenance_runs);
        let maintenance_started_for_install = Arc::clone(&maintenance_started);
        let maintenance_ticked_for_install = Arc::clone(&maintenance_ticked);
        let running_for_install = Arc::clone(&running);
        async move {
            start_scheduler_lifecycle(&lifecycle, &running, move |generation| async move {
                (
                    generation_handles(
                        generation,
                        Arc::clone(&running_for_install),
                        maintenance_runs,
                        maintenance_started_for_install,
                        maintenance_ticked_for_install,
                    ),
                    async {},
                )
            })
            .await
        }
    });
    tokio::task::yield_now().await;
    assert!(!restart.is_finished());

    release_start.notify_one();
    assert!(first_start.await.unwrap());
    assert!(stop.await.unwrap());
    assert!(restart.await.unwrap());

    {
        let state = lifecycle.lock().await;
        assert_eq!(state.running_generation(), Some(2));
        assert_eq!(state.handle_count(), periodic_task_ids().len());
        for task in periodic_task_ids() {
            assert!(state.has_handle(*task));
        }
    }

    let duplicate_installs = Arc::new(AtomicUsize::new(0));
    let duplicate_installs_in_start = Arc::clone(&duplicate_installs);
    assert!(
        !start_scheduler_lifecycle(&lifecycle, &running, move |_| async move {
            duplicate_installs_in_start.fetch_add(1, Ordering::SeqCst);
            (Vec::new(), async {})
        })
        .await
    );
    assert_eq!(duplicate_installs.load(Ordering::SeqCst), 0);

    maintenance_started[2].notified().await;
    tokio::time::advance(DAY).await;
    tokio::time::timeout(Duration::from_secs(1), maintenance_ticked[2].notified())
        .await
        .expect("the restarted maintenance generation should tick after 24 hours");
    assert_eq!(maintenance_runs[1].load(Ordering::SeqCst), 0);
    assert_eq!(maintenance_runs[2].load(Ordering::SeqCst), 1);

    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
    tokio::time::advance(DAY).await;
    tokio::task::yield_now().await;
    assert_eq!(maintenance_runs[2].load(Ordering::SeqCst), 1);
    let state = lifecycle.lock().await;
    assert_eq!(state.running_generation(), None);
    assert_eq!(state.handle_count(), 0);
}

#[tokio::test]
async fn available_maintenance_snapshots_configured_accounts_and_prunes_orphans() {
    let persistence = Arc::new(RecordingPersistence::default());
    let (runtime, events) = notification_runtime(Arc::clone(&persistence));
    let mut config = AppConfig::default();
    config.active_account_id = "primary".into();
    config.accounts = vec!["primary".into()];
    let config = Arc::new(RwLock::new(config));

    run_notification_maintenance_once(&runtime, &config, NOW_MS).await;

    let saves = persistence.saves.lock().unwrap();
    assert_eq!(saves.len(), 1);
    assert!(saves[0].partitions.is_empty());
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].change, NotificationChange::Reset);
    assert_eq!(
        events[0].affected_scopes,
        [NotificationScope::Account {
            account_id: "orphan".into()
        }]
    );
}

#[tokio::test]
async fn unavailable_maintenance_is_a_no_op() {
    let runtime = Arc::new(NotificationRuntime::Unavailable(
        NotificationAvailability::new("NOTIFICATION_STORAGE_UNAVAILABLE", "通知存储不可用"),
    ));
    let config = Arc::new(RwLock::new(AppConfig::default()));

    run_notification_maintenance_once(&runtime, &config, NOW_MS).await;
}

#[tokio::test]
async fn maintenance_save_failure_does_not_emit_a_notification_or_change_revision() {
    let persistence = Arc::new(RecordingPersistence {
        fail: true,
        saves: StdMutex::new(Vec::new()),
    });
    let (runtime, events) = notification_runtime(persistence);
    let config = Arc::new(RwLock::new(AppConfig::default()));
    let service = runtime.service().unwrap().clone();

    run_notification_maintenance_once(&runtime, &config, NOW_MS).await;

    assert_eq!(service.revision().await, "4");
    assert!(events.lock().unwrap().is_empty());
}
