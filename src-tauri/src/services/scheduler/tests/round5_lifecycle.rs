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

fn pending_required_handle() -> SchedulerHandle {
    tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(std::future::pending()))
}

fn install_round5_recovery_handler(
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
        Box::pin(async move {
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
                    (
                        vec![(TaskId::NotificationMaintenance, pending_required_handle())],
                        async {},
                    )
                },
            )
            .await;
        })
    }));
}

fn retained_cleanup_slot(
    hold_generation_owner: bool,
) -> (
    Arc<StdMutex<SchedulerLifecycle>>,
    Arc<CleanupJobSlot>,
    watch::Receiver<bool>,
    Arc<GenerationControl>,
) {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let (generation, _, _, control) = lock_unpoisoned(&lifecycle)
        .begin_start()
        .expect("cleanup fixture starts a generation");
    if hold_generation_owner {
        control.owner_started();
    }
    let resources = match lock_unpoisoned(&lifecycle).begin_cleanup(generation) {
        CleanupRequest::Start(resources) => resources,
        _ => panic!("active generation must yield cleanup resources"),
    };
    let receiver = resources.receiver.clone();
    let slot = CleanupJobSlot::new(CleanupJob {
        handles: resources.handles,
        control: resources.control,
        guard: CleanupCompletionGuard {
            lifecycle: Arc::clone(&lifecycle),
            generation,
            completion: Some(resources.completion),
        },
    });
    lock_unpoisoned(&lifecycle).cleanup_job = Some(Arc::clone(&slot));
    (lifecycle, slot, receiver, control)
}

#[tokio::test]
async fn exit_before_start_driver_first_poll_cannot_create_or_commit_after_exit() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let installs = Arc::new(AtomicUsize::new(0));
    let claimed = claim_scheduler_start(&lifecycle, SchedulerStartOrigin::Explicit);

    assert_eq!(
        lock_unpoisoned(&lifecycle).state,
        SchedulerLifecycleState::Starting { generation: 1 },
        "the app boundary must publish its start claim synchronously"
    );

    let shutdown = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        async move { shutdown_scheduler_lifecycle(&lifecycle).await }
    });
    while lock_unpoisoned(&lifecycle).state != (SchedulerLifecycleState::Stopping { generation: 1 })
    {
        tokio::task::yield_now().await;
    }

    let started = drive_claimed_scheduler_start(claimed, None, |_| async {}, {
        let installs = Arc::clone(&installs);
        move |_, _, _, _, ()| {
            installs.fetch_add(1, Ordering::SeqCst);
            (Vec::new(), async {})
        }
    })
    .await;

    assert!(!started);
    assert!(shutdown.await.unwrap());
    let lifecycle = lock_unpoisoned(&lifecycle);
    assert_eq!(lifecycle.state, SchedulerLifecycleState::Stopped);
    assert!(lifecycle.terminal);
    assert_eq!(lifecycle.next_generation, 1);
    assert_eq!(installs.load(Ordering::SeqCst), 0);
    assert!(lifecycle.desired.is_none());
}

#[tokio::test]
async fn connection_bootstrap_claim_before_start_driver_poll_is_not_silently_skipped() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let claimed = claim_scheduler_start(&lifecycle, SchedulerStartOrigin::Explicit);
    let bootstrap_runs = Arc::new(AtomicUsize::new(0));

    run_generation_activity(&lifecycle, {
        let bootstrap_runs = Arc::clone(&bootstrap_runs);
        move || async move {
            bootstrap_runs.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    })
    .await
    .unwrap();

    assert_eq!(bootstrap_runs.load(Ordering::SeqCst), 1);
    assert!(
        drive_claimed_scheduler_start(
            claimed,
            None,
            |_| async {},
            |_, _, _, _, ()| (Vec::new(), async {}),
        )
        .await
    );
    assert!(stop_scheduler_lifecycle_inner(&lifecycle).await);
}

