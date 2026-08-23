use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex as StdMutex};
use std::time::Duration;

use tokio::sync::RwLock;

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
    let run_state = Arc::new(StdMutex::new(TaskRunState::default()));
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
    run_states: Arc<HashMap<TaskId, TaskRunStateRef>>,
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
                let run_state = run_states
                    .get(&TaskId::NotificationMaintenance)
                    .expect("maintenance registered")
                    .clone();
                tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
                    started[generation as usize].notify_one();
                    run_fixed_periodic(Arc::clone(&running), run_state, DAY, DAY, move || {
                        let runs = Arc::clone(&runs);
                        let ticked = Arc::clone(&ticked);
                        async move {
                            runs[generation as usize].fetch_add(1, Ordering::SeqCst);
                            ticked[generation as usize].notify_one();
                            Ok(())
                        }
                    })
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
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
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
        async move {
            start_scheduler_lifecycle(
                &lifecycle,
                &running,
                move |_| async move {
                    start_entered.notify_one();
                    release_start.notified().await;
                },
                move |generation, run_states, generation_running, ()| {
                    (
                        generation_handles(
                            generation,
                            generation_running,
                            run_states,
                            Arc::clone(&maintenance_runs),
                            Arc::clone(&maintenance_started),
                            Arc::clone(&maintenance_ticked),
                        ),
                        async {},
                    )
                },
            )
            .await
        }
    });
    start_entered.notified().await;

    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);

    let restart = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let maintenance_runs = Arc::clone(&maintenance_runs);
        let maintenance_started_for_install = Arc::clone(&maintenance_started);
        let maintenance_ticked_for_install = Arc::clone(&maintenance_ticked);
        async move {
            start_scheduler_lifecycle(
                &lifecycle,
                &running,
                |_| async {},
                move |generation, run_states, generation_running, ()| {
                    (
                        generation_handles(
                            generation,
                            generation_running,
                            run_states,
                            maintenance_runs,
                            maintenance_started_for_install,
                            maintenance_ticked_for_install,
                        ),
                        async {},
                    )
                },
            )
            .await
        }
    });
    assert!(restart.await.unwrap());

    release_start.notify_one();
    assert!(!first_start.await.unwrap());

    {
        let state = lifecycle.lock().unwrap();
        assert_eq!(state.running_generation(), Some(2));
        assert_eq!(state.handle_count(), periodic_task_ids().len());
        for task in periodic_task_ids() {
            assert!(state.has_handle(*task));
        }
    }

    let duplicate_installs = Arc::new(AtomicUsize::new(0));
    let duplicate_installs_in_start = Arc::clone(&duplicate_installs);
    assert!(
        !start_scheduler_lifecycle(
            &lifecycle,
            &running,
            move |_| async move {
                duplicate_installs_in_start.fetch_add(1, Ordering::SeqCst);
            },
            |_, _, _, ()| (Vec::new(), async {}),
        )
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
    let state = lifecycle.lock().unwrap();
    assert_eq!(state.running_generation(), None);
    assert_eq!(state.handle_count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_during_synchronous_install_waits_before_restart_without_holding_lifecycle_lock() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let install_entered = Arc::new(tokio::sync::Notify::new());
    let release_install = Arc::new((StdMutex::new(false), Condvar::new()));
    let start = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let install_entered = Arc::clone(&install_entered);
        let release_install = Arc::clone(&release_install);
        async move {
            start_scheduler_lifecycle(
                &lifecycle,
                &running,
                |_| async {},
                move |_, _, _, ()| {
                    install_entered.notify_one();
                    let (released, wake) = release_install.as_ref();
                    let mut released = released.lock().unwrap();
                    while !*released {
                        released = wake.wait(released).unwrap();
                    }
                    (Vec::new(), async {})
                },
            )
            .await
        }
    });
    install_entered.notified().await;

    let stop = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        async move { stop_scheduler_lifecycle(&lifecycle, &running).await }
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if lifecycle.lock().unwrap().state
                == (SchedulerLifecycleState::Stopping { generation: 1 })
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("stop must publish Stopping while synchronous install is blocked");
    assert_eq!(
        lifecycle.lock().unwrap().state,
        SchedulerLifecycleState::Stopping { generation: 1 }
    );
    assert!(!stop.is_finished());

    let restart = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        async move {
            start_scheduler_lifecycle(
                &lifecycle,
                &running,
                |_| async {},
                |_, _, _, ()| (Vec::new(), async {}),
            )
            .await
        }
    });
    let (released, wake) = release_install.as_ref();
    *released.lock().unwrap() = true;
    wake.notify_all();

    assert!(!start.await.unwrap());
    assert!(stop.await.unwrap());
    assert!(restart.await.unwrap());
    assert_eq!(lifecycle.lock().unwrap().running_generation(), Some(2));
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[test]
fn starting_generation_is_not_exposed_to_external_run_now_claims() {
    let mut lifecycle = SchedulerLifecycle::new();
    assert!(lifecycle.begin_start().is_some());

    assert!(lifecycle.run_state(TaskId::TimeSync).is_none());
}

