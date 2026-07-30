use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::watch;

use super::NewsService;
use crate::models::news::NewsStatusKind;

#[must_use = "shutdown timeout must be handled by the caller"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewsShutdownOutcome {
    Stopped,
    TimedOut,
}

pub(super) struct ShutdownState {
    owner: AtomicBool,
    complete: watch::Sender<bool>,
}

impl ShutdownState {
    pub(super) fn new() -> Self {
        let (complete, _) = watch::channel(false);
        Self {
            owner: AtomicBool::new(false),
            complete,
        }
    }

    fn try_claim(&self) -> bool {
        self.owner
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn subscribe(&self) -> watch::Receiver<bool> {
        self.complete.subscribe()
    }

    fn finish(&self) {
        self.complete.send_replace(true);
    }
}

impl NewsService {
    pub(crate) async fn stop_and_join(&self, timeout: Duration) -> NewsShutdownOutcome {
        self.cancellation.send_replace(true);
        if !self.shutdown.try_claim() {
            return wait_for_completion(self.shutdown.subscribe(), timeout).await;
        }

        let handle = self.task.lock().unwrap().take();
        let mut outcome = NewsShutdownOutcome::Stopped;
        if let Some(mut handle) = handle {
            if tokio::time::timeout(timeout, &mut handle).await.is_err() {
                handle.abort();
                let _ = handle.await;
                outcome = NewsShutdownOutcome::TimedOut;
            }
        }
        self.publish_from_current(NewsStatusKind::Stopped, None);
        self.shutdown.finish();
        outcome
    }
}

async fn wait_for_completion(
    mut complete: watch::Receiver<bool>,
    timeout: Duration,
) -> NewsShutdownOutcome {
    if *complete.borrow() {
        return NewsShutdownOutcome::Stopped;
    }
    let wait = async move {
        while !*complete.borrow() {
            if complete.changed().await.is_err() {
                return NewsShutdownOutcome::TimedOut;
            }
        }
        NewsShutdownOutcome::Stopped
    };
    tokio::time::timeout(timeout, wait)
        .await
        .unwrap_or(NewsShutdownOutcome::TimedOut)
}