#[tokio::test(start_paused = true)]
async fn cancelled_initial_start_with_concurrent_caller_recovers_without_external_restart() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let recovery_starts = Arc::new(AtomicUsize::new(0));
    install_round5_recovery_handler(&lifecycle, &running, Arc::clone(&recovery_starts));
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());

    let first = tokio::spawn({
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
                |_, _, _, _, ()| (Vec::new(), async {}),
            )
            .await
        }
    });
    entered.notified().await;

    assert!(
        !start_scheduler_lifecycle(
            &lifecycle,
            &running,
            |_| async {},
            |_, _, _, _, ()| (Vec::new(), async {}),
        )
        .await,
        "the concurrent caller must share the already-published desired intent"
    );
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());

    while lock_unpoisoned(&lifecycle).state != SchedulerLifecycleState::Stopped {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_millis(99)).await;
    tokio::task::yield_now().await;
    assert_eq!(recovery_starts.load(Ordering::SeqCst), 0);
    tokio::time::advance(Duration::from_millis(1)).await;
    while lock_unpoisoned(&lifecycle).running_generation() != Some(2) {
        tokio::task::yield_now().await;
    }

    assert_eq!(recovery_starts.load(Ordering::SeqCst), 1);
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test(start_paused = true)]
async fn panicked_initial_start_recovers_without_external_restart() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let recovery_starts = Arc::new(AtomicUsize::new(0));
    install_round5_recovery_handler(&lifecycle, &running, Arc::clone(&recovery_starts));

    let failed = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        async move {
            start_scheduler_lifecycle(
                &lifecycle,
                &running,
                |_| async {},
                |_, _, _, _, ()| -> (Vec<(TaskId, SchedulerHandle)>, std::future::Ready<()>) {
                    panic!("synthetic initial install panic")
                },
            )
            .await
        }
    });
    assert!(failed.await.unwrap_err().is_panic());
    while lock_unpoisoned(&lifecycle).state != SchedulerLifecycleState::Stopped {
        tokio::task::yield_now().await;
    }

    tokio::time::advance(Duration::from_millis(100)).await;
    while lock_unpoisoned(&lifecycle).running_generation() != Some(2) {
        tokio::task::yield_now().await;
    }
    assert_eq!(recovery_starts.load(Ordering::SeqCst), 1);
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test(start_paused = true)]
async fn cancelled_start_waiting_for_cleanup_retains_desired_recovery_owner() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let recovery_starts = Arc::new(AtomicUsize::new(0));
    install_round5_recovery_handler(&lifecycle, &running, Arc::clone(&recovery_starts));
    assert!(
        start_scheduler_lifecycle(
            &lifecycle,
            &running,
            |_| async {},
            |_, _, _, _, ()| {
                (
                    vec![(TaskId::NotificationMaintenance, pending_required_handle())],
                    async {},
                )
            },
        )
        .await
    );
    let stop_request = lock_unpoisoned(&lifecycle).begin_explicit_stop();

    let queued = tokio::spawn({
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
    while lock_unpoisoned(&lifecycle).desired.is_none() {
        tokio::task::yield_now().await;
    }
    queued.abort();
    assert!(queued.await.unwrap_err().is_cancelled());
    assert!(lock_unpoisoned(&lifecycle).pending_recovery.is_some());

    let completion = launch_scheduler_cleanup(&lifecycle, stop_request)
        .expect("stopping generation has a cleanup completion");
    wait_for_owned_cleanup(&lifecycle, completion).await;
    tokio::time::advance(Duration::from_millis(100)).await;
    while lock_unpoisoned(&lifecycle).running_generation() != Some(2) {
        tokio::task::yield_now().await;
    }
    assert_eq!(recovery_starts.load(Ordering::SeqCst), 1);
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test]
async fn queued_start_relaunches_cleanup_after_original_stop_and_worker_are_cancelled() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let held_control = Arc::new(StdMutex::new(None::<Arc<GenerationControl>>));
    assert!(
        start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
            let held_control = Arc::clone(&held_control);
            move |_, _, _, control, ()| {
                control.owner_started();
                *lock_unpoisoned(&held_control) = Some(control);
                (
                    vec![(TaskId::NotificationMaintenance, pending_required_handle())],
                    async {},
                )
            }
        })
        .await
    );

    let stop = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        async move { stop_scheduler_lifecycle_inner(&lifecycle).await }
    });
    let slot = loop {
        let slot = lock_unpoisoned(&lifecycle).cleanup_job.clone();
        if let Some(slot) = slot {
            if lock_unpoisoned(&slot.state).job.is_none() {
                break slot;
            }
        }
        tokio::task::yield_now().await;
    };
    assert!(slot.abort_latest_worker());
    stop.abort();
    assert!(stop.await.unwrap_err().is_cancelled());
    while {
        let slot = lock_unpoisoned(&slot.state);
        slot.worker_active || slot.job.is_none()
    } {
        tokio::task::yield_now().await;
    }

    let installs = Arc::new(AtomicUsize::new(0));
    let replacement = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let installs = Arc::clone(&installs);
        async move {
            start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
                move |_, _, _, _, ()| {
                    installs.fetch_add(1, Ordering::SeqCst);
                    (
                        vec![(TaskId::NotificationMaintenance, pending_required_handle())],
                        async {},
                    )
                }
            })
            .await
        }
    });
    tokio::task::yield_now().await;
    assert_eq!(installs.load(Ordering::SeqCst), 0);
    lock_unpoisoned(&held_control)
        .take()
        .expect("fixture holds a generation owner")
        .owner_finished();

    assert!(
        tokio::time::timeout(Duration::from_millis(200), replacement)
            .await
            .expect("the queued start must own and relaunch retained cleanup")
            .unwrap()
    );
    assert_eq!(installs.load(Ordering::SeqCst), 1);
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test]
async fn queued_start_crosses_stopping_to_job_install_gap_and_recovers_cancelled_worker() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let held_control = Arc::new(StdMutex::new(None::<Arc<GenerationControl>>));
    assert!(
        start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
            let held_control = Arc::clone(&held_control);
            move |_, _, _, control, ()| {
                control.owner_started();
                *lock_unpoisoned(&held_control) = Some(control);
                (
                    vec![(TaskId::NotificationMaintenance, pending_required_handle())],
                    async {},
                )
            }
        })
        .await
    );
    let request = lock_unpoisoned(&lifecycle).begin_explicit_stop();
    assert!(lock_unpoisoned(&lifecycle).cleanup_job.is_none());

    let installs = Arc::new(AtomicUsize::new(0));
    let replacement = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let installs = Arc::clone(&installs);
        async move {
            start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
                move |_, _, _, _, ()| {
                    installs.fetch_add(1, Ordering::SeqCst);
                    (
                        vec![(TaskId::NotificationMaintenance, pending_required_handle())],
                        async {},
                    )
                }
            })
            .await
        }
    });
    tokio::task::yield_now().await;
    let _completion = launch_scheduler_cleanup(&lifecycle, request)
        .expect("the delayed cleanup request retains completion");
    let slot = loop {
        let slot = lock_unpoisoned(&lifecycle)
            .cleanup_job
            .clone()
            .expect("cleanup installs its durable slot");
        if lock_unpoisoned(&slot.state).job.is_none() {
            break slot;
        }
        tokio::task::yield_now().await;
    };
    assert!(slot.abort_latest_worker());
    lock_unpoisoned(&held_control)
        .take()
        .expect("fixture holds a generation owner")
        .owner_finished();

    assert!(
        tokio::time::timeout(Duration::from_millis(200), replacement)
            .await
            .expect("the pre-slot waiter must adopt and relaunch cleanup")
            .unwrap()
    );
    assert_eq!(installs.load(Ordering::SeqCst), 1);
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test(start_paused = true)]
async fn cancelled_recovery_attempt_rearms_with_the_next_bounded_backoff() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let recovery_attempts = Arc::new(AtomicUsize::new(0));
    let second_prepare_entered = Arc::new(tokio::sync::Notify::new());
    let second_prepare_release = Arc::new(tokio::sync::Notify::new());

    let handler_lifecycle = Arc::clone(&lifecycle);
    let handler_running = Arc::clone(&running);
    let handler_attempts = Arc::clone(&recovery_attempts);
    let handler_entered = Arc::clone(&second_prepare_entered);
    let handler_release = Arc::clone(&second_prepare_release);
    lock_unpoisoned(&lifecycle).recovery_handler = Some(Arc::new(move |ticket, completion| {
        let lifecycle = Arc::clone(&handler_lifecycle);
        let running = Arc::clone(&handler_running);
        let attempts = Arc::clone(&handler_attempts);
        let entered = Arc::clone(&handler_entered);
        let release = Arc::clone(&handler_release);
        Box::pin(async move {
            wait_for_cleanup(completion).await;
            tokio::time::sleep(recovery_backoff(ticket.attempt)).await;
            attempts.fetch_add(1, Ordering::SeqCst);
            let block_attempt = ticket.attempt == 1;
            let _ = start_scheduler_lifecycle_for_recovery(
                &lifecycle,
                &running,
                ticket,
                move |_| async move {
                    if block_attempt {
                        entered.notify_one();
                        release.notified().await;
                    }
                },
                |_, _, _, _, ()| {
                    (
                        vec![(TaskId::NotificationMaintenance, pending_required_handle())],
                        async {},
                    )
                },
            )
            .await;
        })
    }));

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
    while lock_unpoisoned(&lifecycle).state != SchedulerLifecycleState::Stopped {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_millis(100)).await;
    second_prepare_entered.notified().await;

    lock_unpoisoned(&lifecycle)
        .recovery_worker
        .as_ref()
        .expect("recovery worker is lifecycle-owned")
        .abort();
    while lock_unpoisoned(&lifecycle).state != SchedulerLifecycleState::Stopped {
        tokio::task::yield_now().await;
    }

    tokio::time::advance(Duration::from_millis(199)).await;
    tokio::task::yield_now().await;
    assert_eq!(recovery_attempts.load(Ordering::SeqCst), 1);
    tokio::time::advance(Duration::from_millis(1)).await;
    while lock_unpoisoned(&lifecycle).running_generation() != Some(3) {
        tokio::task::yield_now().await;
    }

    assert_eq!(recovery_attempts.load(Ordering::SeqCst), 2);
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test(start_paused = true)]
async fn panicked_recovery_attempt_rearms_with_the_next_bounded_backoff() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let recovery_attempts = Arc::new(AtomicUsize::new(0));
    let handler_lifecycle = Arc::clone(&lifecycle);
    let handler_running = Arc::clone(&running);
    let handler_attempts = Arc::clone(&recovery_attempts);
    lock_unpoisoned(&lifecycle).recovery_handler = Some(Arc::new(move |ticket, completion| {
        let lifecycle = Arc::clone(&handler_lifecycle);
        let running = Arc::clone(&handler_running);
        let attempts = Arc::clone(&handler_attempts);
        Box::pin(async move {
            wait_for_cleanup(completion).await;
            tokio::time::sleep(recovery_backoff(ticket.attempt)).await;
            attempts.fetch_add(1, Ordering::SeqCst);
            if ticket.attempt == 1 {
                let _ = start_scheduler_lifecycle_for_recovery(
                    &lifecycle,
                    &running,
                    ticket,
                    |_| async {},
                    |_, _, _, _, ()| -> (Vec<(TaskId, SchedulerHandle)>, std::future::Ready<()>) {
                        panic!("synthetic recovery install panic")
                    },
                )
                .await;
            } else {
                let _ = start_scheduler_lifecycle_for_recovery(
                    &lifecycle,
                    &running,
                    ticket,
                    |_| async {},
                    |_, _, _, _, ()| {
                        (
                            vec![(TaskId::NotificationMaintenance, pending_required_handle())],
                            async {},
                        )
                    },
                )
                .await;
            }
        })
    }));

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
    while lock_unpoisoned(&lifecycle).state != SchedulerLifecycleState::Stopped {
        tokio::task::yield_now().await;
    }

    tokio::time::advance(Duration::from_millis(100)).await;
    while recovery_attempts.load(Ordering::SeqCst) != 1
        || lock_unpoisoned(&lifecycle)
            .pending_recovery
            .as_ref()
            .map(|ticket| ticket.attempt)
            != Some(2)
    {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_millis(199)).await;
    tokio::task::yield_now().await;
    assert_eq!(recovery_attempts.load(Ordering::SeqCst), 1);
    tokio::time::advance(Duration::from_millis(1)).await;
    while lock_unpoisoned(&lifecycle).running_generation() != Some(3) {
        tokio::task::yield_now().await;
    }

    assert_eq!(recovery_attempts.load(Ordering::SeqCst), 2);
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test]
async fn explicit_stop_drains_recovery_owners_even_after_visible_handle_replacement() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let desired = DesiredRunToken::new();
    let ticket = RecoveryTicket {
        failed_generation: 1,
        attempt: 1,
        desired: Arc::clone(&desired),
    };
    let entered = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let handler: RecoveryHandler = Arc::new({
        let entered = Arc::clone(&entered);
        let dropped = Arc::clone(&dropped);
        move |ticket, _| {
            let entered = Arc::clone(&entered);
            let dropped = Arc::clone(&dropped);
            Box::pin(async move {
                let _probe = DropProbe(dropped);
                entered.fetch_add(1, Ordering::SeqCst);
                wait_for_generation_cancel(ticket.desired.subscribe()).await;
            })
        }
    });
    {
        let mut state = lock_unpoisoned(&lifecycle);
        state.desired = Some(desired);
        state.pending_recovery = Some(ticket.clone());
        state.recovery_handler = Some(Arc::clone(&handler));
    }

    launch_recovery_handler(
        &lifecycle,
        Arc::clone(&handler),
        ticket.clone(),
        completed_cleanup_receiver(),
    );
    launch_recovery_handler(&lifecycle, handler, ticket, completed_cleanup_receiver());
    while entered.load(Ordering::SeqCst) != 2
        || lock_unpoisoned(&lifecycle).recovery_drain.active_count() != 2
    {
        tokio::task::yield_now().await;
    }

    assert!(!stop_scheduler_lifecycle_inner(&lifecycle).await);
    let state = lock_unpoisoned(&lifecycle);
    assert_eq!(state.recovery_drain.active_count(), 0);
    assert_eq!(dropped.load(Ordering::SeqCst), 2);
    assert!(state.desired.is_none());
    assert!(state.pending_recovery.is_none());
}

