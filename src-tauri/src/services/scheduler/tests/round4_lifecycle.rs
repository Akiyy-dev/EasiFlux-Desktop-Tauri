use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex as StdMutex};
use std::time::Duration;

use super::super::*;

struct DropProbe(Arc<AtomicUsize>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn start_decision_atomically_waits_for_stopping_then_begins_once() {
    let mut lifecycle = SchedulerLifecycle::new();
    let first = lifecycle.start_decision();
    let first_generation = match first {
        SchedulerStartDecision::Begin {
            generation,
            control,
            ..
        } => {
            control.owner_finished();
            generation
        }
        _ => panic!("the first start must allocate a generation"),
    };
    assert_eq!(first_generation, 1);
    let cleanup = lifecycle.begin_cleanup(first_generation);
    assert!(matches!(cleanup, CleanupRequest::Start(_)));

    let waiting = lifecycle.start_decision();
    assert!(matches!(waiting, SchedulerStartDecision::Wait(_)));
    assert!(lifecycle.finish_cleanup(first_generation));

    let second = lifecycle.start_decision();
    let second_generation = match second {
        SchedulerStartDecision::Begin {
            generation,
            control,
            ..
        } => {
            control.owner_finished();
            generation
        }
        _ => panic!("the waiting start must redecide and allocate once"),
    };
    assert_eq!(second_generation, 2);
    assert!(matches!(
        lifecycle.start_decision(),
        SchedulerStartDecision::AlreadyRunning
    ));
}

#[test]
fn generation_exhaustion_is_permanent_and_explicit() {
    let mut lifecycle = SchedulerLifecycle::new();
    lifecycle.next_generation = u64::MAX;

    assert!(matches!(
        lifecycle.start_decision(),
        SchedulerStartDecision::Exhausted
    ));
    assert!(matches!(
        lifecycle.start_decision(),
        SchedulerStartDecision::Exhausted
    ));
    assert_eq!(lifecycle.state, SchedulerLifecycleState::Stopped);
}

#[test]
fn final_generation_is_used_once_then_cursor_stays_exhausted() {
    let mut lifecycle = SchedulerLifecycle::new();
    lifecycle.next_generation = u64::MAX - 1;
    let (generation, _, _, control) = lifecycle.begin_start().unwrap();
    assert_eq!(generation, u64::MAX);
    control.owner_started();
    control.owner_finished();
    assert!(matches!(
        lifecycle.begin_cleanup(generation),
        CleanupRequest::Start(_)
    ));
    assert!(lifecycle.finish_cleanup(generation));

    assert!(matches!(
        lifecycle.start_decision(),
        SchedulerStartDecision::Exhausted
    ));
    assert!(matches!(
        lifecycle.start_decision(),
        SchedulerStartDecision::Exhausted
    ));
}

#[tokio::test]
async fn bootstrap_activity_is_cancelled_and_drained_before_stop_returns() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    assert!(
        start_scheduler_lifecycle(
            &lifecycle,
            &running,
            |_| async {},
            |_, _, _, _, ()| (Vec::new(), async {}),
        )
        .await
    );

    let entered = Arc::new(tokio::sync::Notify::new());
    let dropped = Arc::new(AtomicUsize::new(0));
    let bootstrap = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let entered = Arc::clone(&entered);
        let dropped = Arc::clone(&dropped);
        async move {
            run_generation_activity(&lifecycle, move || {
                let entered = Arc::clone(&entered);
                let probe = DropProbe(Arc::clone(&dropped));
                async move {
                    let _probe = probe;
                    entered.notify_one();
                    std::future::pending::<AppResult<()>>().await
                }
            })
            .await
        }
    });
    entered.notified().await;

    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
    let result = tokio::time::timeout(Duration::from_secs(1), bootstrap)
        .await
        .expect("a stopped generation must cancel its bootstrap activity")
        .unwrap();

    assert!(result.is_err());
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(
        lifecycle.lock().unwrap().state,
        SchedulerLifecycleState::Stopped
    );
}

