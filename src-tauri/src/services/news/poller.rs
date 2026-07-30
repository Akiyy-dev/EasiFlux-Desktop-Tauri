mod cycle;
mod session;

use std::sync::Arc;

use tokio::sync::watch;

use crate::models::news::NewsStatusKind;

use super::control::{ManualAction, ManualSignal};
use super::status;
use super::NewsService;
use session::SessionStart;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PauseReason {
    Credentials,
    Retry,
    Storage,
}

pub(super) async fn run(service: Arc<NewsService>) {
    let mut cancellation = service.cancellation.subscribe();
    let mut manual = service.manual.subscribe();
    loop {
        if *cancellation.borrow() {
            return;
        }
        let session_manual = *manual.borrow_and_update();
        match session::start(&service).await {
            SessionStart::Degraded(state) => {
                service.publish(status::from_state(
                    NewsStatusKind::DeploymentMisconfigured,
                    &state,
                    None,
                ));
                return;
            }
            SessionStart::Ready(token, state) => {
                let kind = if state.initial_sync_complete {
                    NewsStatusKind::Live
                } else {
                    NewsStatusKind::InitialSync
                };
                service.publish(status::from_state(kind, &state, None));
                match cycle::run(&service, token, state, &mut cancellation, &mut manual).await {
                    cycle::CycleExit::Cancelled => return,
                    cycle::CycleExit::Restart => continue,
                    cycle::CycleExit::Paused(kind, state, reason) => {
                        service.publish(status::from_state(kind, &state, None));
                        if !wait_paused(&mut cancellation, &mut manual, reason, session_manual)
                            .await
                        {
                            return;
                        }
                        if reason == PauseReason::Storage {
                            service.invalidate_repository();
                        }
                    }
                }
            }
            SessionStart::Paused(kind, state, reason) => {
                service.publish(status::from_state(kind, &state, None));
                if !wait_paused(&mut cancellation, &mut manual, reason, session_manual).await {
                    return;
                }
                if reason == PauseReason::Storage {
                    service.invalidate_repository();
                }
            }
        }
    }
}

async fn wait_paused(
    cancellation: &mut watch::Receiver<bool>,
    manual: &mut watch::Receiver<ManualSignal>,
    reason: PauseReason,
    baseline: ManualSignal,
) -> bool {
    let target = match reason {
        PauseReason::Credentials => ManualAction::RecheckCredentials,
        PauseReason::Retry | PauseReason::Storage => ManualAction::RetrySync,
    };
    let last_seen = baseline.generation(target);
    loop {
        if *cancellation.borrow() {
            return false;
        }
        if manual.borrow().generation(target) != last_seen {
            return true;
        }
        tokio::select! {
            _ = cancellation.changed() => return false,
            result = manual.changed() => {
                if result.is_err() {
                    return false;
                }
            }
        }
    }
}