#[test]
fn permanent_generation_exhaustion_clears_active_desired_intent() {
    let mut lifecycle = SchedulerLifecycle::new();
    lifecycle.next_generation = u64::MAX;

    assert!(matches!(
        lifecycle.start_decision(),
        SchedulerStartDecision::Exhausted
    ));
    assert!(lifecycle.generation_exhausted);
    assert!(lifecycle.desired.is_none());
    assert!(lifecycle.pending_recovery.is_none());
}

#[tokio::test]
async fn cleanup_spawn_panic_retains_job_until_a_fallback_acknowledges_success() {
    let (lifecycle, slot, receiver, _) = retained_cleanup_slot(false);

    assert!(!attempt_cleanup_spawn(&slot, |_| {
        panic!("synthetic cleanup spawn panic")
    }));
    assert!(lock_unpoisoned(&slot.state).job.is_some());
    assert_eq!(
        lock_unpoisoned(&lifecycle).state,
        SchedulerLifecycleState::Stopping { generation: 1 }
    );
    assert!(!*receiver.borrow());

    launch_cleanup_job(Arc::clone(&slot));
    wait_for_owned_cleanup(&lifecycle, receiver).await;
    assert_eq!(
        lock_unpoisoned(&lifecycle).state,
        SchedulerLifecycleState::Stopped
    );
}