#[tokio::test]
async fn stopped_and_stopping_generations_reject_new_bootstrap_activity() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let calls = Arc::new(AtomicUsize::new(0));

    let stopped_calls = Arc::clone(&calls);
    let stopped = run_generation_activity(&lifecycle, move || {
        stopped_calls.fetch_add(1, Ordering::SeqCst);
        async { Ok(()) }
    })
    .await;
    assert_eq!(stopped.unwrap_err().to_string(), "内部错误: 调度器未运行");

    let running = Arc::new(AtomicBool::new(false));
    let (_, _, _, control) = lifecycle.lock().unwrap().begin_start().unwrap();
    control.owner_started();
    let cleanup = request_scheduler_cleanup(&lifecycle, &running, 1).unwrap();
    let stopping_calls = Arc::clone(&calls);
    let stopping = run_generation_activity(&lifecycle, move || {
        stopping_calls.fetch_add(1, Ordering::SeqCst);
        async { Ok(()) }
    })
    .await;
    assert_eq!(stopping.unwrap_err().to_string(), "内部错误: 调度器未运行");
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    control.owner_finished();
    wait_for_cleanup(cleanup).await;
}

#[tokio::test]
async fn starting_generation_bootstrap_is_cancelled_before_replacement_can_start() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let (_, _, _, _) = lifecycle.lock().unwrap().begin_start().unwrap();

    let entered = Arc::new(tokio::sync::Notify::new());
    let dropped = Arc::new(AtomicUsize::new(0));
    let bootstrap = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let entered = Arc::clone(&entered);
        let dropped = Arc::clone(&dropped);
        async move {
            run_generation_activity(&lifecycle, move || {
                let entered = Arc::clone(&entered);
                let probe = DropProbe(Arc::clone(&dropped));
                async move {
                    let _probe = probe;
                    entered.notify_one();
                    std::future::pending::<AppResult<()>>().await
                }
            })
            .await
        }
    });
    entered.notified().await;

    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
    assert!(bootstrap.await.unwrap().is_err());
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(
        start_scheduler_lifecycle(
            &lifecycle,
            &running,
            |_| async {},
            |_, _, _, _, ()| (Vec::new(), async {}),
        )
        .await
    );
    assert_eq!(lifecycle.lock().unwrap().running_generation(), Some(2));
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test]
async fn stop_cancels_bootstrap_waiting_for_account_read_guard_without_deadlock() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    assert!(
        start_scheduler_lifecycle(
            &lifecycle,
            &running,
            |_| async {},
            |_, _, _, _, ()| (Vec::new(), async {}),
        )
        .await
    );
    let coordinator = Arc::new(AccountLifecycleCoordinator::new());
    let mutation = coordinator.mutation_guard().await;
    let claimed = Arc::new(tokio::sync::Notify::new());
    let ran = Arc::new(AtomicUsize::new(0));

    let bootstrap = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let coordinator = Arc::clone(&coordinator);
        let claimed = Arc::clone(&claimed);
        let ran = Arc::clone(&ran);
        async move {
            run_generation_activity(&lifecycle, move || async move {
                claimed.notify_one();
                let _read = coordinator.read_guard().await;
                ran.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .await
        }
    });
    claimed.notified().await;

    assert!(tokio::time::timeout(
        Duration::from_secs(1),
        stop_scheduler_lifecycle(&lifecycle, &running),
    )
    .await
    .expect("stop must cancel a bootstrap blocked on the account read guard"));
    assert!(bootstrap.await.unwrap().is_err());
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    drop(mutation);
}

#[test]
fn recovery_backoff_is_exponential_and_bounded() {
    assert_eq!(recovery_backoff(1), Duration::from_millis(100));
    assert_eq!(recovery_backoff(2), Duration::from_millis(200));
    assert_eq!(recovery_backoff(3), Duration::from_millis(400));
    assert_eq!(recovery_backoff(7), Duration::from_secs(5));
    assert_eq!(recovery_backoff(u32::MAX), Duration::from_secs(5));
}

