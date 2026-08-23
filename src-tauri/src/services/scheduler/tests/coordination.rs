use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::super::{execute_task_with_account_lifecycle, run_scheduled_task, TaskId, TaskRunState};
use crate::error::AppError;
use crate::services::account_profiles::run_serialized_account_mutation;
use crate::services::AccountLifecycleCoordinator;

fn record(events: &Arc<Mutex<Vec<String>>>, event: impl Into<String>) {
    events.lock().unwrap().push(event.into());
}

#[tokio::test]
async fn direct_private_task_emits_before_switch_return_and_frontend_clear() {
    let coordinator = AccountLifecycleCoordinator::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let release_network = Arc::new(tokio::sync::Notify::new());
    let task_events = events.clone();
    let task_release = release_network.clone();
    let task = execute_task_with_account_lifecycle(&coordinator, TaskId::Balances, move || {
        let events = task_events.clone();
        let release = task_release.clone();
        async move {
            record(&events, "request:primary");
            release.notified().await;
            record(&events, "emit:primary");
            Ok(())
        }
    });
    tokio::pin!(task);
    assert!(matches!(
        futures_util::poll!(&mut task),
        std::task::Poll::Pending
    ));

    let switch_events = events.clone();
    let switch = run_serialized_account_mutation(&coordinator, move || async move {
        record(&switch_events, "switch:return");
    });
    tokio::pin!(switch);
    assert!(matches!(
        futures_util::poll!(&mut switch),
        std::task::Poll::Pending
    ));

    release_network.notify_one();
    tokio::time::timeout(Duration::from_secs(1), async {
        let (task_result, ()) = tokio::join!(task.as_mut(), switch.as_mut());
        task_result.unwrap();
    })
    .await
    .expect("task and switch should finish after the network response");
    record(&events, "frontend:clear");

    assert_eq!(
        *events.lock().unwrap(),
        [
            "request:primary",
            "emit:primary",
            "switch:return",
            "frontend:clear"
        ]
    );
}