#[test]
fn cleanup_cancelled_before_first_poll_returns_launch_reservation_and_job() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (lifecycle, slot, receiver, _) = retained_cleanup_slot(false);

    assert!(spawn_cleanup_on_runtime(&slot, runtime.handle()));
    assert!(slot.abort_latest_worker());
    runtime.block_on(async {
        while lock_unpoisoned(&slot.state).worker_pending != 0 {
            tokio::task::yield_now().await;
        }
        assert!(lock_unpoisoned(&slot.state).job.is_some());
        assert_eq!(
            lock_unpoisoned(&lifecycle).state,
            SchedulerLifecycleState::Stopping { generation: 1 }
        );
        launch_cleanup_job(Arc::clone(&slot));
        wait_for_owned_cleanup(&lifecycle, receiver).await;
    });
    assert_eq!(
        lock_unpoisoned(&lifecycle).state,
        SchedulerLifecycleState::Stopped
    );
}

#[tokio::test]
async fn cleanup_cancelled_after_take_returns_job_and_resumes_without_false_stopped() {
    let (lifecycle, slot, receiver, control) = retained_cleanup_slot(true);
    launch_cleanup_job(Arc::clone(&slot));
    while lock_unpoisoned(&slot.state).job.is_some() {
        tokio::task::yield_now().await;
    }
    assert!(slot.abort_latest_worker());
    while lock_unpoisoned(&slot.state).job.is_none() {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        lock_unpoisoned(&lifecycle).state,
        SchedulerLifecycleState::Stopping { generation: 1 }
    );
    assert!(!*receiver.borrow());

    control.owner_finished();
    launch_cleanup_job(Arc::clone(&slot));
    wait_for_owned_cleanup(&lifecycle, receiver).await;
    assert_eq!(
        lock_unpoisoned(&lifecycle).state,
        SchedulerLifecycleState::Stopped
    );
}