fn install_test_recovery_handler(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    running: &Arc<AtomicBool>,
    recovery_starts: Arc<AtomicUsize>,
) {
    let recovery_lifecycle = Arc::clone(lifecycle);
    let recovery_running = Arc::clone(running);
    lock_unpoisoned(lifecycle).recovery_handler = Some(Arc::new(move |ticket, completion| {
        let lifecycle = Arc::clone(&recovery_lifecycle);
        let running = Arc::clone(&recovery_running);
        let recovery_starts = Arc::clone(&recovery_starts);
        tokio::spawn(async move {
            wait_for_cleanup(completion).await;
            let cancellation = wait_for_generation_cancel(ticket.desired.subscribe());
            tokio::pin!(cancellation);
            tokio::select! {
                biased;
                _ = &mut cancellation => return,
                _ = tokio::time::sleep(recovery_backoff(ticket.attempt)) => {}
            }
            if !ticket.desired.is_active() {
                return;
            }
            recovery_starts.fetch_add(1, Ordering::SeqCst);
            let _ = start_scheduler_lifecycle_for_recovery(
                &lifecycle,
                &running,
                ticket,
                |_| async {},
                |_, _, _, _, ()| {
                    let handle = tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(
                        std::future::pending(),
                    ));
                    (vec![(TaskId::NotificationMaintenance, handle)], async {})
                },
            )
            .await;
        });
    }));
}