#[tokio::test]
async fn periodic_private_task_waits_for_switch_then_reads_new_context() {
    let coordinator = AccountLifecycleCoordinator::new();
    let account_id = Arc::new(Mutex::new("primary".to_string()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let held_switch = coordinator.mutation_guard().await;
    let task_account = account_id.clone();
    let task_events = events.clone();
    let task =
        execute_task_with_account_lifecycle(&coordinator, TaskId::PrivatePanels, move || {
            let account_id = task_account.clone();
            let events = task_events.clone();
            async move {
                let selected = account_id.lock().unwrap().clone();
                record(&events, format!("emit:{selected}"));
                Ok(())
            }
        });
    tokio::pin!(task);
    assert!(matches!(
        futures_util::poll!(&mut task),
        std::task::Poll::Pending
    ));

    *account_id.lock().unwrap() = "backup".into();
    record(&events, "switch:backup");
    drop(held_switch);
    tokio::time::timeout(Duration::from_secs(1), task.as_mut())
        .await
        .expect("periodic task should run after the switch releases the coordinator")
        .unwrap();

    assert_eq!(*events.lock().unwrap(), ["switch:backup", "emit:backup"]);
}

#[tokio::test]
async fn pending_force_rerun_crosses_switch_only_with_new_account_context() {
    let coordinator = AccountLifecycleCoordinator::new();
    let account_id = Arc::new(Mutex::new("primary".to_string()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let release_network = Arc::new(tokio::sync::Notify::new());
    let run_count = Arc::new(AtomicUsize::new(0));
    let run_state = Arc::new(Mutex::new(TaskRunState::default()));

    let runs = run_scheduled_task(Arc::clone(&run_state), false, || {
        let run = run_count.fetch_add(1, Ordering::SeqCst);
        let account_id = account_id.clone();
        let events = events.clone();
        let release_network = release_network.clone();
        let coordinator = &coordinator;
        async move {
            execute_task_with_account_lifecycle(coordinator, TaskId::DailyPnl, move || {
                let account_id = account_id.clone();
                let events = events.clone();
                let release_network = release_network.clone();
                async move {
                    let selected = account_id.lock().unwrap().clone();
                    if run == 0 {
                        record(&events, format!("request:{selected}"));
                        release_network.notified().await;
                    }
                    record(&events, format!("emit:{selected}"));
                    Ok(())
                }
            })
            .await
        }
    });
    tokio::pin!(runs);
    assert!(matches!(
        futures_util::poll!(&mut runs),
        std::task::Poll::Pending
    ));
    {
        let state = run_state.lock().unwrap();
        assert!(state.in_flight);
        assert!(!state.pending_force);
    }

    let unexpected_force_runs = Arc::new(AtomicUsize::new(0));
    let force_runs = unexpected_force_runs.clone();
    let forced = run_scheduled_task(Arc::clone(&run_state), true, move || {
        force_runs.fetch_add(1, Ordering::SeqCst);
        async { Ok(()) }
    });
    tokio::pin!(forced);
    assert!(matches!(
        futures_util::poll!(&mut forced),
        std::task::Poll::Pending
    ));
    assert_eq!(unexpected_force_runs.load(Ordering::SeqCst), 0);

    let switch_account = account_id.clone();
    let switch_events = events.clone();
    let switch = run_serialized_account_mutation(&coordinator, move || async move {
        *switch_account.lock().unwrap() = "backup".into();
        record(&switch_events, "switch:backup");
    });
    tokio::pin!(switch);
    assert!(matches!(
        futures_util::poll!(&mut switch),
        std::task::Poll::Pending
    ));

    release_network.notify_one();
    tokio::time::timeout(Duration::from_secs(1), async {
        let (runs_result, forced_result, ()) =
            tokio::join!(runs.as_mut(), forced.as_mut(), switch.as_mut());
        runs_result.unwrap();
        forced_result.unwrap();
    })
    .await
    .expect("pending rerun and switch should finish without deadlock");

    assert_eq!(run_count.load(Ordering::SeqCst), 2);
    {
        let state = run_state.lock().unwrap();
        assert!(!state.in_flight);
        assert!(!state.pending_force);
    }
    assert_eq!(
        *events.lock().unwrap(),
        [
            "request:primary",
            "emit:primary",
            "switch:backup",
            "emit:backup"
        ]
    );
}

#[tokio::test]
async fn force_at_completion_handoff_is_drained_without_lost_wakeup() {
    let run_state = Arc::new(Mutex::new(TaskRunState::default()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let release_first = Arc::new(tokio::sync::Notify::new());
    let owner_runs = Arc::new(AtomicUsize::new(0));
    let forced_runs = Arc::new(AtomicUsize::new(0));

    let first_events = events.clone();
    let first_release = release_first.clone();
    let first_runs = owner_runs.clone();
    let first = run_scheduled_task(Arc::clone(&run_state), false, move || {
        let run = first_runs.fetch_add(1, Ordering::SeqCst);
        let events = first_events.clone();
        let release = first_release.clone();
        async move {
            record(&events, format!("execute:owner:{run}"));
            if run == 0 {
                release.notified().await;
            }
            Ok(())
        }
    });
    tokio::pin!(first);
    assert!(matches!(
        futures_util::poll!(&mut first),
        std::task::Poll::Pending
    ));

    let competing_runs = forced_runs.clone();
    let forced = run_scheduled_task(Arc::clone(&run_state), true, move || {
        competing_runs.fetch_add(1, Ordering::SeqCst);
        async { Ok(()) }
    });
    tokio::pin!(forced);
    assert!(matches!(
        futures_util::poll!(&mut forced),
        std::task::Poll::Pending
    ));

    release_first.notify_one();
    tokio::time::timeout(Duration::from_secs(1), async {
        let (first_result, forced_result) = tokio::join!(first.as_mut(), forced.as_mut());
        first_result.unwrap();
        forced_result.unwrap();
    })
    .await
    .expect("the forced request at handoff must execute without another scheduler tick");

    assert_eq!(owner_runs.load(Ordering::SeqCst), 2);
    assert_eq!(forced_runs.load(Ordering::SeqCst), 0);
    assert_eq!(
        *events.lock().unwrap(),
        ["execute:owner:0", "execute:owner:1"]
    );
    let state = run_state.lock().unwrap();
    assert!(!state.in_flight);
    assert!(!state.pending_force);
}

#[tokio::test]
async fn periodic_and_forced_direct_claim_have_one_owner_and_one_rerun() {
    let run_state = Arc::new(Mutex::new(TaskRunState::default()));
    let release_owner = Arc::new(tokio::sync::Notify::new());
    let periodic_runs = Arc::new(AtomicUsize::new(0));
    let direct_runs = Arc::new(AtomicUsize::new(0));

    let owner_release = release_owner.clone();
    let owner_runs = periodic_runs.clone();
    let periodic = run_scheduled_task(Arc::clone(&run_state), false, move || {
        let run = owner_runs.fetch_add(1, Ordering::SeqCst);
        let release = owner_release.clone();
        async move {
            if run == 0 {
                release.notified().await;
            }
            Ok(())
        }
    });
    tokio::pin!(periodic);
    assert!(matches!(
        futures_util::poll!(&mut periodic),
        std::task::Poll::Pending
    ));

    let competing_runs = direct_runs.clone();
    let direct = run_scheduled_task(Arc::clone(&run_state), true, move || {
        competing_runs.fetch_add(1, Ordering::SeqCst);
        async { Ok(()) }
    });
    tokio::pin!(direct);
    assert!(matches!(
        futures_util::poll!(&mut direct),
        std::task::Poll::Pending
    ));

    release_owner.notify_one();
    tokio::time::timeout(Duration::from_secs(1), periodic.as_mut())
        .await
        .expect("the sole owner should drain exactly one forced rerun")
        .unwrap();

    assert_eq!(periodic_runs.load(Ordering::SeqCst), 2);
    assert_eq!(direct_runs.load(Ordering::SeqCst), 0);
    let state = run_state.lock().unwrap();
    assert!(!state.in_flight);
    assert!(!state.pending_force);
}

#[tokio::test]
async fn multiple_force_waiters_share_one_rerun_and_its_failure() {
    let run_state = Arc::new(Mutex::new(TaskRunState::default()));
    let release_initial = Arc::new(tokio::sync::Notify::new());
    let release_rerun = Arc::new(tokio::sync::Notify::new());
    let owner_runs = Arc::new(AtomicUsize::new(0));
    let unexpected_force_runs = Arc::new(AtomicUsize::new(0));

    let owner_release_initial = release_initial.clone();
    let owner_release_rerun = release_rerun.clone();
    let owner_count = owner_runs.clone();
    let owner = run_scheduled_task(Arc::clone(&run_state), false, move || {
        let run = owner_count.fetch_add(1, Ordering::SeqCst);
        let release_initial = owner_release_initial.clone();
        let release_rerun = owner_release_rerun.clone();
        async move {
            if run == 0 {
                release_initial.notified().await;
                Ok(())
            } else {
                release_rerun.notified().await;
                Err(AppError::Internal("rerun failed with raw-secret".into()))
            }
        }
    });
    tokio::pin!(owner);
    assert!(matches!(
        futures_util::poll!(&mut owner),
        std::task::Poll::Pending
    ));

    let first_count = unexpected_force_runs.clone();
    let first_force = run_scheduled_task(Arc::clone(&run_state), true, move || {
        first_count.fetch_add(1, Ordering::SeqCst);
        async { Ok(()) }
    });
    tokio::pin!(first_force);
    assert!(matches!(
        futures_util::poll!(&mut first_force),
        std::task::Poll::Pending
    ));

    release_initial.notify_one();
    assert!(matches!(
        futures_util::poll!(&mut owner),
        std::task::Poll::Pending
    ));
    assert_eq!(owner_runs.load(Ordering::SeqCst), 2);
    assert!(matches!(
        futures_util::poll!(&mut first_force),
        std::task::Poll::Pending
    ));

    let second_count = unexpected_force_runs.clone();
    let second_force = run_scheduled_task(Arc::clone(&run_state), true, move || {
        second_count.fetch_add(1, Ordering::SeqCst);
        async { Ok(()) }
    });
    tokio::pin!(second_force);
    assert!(matches!(
        futures_util::poll!(&mut second_force),
        std::task::Poll::Pending
    ));

    release_rerun.notify_one();
    let (owner_result, first_result, second_result) =
        tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(owner.as_mut(), first_force.as_mut(), second_force.as_mut())
        })
        .await
        .expect("all force waiters should finish with the coalesced rerun");

    for result in [owner_result, first_result, second_result] {
        assert_eq!(
            result.unwrap_err().to_string(),
            "内部错误: rerun failed with raw-secret"
        );
    }
    assert_eq!(owner_runs.load(Ordering::SeqCst), 2);
    assert_eq!(unexpected_force_runs.load(Ordering::SeqCst), 0);
    let state = run_state.lock().unwrap();
    assert!(!state.in_flight);
    assert!(!state.pending_force);
}

#[tokio::test]
async fn non_force_request_still_skips_a_busy_task() {
    let run_state = Arc::new(Mutex::new(TaskRunState::default()));
    let release_owner = Arc::new(tokio::sync::Notify::new());
    let owner_release = release_owner.clone();
    let owner = run_scheduled_task(Arc::clone(&run_state), false, move || {
        let release = owner_release.clone();
        async move {
            release.notified().await;
            Ok(())
        }
    });
    tokio::pin!(owner);
    assert!(matches!(
        futures_util::poll!(&mut owner),
        std::task::Poll::Pending
    ));

    let skipped_runs = Arc::new(AtomicUsize::new(0));
    let skipped_count = skipped_runs.clone();
    let skipped = run_scheduled_task(Arc::clone(&run_state), false, move || {
        skipped_count.fetch_add(1, Ordering::SeqCst);
        async { Ok(()) }
    });
    tokio::pin!(skipped);
    assert!(matches!(
        futures_util::poll!(&mut skipped),
        std::task::Poll::Ready(Ok(()))
    ));
    assert_eq!(skipped_runs.load(Ordering::SeqCst), 0);

    release_owner.notify_one();
    tokio::time::timeout(Duration::from_secs(1), owner.as_mut())
        .await
        .expect("the owner should finish after release")
        .unwrap();
}

#[tokio::test]
async fn aborting_an_in_flight_owner_releases_waiters_and_allows_the_next_run() {
    let run_state = Arc::new(Mutex::new(TaskRunState::default()));
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let runs = Arc::new(AtomicUsize::new(0));

    let owner = tokio::spawn({
        let run_state = Arc::clone(&run_state);
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        let runs = Arc::clone(&runs);
        async move {
            run_scheduled_task(Arc::clone(&run_state), false, move || {
                let entered = Arc::clone(&entered);
                let release = Arc::clone(&release);
                let runs = Arc::clone(&runs);
                async move {
                    runs.fetch_add(1, Ordering::SeqCst);
                    entered.notify_one();
                    release.notified().await;
                    Ok(())
                }
            })
            .await
        }
    });
    entered.notified().await;

    let forced = tokio::spawn({
        let run_state = Arc::clone(&run_state);
        async move { run_scheduled_task(run_state, true, || async { Ok(()) }).await }
    });
    loop {
        if run_state.lock().unwrap().pending_force {
            break;
        }
        tokio::task::yield_now().await;
    }

    owner.abort();
    assert!(owner.await.unwrap_err().is_cancelled());
    let forced_result = tokio::time::timeout(Duration::from_millis(100), forced).await;

    let next_runs = Arc::clone(&runs);
    run_scheduled_task(Arc::clone(&run_state), false, move || {
        let runs = Arc::clone(&next_runs);
        async move {
            runs.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    })
    .await
    .unwrap();

    assert!(forced_result.is_ok(), "a pre-stop waiter must not hang");
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[test]
fn late_old_owner_drop_cannot_clear_a_newer_owner_token() {
    let run_state = Arc::new(Mutex::new(TaskRunState::default()));
    {
        let mut state = run_state.lock().unwrap();
        state.in_flight = true;
        state.owner_token = Some(2);
        state.next_owner_token = 2;
    }
    let stale_owner = super::super::TaskRunOwnerGuard::new(Arc::clone(&run_state), 1);

    drop(stale_owner);

    let state = run_state.lock().unwrap();
    assert!(state.in_flight);
    assert_eq!(state.owner_token, Some(2));
}

#[tokio::test]
async fn environment_probe_finishes_before_account_switch_returns() {
    let coordinator = AccountLifecycleCoordinator::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let release_probe = Arc::new(tokio::sync::Notify::new());
    let probe_events = events.clone();
    let probe_release = release_probe.clone();
    let probe = execute_task_with_account_lifecycle(&coordinator, TaskId::Environment, move || {
        let events = probe_events.clone();
        let release = probe_release.clone();
        async move {
            record(&events, "probe:primary");
            release.notified().await;
            record(&events, "emit:primary");
            Ok(())
        }
    });
    tokio::pin!(probe);
    assert!(matches!(
        futures_util::poll!(&mut probe),
        std::task::Poll::Pending
    ));

    let switch_events = events.clone();
    let switch = run_serialized_account_mutation(&coordinator, move || async move {
        record(&switch_events, "switch:return");
    });
    tokio::pin!(switch);
    assert!(matches!(
        futures_util::poll!(&mut switch),
        std::task::Poll::Pending
    ));

    release_probe.notify_one();
    tokio::time::timeout(Duration::from_secs(1), async {
        let (probe_result, ()) = tokio::join!(probe.as_mut(), switch.as_mut());
        probe_result.unwrap();
    })
    .await
    .expect("environment probe and switch should serialize without deadlock");

    assert_eq!(
        *events.lock().unwrap(),
        ["probe:primary", "emit:primary", "switch:return"]
    );
}

#[tokio::test]
async fn public_scheduler_task_also_waits_for_account_lifecycle_guard() {
    let coordinator = AccountLifecycleCoordinator::new();
    let held_guard = coordinator.mutation_guard().await;
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let task_ran = ran.clone();
    let task = execute_task_with_account_lifecycle(
        &coordinator,
        TaskId::MarketFallback,
        move || async move {
            task_ran.store(true, Ordering::SeqCst);
            Ok(())
        },
    );
    tokio::pin!(task);

    assert!(matches!(
        futures_util::poll!(&mut task),
        std::task::Poll::Pending
    ));
    assert!(!ran.load(Ordering::SeqCst));

    drop(held_guard);
    tokio::time::timeout(Duration::from_secs(1), task.as_mut())
        .await
        .expect("public scheduler task should resume after lifecycle release")
        .unwrap();
    assert!(ran.load(Ordering::SeqCst));
}

#[tokio::test]
async fn independent_scheduler_tasks_share_lifecycle_read_access() {
    let coordinator = AccountLifecycleCoordinator::new();
    let release_first = Arc::new(tokio::sync::Notify::new());
    let release = Arc::clone(&release_first);
    let first = execute_task_with_account_lifecycle(
        &coordinator,
        TaskId::PrivatePanels,
        move || async move {
            release.notified().await;
            Ok(())
        },
    );
    tokio::pin!(first);
    assert!(matches!(
        futures_util::poll!(&mut first),
        std::task::Poll::Pending
    ));

    let second_ran = AtomicBool::new(false);
    let second =
        execute_task_with_account_lifecycle(&coordinator, TaskId::MarketFallback, || async {
            second_ran.store(true, Ordering::SeqCst);
            Ok(())
        });
    tokio::pin!(second);

    assert!(matches!(
        futures_util::poll!(&mut second),
        std::task::Poll::Ready(Ok(()))
    ));
    assert!(second_ran.load(Ordering::SeqCst));

    release_first.notify_one();
    tokio::time::timeout(Duration::from_secs(1), first.as_mut())
        .await
        .expect("the first scheduler task should finish after release")
        .unwrap();
}