#[tokio::test]
async fn cancelling_start_before_handle_install_rolls_back_and_allows_restart() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());

    let start = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        async move {
            start_scheduler_lifecycle(
                &lifecycle,
                &running,
                move |_| async move {
                    entered.notify_one();
                    release.notified().await;
                },
                |_, _, _, ()| (Vec::new(), async {}),
            )
            .await
        }
    });
    entered.notified().await;
    start.abort();
    assert!(start.await.unwrap_err().is_cancelled());
    let _ = stop_scheduler_lifecycle(&lifecycle, &running).await;

    let coherent_after_cancel = {
        let state = lifecycle.lock().unwrap();
        state.state == SchedulerLifecycleState::Stopped
            && state.handle_count() == 0
            && !running.load(Ordering::SeqCst)
    };
    let restarted = start_scheduler_lifecycle(
        &lifecycle,
        &running,
        |_| async {},
        |_, _, _, ()| (Vec::new(), async {}),
    )
    .await;
    stop_scheduler_lifecycle(&lifecycle, &running).await;

    assert!(coherent_after_cancel);
    assert!(restarted);
}

#[tokio::test(start_paused = true)]
async fn cancelling_start_during_immediate_run_aborts_installed_generation() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let immediate_entered = Arc::new(tokio::sync::Notify::new());
    let release_immediate = Arc::new(tokio::sync::Notify::new());
    let orphan_ticks = Arc::new(AtomicUsize::new(0));

    let start = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let immediate_entered = Arc::clone(&immediate_entered);
        let release_immediate = Arc::clone(&release_immediate);
        let orphan_ticks = Arc::clone(&orphan_ticks);
        async move {
            start_scheduler_lifecycle(
                &lifecycle,
                &running,
                |_| async {},
                move |_, _, _, ()| {
                    let handle =
                        tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
                            let mut interval = tokio::time::interval(Duration::from_secs(1));
                            interval.tick().await;
                            loop {
                                interval.tick().await;
                                orphan_ticks.fetch_add(1, Ordering::SeqCst);
                            }
                        }));
                    let finish_start = async move {
                        immediate_entered.notify_one();
                        release_immediate.notified().await;
                    };
                    (vec![(TaskId::TimeSync, handle)], finish_start)
                },
            )
            .await
        }
    });
    immediate_entered.notified().await;
    start.abort();
    assert!(start.await.unwrap_err().is_cancelled());
    let _ = stop_scheduler_lifecycle(&lifecycle, &running).await;
    tokio::time::advance(Duration::from_secs(2)).await;
    tokio::task::yield_now().await;

    let coherent_after_cancel = {
        let state = lifecycle.lock().unwrap();
        state.state == SchedulerLifecycleState::Stopped
            && state.handle_count() == 0
            && !running.load(Ordering::SeqCst)
    };
    let leaked_ticks = orphan_ticks.load(Ordering::SeqCst);
    let restarted = start_scheduler_lifecycle(
        &lifecycle,
        &running,
        |_| async {},
        |_, _, _, ()| (Vec::new(), async {}),
    )
    .await;
    stop_scheduler_lifecycle(&lifecycle, &running).await;

    assert!(coherent_after_cancel);
    assert_eq!(leaked_ticks, 0);
    assert!(restarted);
}