#[tokio::test(start_paused = true)]
async fn unexpected_required_loop_exit_recovers_once_after_cleanup_and_backoff() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let recovery_starts = Arc::new(AtomicUsize::new(0));
    install_test_recovery_handler(&lifecycle, &running, Arc::clone(&recovery_starts));
    let exit = Arc::new(tokio::sync::Notify::new());

    assert!(
        start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
            let exit = Arc::clone(&exit);
            move |_, _, _, _, ()| {
                let handle = tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
                    exit.notified().await;
                }));
                (vec![(TaskId::NotificationMaintenance, handle)], async {})
            }
        })
        .await
    );

    exit.notify_one();
    loop {
        if lifecycle.lock().unwrap().state == SchedulerLifecycleState::Stopped {
            break;
        }
        tokio::task::yield_now().await;
    }
    tokio::task::yield_now().await;
    assert_eq!(recovery_starts.load(Ordering::SeqCst), 0);
    tokio::time::advance(Duration::from_millis(99)).await;
    tokio::task::yield_now().await;
    assert_eq!(recovery_starts.load(Ordering::SeqCst), 0);
    tokio::time::advance(Duration::from_millis(1)).await;
    loop {
        if lifecycle.lock().unwrap().running_generation() == Some(2) {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert_eq!(recovery_starts.load(Ordering::SeqCst), 1);
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test(start_paused = true)]
async fn required_loop_exit_while_starting_drains_siblings_then_recovers() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let recovery_starts = Arc::new(AtomicUsize::new(0));
    install_test_recovery_handler(&lifecycle, &running, Arc::clone(&recovery_starts));
    let exit = Arc::new(tokio::sync::Notify::new());
    let sibling_dropped = Arc::new(AtomicUsize::new(0));
    let start = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let exit = Arc::clone(&exit);
        let sibling_dropped = Arc::clone(&sibling_dropped);
        async move {
            start_scheduler_lifecycle(
                &lifecycle,
                &running,
                |_| async {},
                move |_, _, _, _, ()| {
                    let failing =
                        tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
                            exit.notified().await
                        }));
                    let sibling =
                        tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
                            let _probe = DropProbe(sibling_dropped);
                            std::future::pending::<()>().await;
                        }));
                    (
                        vec![
                            (TaskId::TimeSync, failing),
                            (TaskId::NotificationMaintenance, sibling),
                        ],
                        std::future::pending::<()>(),
                    )
                },
            )
            .await
        }
    });
    loop {
        let lifecycle = lifecycle.lock().unwrap();
        let both_ready = lifecycle.required_loops.len() == 2
            && lifecycle
                .required_loops
                .values()
                .all(|status| *status == RequiredLoopStatus::Ready);
        drop(lifecycle);
        if both_ready {
            break;
        }
        tokio::task::yield_now().await;
    }

    exit.notify_one();
    assert!(!start.await.unwrap());
    loop {
        if lifecycle.lock().unwrap().state == SchedulerLifecycleState::Stopped {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(sibling_dropped.load(Ordering::SeqCst), 1);
    tokio::time::advance(RECOVERY_BACKOFF_BASE).await;
    loop {
        if lifecycle.lock().unwrap().running_generation() == Some(2) {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert_eq!(recovery_starts.load(Ordering::SeqCst), 1);
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test]
async fn simultaneous_required_loop_exits_reserve_only_one_recovery_ticket() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let recovery_requests = Arc::new(AtomicUsize::new(0));
    let requests = Arc::clone(&recovery_requests);
    lock_unpoisoned(&lifecycle).recovery_handler = Some(Arc::new(move |_, _| {
        requests.fetch_add(1, Ordering::SeqCst);
    }));
    let (exit, exit_receiver) = watch::channel(false);

    assert!(
        start_scheduler_lifecycle(
            &lifecycle,
            &running,
            |_| async {},
            move |_, _, _, _, ()| {
                let handles = [TaskId::TimeSync, TaskId::NotificationMaintenance]
                    .into_iter()
                    .map(|task| {
                        let mut exit = exit_receiver.clone();
                        let handle =
                            tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
                                let _ = exit.wait_for(|released| *released).await;
                            }));
                        (task, handle)
                    })
                    .collect::<Vec<_>>();
                (handles, async {})
            }
        )
        .await
    );
    exit.send_replace(true);
    loop {
        if lifecycle.lock().unwrap().state == SchedulerLifecycleState::Stopped {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert_eq!(recovery_requests.load(Ordering::SeqCst), 1);
    assert!(!stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test]
async fn production_required_set_rejects_a_missing_maintenance_loop() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let started = start_scheduler_lifecycle_with_origin(
        &lifecycle,
        SchedulerStartOrigin::Explicit,
        Some(&[TaskId::TimeSync, TaskId::NotificationMaintenance]),
        |_| async {},
        |_, _, _, _, ()| {
            let handle =
                tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(std::future::pending()));
            (vec![(TaskId::TimeSync, handle)], async {})
        },
    )
    .await;

    assert!(!started);
    let _ = stop_scheduler_lifecycle_inner(&lifecycle).await;
    assert_eq!(
        lifecycle.lock().unwrap().state,
        SchedulerLifecycleState::Stopped
    );
}

