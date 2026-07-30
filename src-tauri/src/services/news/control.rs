use std::time::Duration;

use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ManualAction {
    RecheckCredentials,
    RetrySync,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct ManualSignal {
    credentials_generation: u64,
    retry_generation: u64,
}

impl ManualSignal {
    pub(super) fn advance(&mut self, action: ManualAction) {
        let generation = match action {
            ManualAction::RecheckCredentials => &mut self.credentials_generation,
            ManualAction::RetrySync => &mut self.retry_generation,
        };
        *generation = generation.wrapping_add(1);
    }

    pub(super) fn generation(self, action: ManualAction) -> u64 {
        match action {
            ManualAction::RecheckCredentials => self.credentials_generation,
            ManualAction::RetrySync => self.retry_generation,
        }
    }
}

pub(super) enum WaitOutcome {
    Cancelled,
    Manual,
    Elapsed,
}

pub(super) async fn wait(
    cancellation: &mut watch::Receiver<bool>,
    manual: &mut watch::Receiver<ManualSignal>,
    duration: Duration,
) -> WaitOutcome {
    if *cancellation.borrow() {
        return WaitOutcome::Cancelled;
    }
    tokio::select! {
        _ = cancellation.changed() => WaitOutcome::Cancelled,
        result = manual.changed() => {
            if result.is_err() {
                WaitOutcome::Cancelled
            } else {
                WaitOutcome::Manual
            }
        }
        _ = tokio::time::sleep(duration) => WaitOutcome::Elapsed,
    }
}
