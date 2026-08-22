use super::super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

#[test]
fn market_fallback_interval_comes_from_config_without_changing_other_tasks() {
    let mut config = AppConfig::default();
    config.ticker_poll_interval = 17.5;

    assert_eq!(
        configured_task_interval(TaskId::MarketFallback, &config),
        Some(Duration::from_secs_f64(17.5)),
    );
    assert_eq!(
        configured_task_interval(TaskId::FundingRate, &config),
        Some(Duration::from_secs(60)),
    );
}

#[tokio::test(start_paused = true)]
async fn interval_change_rearms_from_change_time_without_duplicate_tick() {
    let (tx, rx) = tokio::sync::watch::channel(Duration::from_secs(1));
    let running = Arc::new(AtomicBool::new(true));
    let run_state = Arc::new(Mutex::new(TaskRunState::default()));
    let runs = Arc::new(AtomicUsize::new(0));
    let runs_for_loop = Arc::clone(&runs);

    let handle = tokio::spawn(run_reschedulable_periodic(
        Arc::clone(&running),
        run_state,
        rx,
        Duration::ZERO,
        move || {
            let runs = Arc::clone(&runs_for_loop);
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        },
    ));

    tokio::task::yield_now().await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    tx.send_replace(Duration::from_secs(10));
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(9)).await;
    tokio::task::yield_now().await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(runs.load(Ordering::SeqCst), 2);

    running.store(false, Ordering::SeqCst);
    tx.send_replace(Duration::from_secs(1));
    handle.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn reschedule_during_in_flight_run_keeps_one_owner_and_uses_latest_period() {
    let (tx, rx) = tokio::sync::watch::channel(Duration::from_secs(1));
    let running = Arc::new(AtomicBool::new(true));
    let run_state = Arc::new(Mutex::new(TaskRunState::default()));
    let started = Arc::new(AtomicUsize::new(0));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let first_release = Arc::new(Mutex::new(Some(release_rx)));

    let handle = tokio::spawn(run_reschedulable_periodic(
        Arc::clone(&running),
        run_state,
        rx,
        Duration::ZERO,
        {
            let started = Arc::clone(&started);
            let in_flight = Arc::clone(&in_flight);
            let max_in_flight = Arc::clone(&max_in_flight);
            let first_release = Arc::clone(&first_release);
            move || {
                let started = Arc::clone(&started);
                let in_flight = Arc::clone(&in_flight);
                let max_in_flight = Arc::clone(&max_in_flight);
                let first_release = Arc::clone(&first_release);
                async move {
                    let current = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    max_in_flight.fetch_max(current, Ordering::SeqCst);
                    let run_number = started.fetch_add(1, Ordering::SeqCst) + 1;
                    if run_number == 1 {
                        let receiver = first_release.lock().await.take();
                        if let Some(receiver) = receiver {
                            let _ = receiver.await;
                        }
                    }
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                }
            }
        },
    ));

    tokio::task::yield_now().await;
    assert_eq!(started.load(Ordering::SeqCst), 1);

    tx.send_replace(Duration::from_secs(5));
    tokio::time::advance(Duration::from_secs(20)).await;
    tokio::task::yield_now().await;
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);

    release_tx.send(()).unwrap();
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(4)).await;
    tokio::task::yield_now().await;
    assert_eq!(started.load(Ordering::SeqCst), 1);

    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(started.load(Ordering::SeqCst), 2);
    assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);

    running.store(false, Ordering::SeqCst);
    tx.send_replace(Duration::from_secs(1));
    handle.await.unwrap();
}