#[tokio::test(start_paused = true)]
async fn explicit_stop_cancels_pending_recovery_and_stale_generation_cannot_restart() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let recovery_starts = Arc::new(AtomicUsize::new(0));
    install_test_recovery_handler(&lifecycle, &running, Arc::clone(&recovery_starts));
    let exit = Arc::new(tokio::sync::Notify::new());
    assert!(
        start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
            let exit = Arc::clone(&exit);
            move |_, _, _, _, ()| {
                let handle = tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
                    exit.notified().await;
                }));
                (vec![(TaskId::NotificationMaintenance, handle)], async {})
            }
        })
        .await
    );
    exit.notify_one();
    loop {
        if lifecycle.lock().unwrap().state == SchedulerLifecycleState::Stopped {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert!(!stop_scheduler_lifecycle(&lifecycle, &running).await);
    assert!(
        start_scheduler_lifecycle(
            &lifecycle,
            &running,
            |_| async {},
            |_, _, _, _, ()| {
                let handle =
                    tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(std::future::pending()));
                (vec![(TaskId::NotificationMaintenance, handle)], async {})
            },
        )
        .await
    );
    tokio::time::advance(RECOVERY_BACKOFF_CAP).await;
    tokio::task::yield_now().await;

    assert_eq!(lifecycle.lock().unwrap().running_generation(), Some(2));
    assert_eq!(recovery_starts.load(Ordering::SeqCst), 0);
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test]
async fn exit_after_ready_before_commit_cannot_publish_durable_running() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let exit = Arc::new(tokio::sync::Notify::new());
    let release_finish = Arc::new(tokio::sync::Notify::new());
    let start = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let exit = Arc::clone(&exit);
        let release_finish = Arc::clone(&release_finish);
        async move {
            start_scheduler_lifecycle(
                &lifecycle,
                &running,
                |_| async {},
                move |_, _, _, _, ()| {
                    let handle =
                        tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
                            exit.notified().await;
                        }));
                    (
                        vec![(TaskId::NotificationMaintenance, handle)],
                        async move { release_finish.notified().await },
                    )
                },
            )
            .await
        }
    });

    loop {
        let ready = lifecycle
            .lock()
            .unwrap()
            .required_loops
            .get(&TaskId::NotificationMaintenance)
            == Some(&RequiredLoopStatus::Ready);
        if ready {
            break;
        }
        tokio::task::yield_now().await;
    }
    exit.notify_one();
    loop {
        if matches!(
            lifecycle.lock().unwrap().state,
            SchedulerLifecycleState::Stopping { .. } | SchedulerLifecycleState::Stopped
        ) {
            break;
        }
        tokio::task::yield_now().await;
    }
    release_finish.notify_one();

    assert!(!start.await.unwrap());
    let _ = stop_scheduler_lifecycle(&lifecycle, &running).await;
    assert_eq!(
        lifecycle.lock().unwrap().state,
        SchedulerLifecycleState::Stopped
    );
}

#[tokio::test]
async fn approved_loop_exit_prevents_a_later_loop_from_publishing_running() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let (generation, control) = match lifecycle.lock().unwrap().start_decision() {
        SchedulerStartDecision::Begin {
            generation,
            control,
            ..
        } => (generation, control),
        _ => panic!("test generation must begin"),
    };
    let (exit_approved, wait_for_exit) = oneshot::channel::<()>();
    let approved = spawn_required_loop_future(
        TaskId::NotificationMaintenance,
        generation,
        Arc::clone(&control),
        Arc::clone(&lifecycle),
        async move {
            let _ = wait_for_exit.await;
        },
    );
    let sibling = spawn_required_loop_future(
        TaskId::TimeSync,
        generation,
        Arc::clone(&control),
        Arc::clone(&lifecycle),
        std::future::pending(),
    );
    // Batch wrapping pops from the end, so the first startup channel belongs
    // to NotificationMaintenance and can vote before TimeSync.
    let mut batch: SchedulerHandleBatch = vec![
        (TaskId::TimeSync, sibling),
        (TaskId::NotificationMaintenance, approved),
    ]
    .into_iter()
    .collect();
    batch.attach(Arc::clone(&control));
    let tasks = [TaskId::TimeSync, TaskId::NotificationMaintenance];
    assert!(lifecycle
        .lock()
        .unwrap()
        .register_required_loops(generation, &tasks));
    let (supervised, mut startup, _commit_result) =
        supervise_scheduler_batch(generation, &control, &lifecycle, batch, |_, _| {});
    lifecycle
        .lock()
        .unwrap()
        .install_handles(generation, supervised)
        .unwrap();
    for (ready, _) in &mut startup {
        assert!(ready.await.unwrap());
    }

    let (_, approved_commit) = startup.remove(0);
    approved_commit.send(()).unwrap();
    loop {
        if lifecycle
            .lock()
            .unwrap()
            .required_loops
            .get(&TaskId::NotificationMaintenance)
            == Some(&RequiredLoopStatus::CommitApproved)
        {
            break;
        }
        tokio::task::yield_now().await;
    }

    exit_approved.send(()).unwrap();
    let cleanup = loop {
        let lifecycle = lifecycle.lock().unwrap();
        if lifecycle.state == (SchedulerLifecycleState::Stopping { generation }) {
            break lifecycle.cleanup_completion.clone().unwrap();
        }
        drop(lifecycle);
        tokio::task::yield_now().await;
    };
    assert_eq!(lifecycle.lock().unwrap().running_generation(), None);
    let (_, sibling_commit) = startup.remove(0);
    let _ = sibling_commit.send(());
    control.owner_finished();
    wait_for_cleanup(cleanup).await;
    assert_eq!(
        lifecycle.lock().unwrap().state,
        SchedulerLifecycleState::Stopped
    );
}