#[tokio::test]
async fn stop_resolves_time_sync_force_waiter_and_restart_runs_a_fresh_generation() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let owner_entered = Arc::new(AtomicBool::new(false));
    let owner_release = Arc::new(tokio::sync::Notify::new());

    let first_start = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let owner_entered = Arc::clone(&owner_entered);
        let owner_release = Arc::clone(&owner_release);
        async move {
            start_scheduler_lifecycle(
                &lifecycle,
                &running,
                |_| async {},
                move |_, run_states, generation_running, ()| {
                    let time_state = run_states.get(&TaskId::TimeSync).unwrap().clone();
                    let handle_state = time_state.clone();
                    let execute_entered = Arc::clone(&owner_entered);
                    let finish_entered = Arc::clone(&owner_entered);
                    let handle =
                        tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(run_fixed_periodic(
                            generation_running,
                            handle_state,
                            Duration::ZERO,
                            DAY,
                            move || {
                                let entered = Arc::clone(&execute_entered);
                                let release = Arc::clone(&owner_release);
                                async move {
                                    entered.store(true, Ordering::SeqCst);
                                    release.notified().await;
                                    Ok(())
                                }
                            },
                        )));
                    let finish_start = async move {
                        while !finish_entered.load(Ordering::SeqCst) {
                            tokio::task::yield_now().await;
                        }
                        let _ = run_scheduled_task(time_state, true, || async { Ok(()) }).await;
                    };
                    (vec![(TaskId::TimeSync, handle)], finish_start)
                },
            )
            .await
        }
    });
    loop {
        let time_state = lifecycle
            .lock()
            .unwrap()
            .run_states
            .as_ref()
            .and_then(|states| states.get(&TaskId::TimeSync))
            .cloned();
        if let Some(time_state) = time_state {
            if time_state.lock().unwrap().pending_force {
                break;
            }
        }
        tokio::task::yield_now().await;
    }

    assert!(tokio::time::timeout(
        Duration::from_secs(1),
        stop_scheduler_lifecycle(&lifecycle, &running),
    )
    .await
    .expect("stop must not wait on the forced bootstrap waiter"));
    assert!(!tokio::time::timeout(Duration::from_secs(1), first_start)
        .await
        .expect("the cancelled start waiter must resolve")
        .unwrap());

    let fresh_runs = Arc::new(AtomicUsize::new(0));
    let restarted =
        start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
            let fresh_runs = Arc::clone(&fresh_runs);
            move |_, run_states, generation_running, ()| {
                let state = run_states.get(&TaskId::TimeSync).unwrap().clone();
                let handle = tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                    run_fixed_periodic(generation_running, state, Duration::ZERO, DAY, move || {
                        let fresh_runs = Arc::clone(&fresh_runs);
                        async move {
                            fresh_runs.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        }
                    }),
                ));
                (vec![(TaskId::TimeSync, handle)], async {})
            }
        })
        .await;
    tokio::time::timeout(Duration::from_secs(1), async {
        while fresh_runs.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the restarted generation should execute TimeSync");

    assert!(restarted);
    assert_eq!(fresh_runs.load(Ordering::SeqCst), 1);
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test]
async fn stop_during_in_flight_runs_allows_every_periodic_task_to_run_after_restart() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(tokio::sync::Notify::new());

    assert!(
        start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            move |_, run_states, generation_running, ()| {
                let handles: Vec<_> = periodic_task_ids()
                    .iter()
                    .copied()
                    .map(|task| {
                        let state = run_states.get(&task).unwrap().clone();
                        let running = Arc::clone(&generation_running);
                        let entered = Arc::clone(&entered);
                        let release = Arc::clone(&release);
                        let handle = tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                            run_fixed_periodic(running, state, Duration::ZERO, DAY, move || {
                                let entered = Arc::clone(&entered);
                                let release = Arc::clone(&release);
                                async move {
                                    entered.fetch_add(1, Ordering::SeqCst);
                                    release.notified().await;
                                    Ok(())
                                }
                            }),
                        ));
                        (task, handle)
                    })
                    .collect();
                (handles, async {})
            }
        },)
        .await
    );
    tokio::time::timeout(Duration::from_secs(1), async {
        while entered.load(Ordering::SeqCst) != periodic_task_ids().len() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("every first-generation task should become in flight");

    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);

    let fresh_runs = Arc::new(
        (0..periodic_task_ids().len())
            .map(|_| AtomicUsize::new(0))
            .collect::<Vec<_>>(),
    );
    assert!(
        start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
            let fresh_runs = Arc::clone(&fresh_runs);
            move |_, run_states, generation_running, ()| {
                let handles: Vec<_> = periodic_task_ids()
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(index, task)| {
                        let state = run_states.get(&task).unwrap().clone();
                        let running = Arc::clone(&generation_running);
                        let fresh_runs = Arc::clone(&fresh_runs);
                        let handle = tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                            run_fixed_periodic(running, state, Duration::ZERO, DAY, move || {
                                let fresh_runs = Arc::clone(&fresh_runs);
                                async move {
                                    fresh_runs[index].fetch_add(1, Ordering::SeqCst);
                                    Ok(())
                                }
                            }),
                        ));
                        (task, handle)
                    })
                    .collect();
                (handles, async {})
            }
        },)
        .await
    );
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if fresh_runs
                .iter()
                .all(|runs| runs.load(Ordering::SeqCst) == 1)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("every restarted periodic task, including maintenance, should run once");

    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test]
