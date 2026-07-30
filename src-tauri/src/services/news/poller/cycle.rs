use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
use tokio::sync::watch;

use crate::api::news_client::NewsFetchErrorKind;
use crate::models::news::{NewsMessagesCommittedEvent, NewsStatusKind};
use crate::storage::{NewsApiToken, NewsDatabaseState};

use super::PauseReason;
use crate::services::news::backoff::RetryBackoff;
use crate::services::news::blocking;
use crate::services::news::control::{self, ManualSignal, WaitOutcome};
use crate::services::news::status;
use crate::services::news::NewsService;

pub(super) enum CycleExit {
    Cancelled,
    Restart,
    Paused(NewsStatusKind, NewsDatabaseState, PauseReason),
}

pub(super) async fn run(
    service: &NewsService,
    token: NewsApiToken,
    mut state: NewsDatabaseState,
    cancellation: &mut watch::Receiver<bool>,
    manual: &mut watch::Receiver<ManualSignal>,
) -> CycleExit {
    let repository = service
        .repository()
        .expect("poll cycle starts only after repository initialization");
    let client = service.client.as_ref().unwrap().clone();
    let mut backoff = RetryBackoff::default();
    loop {
        if *cancellation.borrow() {
            return CycleExit::Cancelled;
        }
        if manual.has_changed().unwrap_or(false) {
            manual.borrow_and_update();
            return CycleExit::Restart;
        }
        let response = {
            let fetch = client.fetch_after(&token, state.cursor, 100);
            tokio::pin!(fetch);
            tokio::select! {
                _ = cancellation.changed() => return CycleExit::Cancelled,
                result = manual.changed() => {
                    return if result.is_err() {
                        CycleExit::Cancelled
                    } else {
                        CycleExit::Restart
                    };
                }
                response = &mut fetch => response,
            }
        };

        match response {
            Ok(page) => {
                backoff.reset();
                let has_more = page.has_more;
                let received_at_ms = Utc::now().timestamp_millis();
                let outcome =
                    match blocking::commit_page(repository.clone(), page, received_at_ms).await {
                        Ok(outcome) => outcome,
                        Err(_) => {
                            return CycleExit::Paused(
                                NewsStatusKind::StorageError,
                                state,
                                PauseReason::Storage,
                            );
                        }
                    };
                let event = NewsMessagesCommittedEvent {
                    inserted_count: outcome.inserted_count,
                    newest_delivery_id: outcome.newest_delivery_id.map(|id| id.to_string()),
                    unread_count: outcome.unread_count.min(100),
                    initial_sync_complete: outcome.initial_sync_complete,
                };
                let _ = service.event_sink.emit_messages_committed(&event);
                state = match blocking::state_snapshot(repository.clone()).await {
                    Ok(state) => state,
                    Err(_) => {
                        return CycleExit::Paused(
                            NewsStatusKind::StorageError,
                            state,
                            PauseReason::Storage,
                        );
                    }
                };
                let kind = if state.initial_sync_complete {
                    NewsStatusKind::Live
                } else {
                    NewsStatusKind::InitialSync
                };
                service.publish(status::from_state(kind, &state, None));
                if has_more {
                    tokio::task::yield_now().await;
                    if *cancellation.borrow() {
                        return CycleExit::Cancelled;
                    }
                    if manual.has_changed().unwrap_or(false) {
                        manual.borrow_and_update();
                        return CycleExit::Restart;
                    }
                    continue;
                }
                match control::wait(cancellation, manual, std::time::Duration::from_secs(3)).await {
                    WaitOutcome::Cancelled => return CycleExit::Cancelled,
                    WaitOutcome::Manual => return CycleExit::Restart,
                    WaitOutcome::Elapsed => {}
                }
            }
            Err(error) => match error.kind() {
                NewsFetchErrorKind::Transient => {
                    let delay = backoff.next_delay(error.retry_after());
                    let retry_at = Utc::now()
                        .checked_add_signed(
                            ChronoDuration::from_std(delay).unwrap_or(ChronoDuration::seconds(60)),
                        )
                        .map(|time| time.to_rfc3339_opts(SecondsFormat::Millis, true));
                    service.publish(status::from_state(
                        NewsStatusKind::Retrying,
                        &state,
                        retry_at,
                    ));
                    match control::wait(cancellation, manual, delay).await {
                        WaitOutcome::Cancelled => return CycleExit::Cancelled,
                        WaitOutcome::Manual => return CycleExit::Restart,
                        WaitOutcome::Elapsed => {}
                    }
                }
                NewsFetchErrorKind::CredentialInvalid => {
                    return CycleExit::Paused(
                        NewsStatusKind::CredentialInvalid,
                        state,
                        PauseReason::Credentials,
                    );
                }
                NewsFetchErrorKind::Contract => {
                    return CycleExit::Paused(
                        NewsStatusKind::ContractError,
                        state,
                        PauseReason::Retry,
                    );
                }
            },
        }
    }
}