#[tokio::test]
async fn start_overlapping_atomic_stop_waits_then_starts_a_replacement() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    assert!(
        start_scheduler_lifecycle(
            &lifecycle,
            &running,
            |_| async {},
            |_, _, _, _, ()| (Vec::new(), async {}),
        )
        .await
    );

    let stop_request = lifecycle.lock().unwrap().begin_explicit_stop();
    assert!(matches!(stop_request, CleanupRequest::Start(_)));
    assert!(matches!(
        lifecycle.lock().unwrap().state,
        SchedulerLifecycleState::Stopping { generation: 1 }
    ));

    let installs = Arc::new(AtomicUsize::new(0));
    let replacement = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let installs = Arc::clone(&installs);
        async move {
            start_scheduler_lifecycle(
                &lifecycle,
                &running,
                |_| async {},
                move |_, _, _, _, ()| {
                    installs.fetch_add(1, Ordering::SeqCst);
                    (Vec::new(), async {})
                },
            )
            .await
        }
    });
    loop {
        if lifecycle.lock().unwrap().desired.is_some() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(installs.load(Ordering::SeqCst), 0);

    let completion = launch_scheduler_cleanup(&lifecycle, stop_request).unwrap();
    wait_for_cleanup(completion).await;
    assert!(replacement.await.unwrap());
    assert_eq!(installs.load(Ordering::SeqCst), 1);
    assert_eq!(lifecycle.lock().unwrap().running_generation(), Some(2));
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test]
async fn cleanup_primary_launch_failure_keeps_job_owned_until_fallback_finishes() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let generation = lifecycle.lock().unwrap().begin_start().unwrap().0;
    let resources = match lifecycle.lock().unwrap().begin_cleanup(generation) {
        CleanupRequest::Start(resources) => resources,
        _ => panic!("active generation must produce cleanup resources"),
    };
    let CleanupResources {
        generation,
        handles: _,
        run_states: _,
        generation_running: _,
        control,
        completion,
        receiver,
        recovery: _,
    } = resources;
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
    handle.abort();
    let job = Arc::new(StdMutex::new(Some(CleanupJob {
        handles: vec![handle],
        control,
        guard: CleanupCompletionGuard {
            lifecycle: Arc::clone(&lifecycle),
            generation,
            completion: Some(completion),
        },
    })));

    // `false` deterministically skips the primary branch and exercises the
    // retained-Arc fallback on this same executor.
    launch_cleanup_job_inner(job, false);
    wait_for_cleanup(receiver).await;
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(
        lifecycle.lock().unwrap().state,
        SchedulerLifecycleState::Stopped
    );
}