#[test]
fn ambiguous_dual_cleanup_launch_reserves_only_one_unpolled_worker() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (lifecycle, slot, receiver, _) = retained_cleanup_slot(false);

    assert!(spawn_cleanup_on_runtime(&slot, runtime.handle()));
    assert!(spawn_cleanup_on_runtime(&slot, runtime.handle()));
    assert_eq!(lock_unpoisoned(&slot.state).workers.len(), 1);
    assert_eq!(lock_unpoisoned(&slot.state).worker_pending, 1);

    runtime.block_on(wait_for_owned_cleanup(&lifecycle, receiver));
    assert_eq!(
        lock_unpoisoned(&lifecycle).state,
        SchedulerLifecycleState::Stopped
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_generation_drain_requeues_owned_deferred_batch() {
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

    let control = GenerationControl::new();
    let started = Arc::new(tokio::sync::Notify::new());
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new((StdMutex::new(false), Condvar::new()));
    let handle = tauri::async_runtime::JoinHandle::Tokio(tokio::spawn({
        let entered = Arc::clone(&entered);
        let started = Arc::clone(&started);
        let release = Arc::clone(&release);
        async move {
            let _probe = BlockingDrop { entered, release };
            started.notify_one();
            std::future::pending::<()>().await;
        }
    }));
    started.notified().await;
    control.defer_handles(vec![handle], None);

    let drain = tokio::spawn({
        let control = Arc::clone(&control);
        async move { control.wait_drained().await }
    });
    entered.notified().await;
    drain.abort();
    while lock_unpoisoned(&control.drain).deferred_handles.is_empty() {
        tokio::task::yield_now().await;
    }
    assert_eq!(lock_unpoisoned(&control.drain).active_owners, 1);

    let (released, wake) = release.as_ref();
    *released.lock().unwrap() = true;
    wake.notify_all();
    assert!(drain.await.unwrap_err().is_cancelled());
    control.wait_drained().await;
    assert_eq!(lock_unpoisoned(&control.drain).active_owners, 0);
}

#[tokio::test]
async fn cancelled_market_backfill_is_dropped_before_restart_can_enter_replacement() {
    let previous_connected = Arc::new(Mutex::new(false));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let stale_events = Arc::new(AtomicUsize::new(0));
    let first_entered = Arc::new(tokio::sync::Notify::new());
    let first_dropped = Arc::new(AtomicUsize::new(0));

    let first = tokio::spawn(run_market_reconnect_backfill(
        Arc::clone(&previous_connected),
        true,
        {
            let in_flight = Arc::clone(&in_flight);
            let max_in_flight = Arc::clone(&max_in_flight);
            let stale_events = Arc::clone(&stale_events);
            let entered = Arc::clone(&first_entered);
            let dropped = Arc::clone(&first_dropped);
            move || async move {
                let _probe = DropProbe(dropped);
                let active = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                max_in_flight.fetch_max(active, Ordering::SeqCst);
                entered.notify_one();
                std::future::pending::<()>().await;
                #[allow(unreachable_code)]
                {
                    stale_events.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            }
        },
    ));
    first_entered.notified().await;
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    in_flight.fetch_sub(1, Ordering::SeqCst);
    assert_eq!(first_dropped.load(Ordering::SeqCst), 1);

    let replacement_ran = Arc::new(AtomicUsize::new(0));
    run_market_reconnect_backfill(Arc::clone(&previous_connected), true, {
        let in_flight = Arc::clone(&in_flight);
        let max_in_flight = Arc::clone(&max_in_flight);
        let replacement_ran = Arc::clone(&replacement_ran);
        move || async move {
            let active = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            max_in_flight.fetch_max(active, Ordering::SeqCst);
            replacement_ran.fetch_add(1, Ordering::SeqCst);
            in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    })
    .await
    .unwrap();

    assert_eq!(replacement_ran.load(Ordering::SeqCst), 1);
    assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);
    assert_eq!(stale_events.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn scheduler_market_backfill_does_not_reacquire_a_fair_account_read_guard() {
    let coordinator = Arc::new(AccountLifecycleCoordinator::new());
    let outer_entered = Arc::new(tokio::sync::Notify::new());
    let inner_ran = Arc::new(AtomicUsize::new(0));
    let (release_inner, released_inner) = tokio::sync::oneshot::channel();

    let operation = tokio::spawn({
        let coordinator = Arc::clone(&coordinator);
        let outer_entered = Arc::clone(&outer_entered);
        let inner_ran = Arc::clone(&inner_ran);
        async move {
            execute_task_with_account_lifecycle(
                coordinator.as_ref(),
                TaskId::MarketFallback,
                move || async move {
                    outer_entered.notify_one();
                    let _ = released_inner.await;
                    run_scheduler_owned_market_backfill(|| async move {
                        inner_ran.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                    .await
                },
            )
            .await
        }
    });
    outer_entered.notified().await;

    // Polling the writer once queues it behind the outer scheduler read. A
    // nested read would now queue behind this writer and deadlock.
    let mutation = coordinator.mutation_guard();
    tokio::pin!(mutation);
    assert!(matches!(
        futures_util::poll!(mutation.as_mut()),
        std::task::Poll::Pending
    ));
    release_inner.send(()).unwrap();

    tokio::time::timeout(Duration::from_secs(1), operation)
        .await
        .expect("owned backfill must not block behind the queued writer")
        .unwrap()
        .unwrap();
    assert_eq!(inner_ran.load(Ordering::SeqCst), 1);
    let _mutation = tokio::time::timeout(Duration::from_secs(1), mutation.as_mut())
        .await
        .expect("writer proceeds after the scheduler releases its outer read");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_and_restart_wait_for_generation_owned_kline_storage_without_stale_event() {
    let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
    let running = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(tokio::sync::Notify::new());
    let begin_storage = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new((StdMutex::new(false), Condvar::new()));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let stale_events = Arc::new(AtomicUsize::new(0));
    let task_lifecycle = Arc::clone(&lifecycle);

    assert!(
        start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
            let entered = Arc::clone(&entered);
            let begin_storage = Arc::clone(&begin_storage);
            let release = Arc::clone(&release);
            let in_flight = Arc::clone(&in_flight);
            let max_in_flight = Arc::clone(&max_in_flight);
            let stale_events = Arc::clone(&stale_events);
            move |generation, _, _, control, ()| {
                let begin_storage_for_task = Arc::clone(&begin_storage);
                let handle = spawn_required_loop_future(
                    TaskId::MarketFallback,
                    generation,
                    control,
                    Arc::clone(&task_lifecycle),
                    async move {
                        begin_storage_for_task.notified().await;
                        run_generation_owned_kline_storage(move || {
                            let active = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                            max_in_flight.fetch_max(active, Ordering::SeqCst);
                            entered.notify_one();
                            let (released, wake) = release.as_ref();
                            let mut released = released.lock().unwrap();
                            while !*released {
                                released = wake.wait(released).unwrap();
                            }
                            in_flight.fetch_sub(1, Ordering::SeqCst);
                        })
                        .await;
                        stale_events.fetch_add(1, Ordering::SeqCst);
                    },
                );
                (vec![(TaskId::MarketFallback, handle)], async move {
                    begin_storage.notify_one()
                })
            }
        })
        .await
    );
    entered.notified().await;

    let mut stop = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        async move { stop_scheduler_lifecycle_inner(&lifecycle).await }
    });
    while lock_unpoisoned(&lifecycle).state != (SchedulerLifecycleState::Stopping { generation: 1 })
    {
        tokio::task::yield_now().await;
    }
    if let Ok(outcome) = tokio::time::timeout(Duration::from_millis(50), &mut stop).await {
        let (released, wake) = release.as_ref();
        *released.lock().unwrap() = true;
        wake.notify_all();
        panic!(
            "stop completed before synchronous storage returned: {outcome:?}; state={:?}",
            lock_unpoisoned(&lifecycle).state
        );
    }

    let replacement_installs = Arc::new(AtomicUsize::new(0));
    let replacement = tokio::spawn({
        let lifecycle = Arc::clone(&lifecycle);
        let running = Arc::clone(&running);
        let replacement_installs = Arc::clone(&replacement_installs);
        async move {
            start_scheduler_lifecycle(&lifecycle, &running, |_| async {}, {
                move |_, _, _, _, ()| {
                    replacement_installs.fetch_add(1, Ordering::SeqCst);
                    (
                        vec![(TaskId::NotificationMaintenance, pending_required_handle())],
                        async {},
                    )
                }
            })
            .await
        }
    });
    tokio::task::yield_now().await;
    assert_eq!(replacement_installs.load(Ordering::SeqCst), 0);

    let (released, wake) = release.as_ref();
    *released.lock().unwrap() = true;
    wake.notify_all();
    assert!(stop.await.unwrap());
    assert!(replacement.await.unwrap());
    assert_eq!(in_flight.load(Ordering::SeqCst), 0);
    assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);
    assert_eq!(stale_events.load(Ordering::SeqCst), 0);
    assert_eq!(replacement_installs.load(Ordering::SeqCst), 1);
    assert!(stop_scheduler_lifecycle(&lifecycle, &running).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn background_and_shutdown_kline_flushes_share_one_behavioral_gate() {
    let coordinator = Arc::new(KlineFlushCoordinator::new());
    let background_entered = Arc::new(tokio::sync::Notify::new());
    let shutdown_entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new((StdMutex::new(false), Condvar::new()));

    let background = tokio::spawn(Arc::clone(&coordinator).run_blocking(
        KlineFlushOrigin::Background,
        {
            let entered = Arc::clone(&background_entered);
            let release = Arc::clone(&release);
            move || {
                entered.notify_one();
                let (released, wake) = release.as_ref();
                let mut released = released.lock().unwrap();
                while !*released {
                    released = wake.wait(released).unwrap();
                }
            }
        },
    ));
    background_entered.notified().await;
    background.abort();
    assert!(background.await.unwrap_err().is_cancelled());

    let shutdown = tokio::spawn(Arc::clone(&coordinator).run_blocking(
        KlineFlushOrigin::Shutdown,
        {
            let entered = Arc::clone(&shutdown_entered);
            move || entered.notify_one()
        },
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), shutdown_entered.notified())
            .await
            .is_err(),
        "the final shutdown flush must wait for the cancelled background closure"
    );

    let (released, wake) = release.as_ref();
    *released.lock().unwrap() = true;
    wake.notify_all();
    shutdown.await.unwrap().unwrap();
}

#[tokio::test]
async fn cancelled_pre_gate_background_flush_cannot_overtake_shutdown_flush() {
    let coordinator = Arc::new(KlineFlushCoordinator::new());
    let held = Arc::clone(&coordinator.gate).lock_owned().await;
    let old_runs = Arc::new(AtomicUsize::new(0));
    let shutdown_runs = Arc::new(AtomicUsize::new(0));
    let mut background = Box::pin(Arc::clone(&coordinator).run_blocking(
        KlineFlushOrigin::Background,
        {
            let old_runs = Arc::clone(&old_runs);
            move || old_runs.fetch_add(1, Ordering::SeqCst)
        },
    ));

    assert!(matches!(
        futures_util::poll!(background.as_mut()),
        std::task::Poll::Pending
    ));
    drop(background);
    drop(held);

    Arc::clone(&coordinator)
        .run_blocking(KlineFlushOrigin::Shutdown, {
            let shutdown_runs = Arc::clone(&shutdown_runs);
            move || shutdown_runs.fetch_add(1, Ordering::SeqCst)
        })
        .await
        .unwrap();
    assert_eq!(old_runs.load(Ordering::SeqCst), 0);
    assert_eq!(shutdown_runs.load(Ordering::SeqCst), 1);
}
