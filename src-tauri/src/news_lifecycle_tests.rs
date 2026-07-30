use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{stop_news_with, NEWS_SHUTDOWN_TIMEOUT};
use crate::services::news::NewsShutdownOutcome;

#[tokio::test]
async fn exit_shutdown_uses_exactly_20_seconds_and_handles_timeout_explicitly() {
    let observed = Arc::new(Mutex::new(None));
    let timed_out = Arc::new(AtomicBool::new(false));
    stop_news_with(
        {
            let observed = observed.clone();
            move |timeout| {
                *observed.lock().unwrap() = Some(timeout);
                async { NewsShutdownOutcome::TimedOut }
            }
        },
        {
            let timed_out = timed_out.clone();
            move || timed_out.store(true, Ordering::SeqCst)
        },
    )
    .await;

    assert_eq!(NEWS_SHUTDOWN_TIMEOUT, Duration::from_secs(20));
    assert_eq!(*observed.lock().unwrap(), Some(Duration::from_secs(20)));
    assert!(timed_out.load(Ordering::SeqCst));
}

#[tokio::test]
async fn stopped_outcome_does_not_report_a_timeout() {
    let timed_out = AtomicBool::new(false);
    stop_news_with(
        |_| async { NewsShutdownOutcome::Stopped },
        || timed_out.store(true, Ordering::SeqCst),
    )
    .await;
    assert!(!timed_out.load(Ordering::SeqCst));
}

#[test]
fn tauri_wiring_registers_all_five_commands_and_starts_news_independently() {
    let source = include_str!("lib.rs");
    for command in [
        "get_news_status,",
        "list_news_messages,",
        "mark_news_seen,",
        "recheck_news_credentials,",
        "retry_news_sync,",
    ] {
        assert!(
            source.contains(command),
            "missing command registration: {command}"
        );
    }
    assert!(source.contains("let news = state.news.clone();"));
    assert!(source.contains("news.start();"));
    assert!(source.contains("stop_news_with("));
}