#[test]
fn generation_handle_drops_outside_tokio_only_defer_and_never_panic() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let control = GenerationControl::new();
    let entered = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let make_handle = || {
        let entered = Arc::clone(&entered);
        let dropped = Arc::clone(&dropped);
        tauri::async_runtime::JoinHandle::Tokio(runtime.spawn(async move {
            let _probe = DropProbe(dropped);
            entered.fetch_add(1, Ordering::SeqCst);
            std::future::pending::<()>().await;
        }))
    };
    let owned = GenerationOwnedSchedulerHandle::new(Arc::clone(&control), make_handle());
    let mut batch: SchedulerHandleBatch = vec![(TaskId::TimeSync, make_handle())]
        .into_iter()
        .collect();
    batch.attach(Arc::clone(&control));
    runtime.block_on(async {
        while entered.load(Ordering::SeqCst) != 2 {
            tokio::task::yield_now().await;
        }
    });

    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(owned);
        drop(batch);
    }))
    .is_ok());
    runtime.block_on(control.wait_drained());
    assert_eq!(dropped.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partial_install_panic_cannot_outlive_cleanup_or_overlap_replacement() {
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
    let drop_entered = Arc::new(tokio::sync::Notify::new());
    let release_drop = Arc::new((StdMutex::new(false), Condvar::new()));
    let failed_start = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let drop_entered = Arc::clone(&drop_entered);
        let release_drop = Arc::clone(&release_drop);
        async move {
            start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
                let lifecycle = Arc::clone(&lifecycle);
                move |generation, _, _, control, ()| {
                    let cancellation = wait_for_generation_cancel(control.subscribe());
                    let _detached_on_unwind = spawn_required_loop_future(
                        TaskId::NotificationMaintenance,
                        generation,
                        control,
                        Arc::clone(&lifecycle),
                        async move {
                            let _probe = BlockingDrop {
                                entered: drop_entered,
                                release: release_drop,
                            };
                            cancellation.await;
                        },
                    );
                    panic!("synthetic partial install panic");
                    #[allow(unreachable_code)]
                    (Vec::new(), async {})
                }
            })
            .await
        }
    });

    drop_entered.notified().await;
    assert!(matches!(
        lifecycle.lock().unwrap().state,
        SchedulerLifecycleState::Stopping { generation: 1 }
    ));
    let replacement = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        async move {
            start_scheduler_lifecycle(
                &lifecycle,
                &running,
                |_| async {},
                |_, _, _, _, ()| (Vec::new(), async {}),
            )
            .await
        }
    });
    tokio::task::yield_now().await;
    assert!(!replacement.is_finished());

    let (released, wake) = release_drop.as_ref();
    *released.lock().unwrap() = true;
    wake.notify_all();
    assert!(failed_start.await.unwrap_err().is_panic());
    assert!(replacement.await.unwrap());
    assert_eq!(lifecycle.lock().unwrap().running_generation(), Some(2));
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