async fn stale_run_state_captured_before_stop_cannot_claim_after_retirement() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    assert!(
        start_scheduler_lifecycle(
            &lifecycle,
            &running,
            |_| async {},
            |_, _, _, ()| (Vec::new(), async {}),
        )
        .await
    );
    let stale = lifecycle
        .lock()
        .unwrap()
        .run_state(TaskId::NotificationMaintenance)
        .unwrap();

    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
    let runs = Arc::new(AtomicUsize::new(0));
    let runs_by_task = Arc::clone(&runs);
    let result = run_scheduled_task(stale, false, move || {
        let runs = Arc::clone(&runs_by_task);
        async move {
            runs.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    })
    .await;

    assert!(result.is_err());
    assert_eq!(runs.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn stop_waits_for_manual_owner_to_cancel_and_drop_before_returning() {
    struct DropProbe(Arc<AtomicUsize>);
    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    assert!(
        start_scheduler_lifecycle(
            &lifecycle,
            &running,
            |_| async {},
            |_, _, _, ()| (Vec::new(), async {}),
        )
        .await
    );
    let run_state = lifecycle
        .lock()
        .unwrap()
        .run_state(TaskId::Balances)
        .unwrap();
    let entered = Arc::new(tokio::sync::Notify::new());
    let dropped = Arc::new(AtomicUsize::new(0));
    let manual = tokio::spawn({
        let entered = Arc::clone(&entered);
        let dropped = Arc::clone(&dropped);
        async move {
            run_scheduled_task(run_state, false, move || {
                let entered = Arc::clone(&entered);
                let probe = DropProbe(Arc::clone(&dropped));
                async move {
                    entered.notify_one();
                    let _probe = probe;
                    std::future::pending::<AppResult<()>>().await
                }
            })
            .await
        }
    });
    entered.notified().await;

    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
    let result = tokio::time::timeout(Duration::from_millis(100), manual)
        .await
        .expect("stop must wake and drain a manual owner")
        .unwrap();

    assert!(result.is_err());
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(
        lifecycle.lock().unwrap().state,
        SchedulerLifecycleState::Stopped
    );
}

#[tokio::test]
async fn periodic_panic_during_start_prevents_commit_and_allows_healthy_restart() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let exited = Arc::new(tokio::sync::Notify::new());
    let started = start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
        let exited = Arc::clone(&exited);
        move |_, _, _, ()| {
            let handle = tauri::async_runtime::JoinHandle::Tokio(tokio::spawn({
                let exited = Arc::clone(&exited);
                async move {
                    exited.notify_one();
                    panic!("maintenance loop panic during start");
                }
            }));
            let finish = async move { exited.notified().await };
            (vec![(TaskId::NotificationMaintenance, handle)], finish)
        }
    })
    .await;

    assert!(!started);
    let _ = stop_scheduler_lifecycle(&lifecycle, &running).await;
    assert_eq!(
        lifecycle.lock().unwrap().state,
        SchedulerLifecycleState::Stopped
    );
    assert!(
        start_scheduler_lifecycle(
            &lifecycle,
            &running,
            |_| async {},
            |_, _, _, ()| (Vec::new(), async {}),
        )
        .await
    );
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test]
async fn stop_returns_only_after_periodic_future_is_dropped() {
    struct DropProbe(Arc<AtomicUsize>);
    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(tokio::sync::Notify::new());
    let dropped = Arc::new(AtomicUsize::new(0));
    assert!(
        start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
            let entered = Arc::clone(&entered);
            let dropped = Arc::clone(&dropped);
            move |_, _, _, ()| {
                let handle = tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
                    let _probe = DropProbe(dropped);
                    entered.notify_one();
                    std::future::pending::<()>().await;
                }));
                (vec![(TaskId::NotificationMaintenance, handle)], async {})
            }
        },)
        .await
    );
    entered.notified().await;

    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_stop_cleanup_finishes_and_a_waiting_restart_succeeds() {
    struct BlockingDrop {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<(StdMutex<bool>, Condvar)>,
    }
    impl Drop for BlockingDrop {
        fn drop(&mut self) {
            self.entered.notify_one();
            let (released, wake) = self.release.as_ref();
            let mut released = released.lock().unwrap();
            while !*released {
                released = wake.wait(released).unwrap();
            }
        }
    }

    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    assert!(
        start_scheduler_lifecycle(
            &lifecycle,
            &running,
            |_| async {},
            |_, _, _, ()| (Vec::new(), async {}),
        )
        .await
    );
    let run_state = lifecycle
        .lock()
        .unwrap()
        .run_state(TaskId::Balances)
        .unwrap();
    let owner_entered = Arc::new(tokio::sync::Notify::new());
    let drop_entered = Arc::new(tokio::sync::Notify::new());
    let release_drop = Arc::new((StdMutex::new(false), Condvar::new()));
    let manual = tokio::spawn({
        let owner_entered = Arc::clone(&owner_entered);
        let drop_entered = Arc::clone(&drop_entered);
        let release_drop = Arc::clone(&release_drop);
        async move {
            run_scheduled_task(run_state, false, move || {
                let probe = BlockingDrop {
                    entered: Arc::clone(&drop_entered),
                    release: Arc::clone(&release_drop),
                };
                let owner_entered = Arc::clone(&owner_entered);
                async move {
                    let _probe = probe;
                    owner_entered.notify_one();
                    std::future::pending::<AppResult<()>>().await
                }
            })
            .await
        }
    });
    owner_entered.notified().await;

    let stop = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        async move { stop_scheduler_lifecycle(&lifecycle, &running).await }
    });
    drop_entered.notified().await;
    stop.abort();
    assert!(stop.await.unwrap_err().is_cancelled());

    let restart = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        async move {
            start_scheduler_lifecycle(
                &lifecycle,
                &running,
                |_| async {},
                |_, _, _, ()| (Vec::new(), async {}),
            )
            .await
        }
    });
    tokio::task::yield_now().await;
    assert!(!restart.is_finished());

    let (released, wake) = release_drop.as_ref();
    *released.lock().unwrap() = true;
    wake.notify_all();
    assert!(tokio::time::timeout(Duration::from_secs(1), restart)
        .await
        .expect("cleanup must outlive the cancelled stop caller")
        .unwrap());
    assert!(manual.await.unwrap().is_err());
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test]
async fn periodic_exit_while_running_tears_down_generation_before_restart() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let release = Arc::new(tokio::sync::Notify::new());
    let exited = Arc::new(tokio::sync::Notify::new());
    assert!(
        start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
            let release = Arc::clone(&release);
            let exited = Arc::clone(&exited);
            move |_, _, _, ()| {
                let handle = tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
                    release.notified().await;
                    exited.notify_one();
                }));
                (vec![(TaskId::NotificationMaintenance, handle)], async {})
            }
        },)
        .await
    );

    release.notify_one();
    exited.notified().await;
    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            if lifecycle.lock().unwrap().state == SchedulerLifecycleState::Stopped {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("an unexpected periodic exit must retire its generation");

    assert!(
        start_scheduler_lifecycle(
            &lifecycle,
            &running,
            |_| async {},
            |_, _, _, ()| (Vec::new(), async {}),
        )
        .await
    );
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[test]
fn generation_exhaustion_fails_closed_without_reusing_the_last_generation() {
    let mut lifecycle = SchedulerLifecycle::new();
    lifecycle.next_generation = u64::MAX;

    assert!(lifecycle.begin_start().is_none());
    assert_eq!(lifecycle.state, SchedulerLifecycleState::Stopped);
    assert!(lifecycle.run_states.is_none());
}

#[test]
fn cancelled_starting_generation_cannot_commit_running() {
    let mut lifecycle = SchedulerLifecycle::new();
    let (_, _, _, control) = lifecycle.begin_start().unwrap();
    control.cancel();

    assert!(!lifecycle.finish_start(1));
    assert_eq!(
        lifecycle.state,
        SchedulerLifecycleState::Starting { generation: 1 }
    );
}

#[tokio::test]
async fn partially_collected_handle_batch_aborts_owned_loops_on_panic() {
    struct DropProbe(Arc<AtomicUsize>);
    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    let entered = Arc::new(tokio::sync::Notify::new());
    let dropped = Arc::new(AtomicUsize::new(0));
    let handle = tauri::async_runtime::JoinHandle::Tokio(tokio::spawn({
        let entered = Arc::clone(&entered);
        let dropped = Arc::clone(&dropped);
        async move {
            let _probe = DropProbe(dropped);
            entered.notify_one();
            std::future::pending::<()>().await;
        }
    }));
    entered.notified().await;

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let items = [Some((TaskId::TimeSync, handle)), None];
        let _batch: SchedulerHandleBatch = items
            .into_iter()
            .map(|item| item.expect("synthetic install panic"))
            .collect();
    }));
    assert!(result.is_err());
    tokio::time::timeout(Duration::from_secs(1), async {
        while dropped.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("partial handle batch must abort the loop it already owns");
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
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