fn probed_pending_handles(
    entered: &Arc<AtomicUsize>,
    dropped: &Arc<AtomicUsize>,
    tasks: &[TaskId],
) -> SchedulerHandleBatch {
    tasks
        .iter()
        .copied()
        .map(|task| {
            let entered = Arc::clone(entered);
            let dropped = Arc::clone(dropped);
            let handle = tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
                let _probe = DropProbe(dropped);
                entered.fetch_add(1, Ordering::SeqCst);
                std::future::pending::<()>().await;
            }));
            (task, handle)
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn panic_at_every_partial_wrap_stage_still_drains_all_generation_handles() {
    let tasks = [
        TaskId::TimeSync,
        TaskId::FundingRate,
        TaskId::NotificationMaintenance,
    ];
    for panic_stage in [
        HandleWrapStage::BeforePop,
        HandleWrapStage::Owned,
        HandleWrapStage::Spawned,
    ] {
        let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
        let running = Arc::new(AtomicBool::new(false));
        let (generation, control) = match lifecycle.lock().unwrap().start_decision() {
            SchedulerStartDecision::Begin {
                generation,
                control,
                ..
            } => (generation, control),
            _ => panic!("test generation must begin"),
        };
        let entered = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut batch = probed_pending_handles(&entered, &dropped, &tasks);
        while entered.load(Ordering::SeqCst) != tasks.len() {
            tokio::task::yield_now().await;
        }
        batch.attach(Arc::clone(&control));
        assert!(lifecycle
            .lock()
            .unwrap()
            .register_required_loops(generation, &tasks));
        let lifecycle_for_wrap = Arc::clone(&lifecycle);
        let control_for_wrap = Arc::clone(&control);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _ = supervise_scheduler_batch(
                generation,
                &control_for_wrap,
                &lifecycle_for_wrap,
                batch,
                |stage, index| {
                    if stage == panic_stage && index == 1 {
                        panic!("synthetic partial wrapping panic at {stage:?}");
                    }
                },
            );
        }));
        assert!(result.is_err());

        let completion = request_scheduler_cleanup(&lifecycle, &running, generation)
            .expect("panic cleanup must be observable");
        control.owner_finished();
        wait_for_cleanup(completion).await;
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            tasks.len(),
            "all handles must drain after a {panic_stage:?} panic"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_supervisors_remain_generation_owned_after_caller_drops_them() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let (generation, control) = match lifecycle.lock().unwrap().start_decision() {
        SchedulerStartDecision::Begin {
            generation,
            control,
            ..
        } => (generation, control),
        _ => panic!("test generation must begin"),
    };
    let tasks = [TaskId::TimeSync, TaskId::NotificationMaintenance];
    let entered = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut batch = probed_pending_handles(&entered, &dropped, &tasks);
    while entered.load(Ordering::SeqCst) != tasks.len() {
        tokio::task::yield_now().await;
    }
    batch.attach(Arc::clone(&control));
    assert!(lifecycle
        .lock()
        .unwrap()
        .register_required_loops(generation, &tasks));

    let completion = request_scheduler_cleanup(&lifecycle, &running, generation)
        .expect("stop seals the starting generation");
    let (supervised, startup_channels, _commit_result) =
        supervise_scheduler_batch(generation, &control, &lifecycle, batch, |_, _| {});
    let rejected = lifecycle
        .lock()
        .unwrap()
        .install_handles(generation, supervised)
        .expect_err("a sealed generation rejects late supervisors");
    drop(startup_channels);
    drop(rejected);
    control.owner_finished();

    wait_for_cleanup(completion).await;
    assert_eq!(dropped.load(Ordering::SeqCst), tasks.len());
    assert_eq!(
        lifecycle.lock().unwrap().state,
        SchedulerLifecycleState::Stopped
    );
}

#[test]
fn cleanup_and_owner_drop_recover_poisoned_mutexes_without_double_panicking() {
    let control = GenerationControl::new();
    let owner = GenerationOwnerGuard::new(Arc::clone(&control));
    let poison_control = Arc::clone(&control);
    assert!(std::thread::spawn(move || {
        let _guard = poison_control.drain.lock().unwrap();
        panic!("synthetic owner mutex poison");
    })
    .join()
    .is_err());
    drop(owner);
    assert_eq!(lock_unpoisoned(&control.drain).active_owners, 0);

    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let generation = lifecycle.lock().unwrap().begin_start().unwrap().0;
    let resources = match lifecycle.lock().unwrap().begin_cleanup(generation) {
        CleanupRequest::Start(resources) => resources,
        _ => panic!("active generation must yield cleanup resources"),
    };
    let guard = CleanupCompletionGuard {
        lifecycle: Arc::clone(&lifecycle),
        generation,
        completion: Some(resources.completion),
    };
    let poison_lifecycle = Arc::clone(&lifecycle);
    assert!(std::thread::spawn(move || {
        let _guard = poison_lifecycle.lock().unwrap();
        panic!("synthetic lifecycle mutex poison");
    })
    .join()
    .is_err());
    drop(guard);
    assert_eq!(
        lock_unpoisoned(&lifecycle).state,
        SchedulerLifecycleState::Stopped
    );
}
