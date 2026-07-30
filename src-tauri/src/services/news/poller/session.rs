use std::sync::Arc;

use crate::models::news::NewsStatusKind;
use crate::storage::{NewsApiToken, NewsDatabaseState, NewsStorageErrorKind};

use super::super::blocking;
use super::super::ports::NewsRepository;
use super::super::NewsService;
use super::PauseReason;

pub(super) enum SessionStart {
    Degraded(NewsDatabaseState),
    Ready(NewsApiToken, NewsDatabaseState),
    Paused(NewsStatusKind, NewsDatabaseState, PauseReason),
}

pub(super) async fn start(service: &NewsService) -> SessionStart {
    let repository = match service.open_repository().await {
        Ok(repository) => repository,
        Err(()) => return storage_pause(service),
    };
    let (Some(fingerprint), Some(_client)) =
        (service.source_fingerprint.clone(), service.client.clone())
    else {
        return match blocking::state_snapshot(repository).await {
            Ok(state) => SessionStart::Degraded(state),
            Err(_) => SessionStart::Degraded(current_state(service)),
        };
    };
    if let Err(error) = blocking::prepare_source(repository.clone(), fingerprint).await {
        let state = load_state_or_current(service, repository.clone()).await;
        return match error.kind() {
            NewsStorageErrorKind::SourceMismatch => {
                SessionStart::Paused(NewsStatusKind::ContractError, state, PauseReason::Retry)
            }
            _ => SessionStart::Paused(NewsStatusKind::StorageError, state, PauseReason::Storage),
        };
    }

    let token_store = match service.token_store().await {
        Ok(token_store) => token_store,
        Err(()) => {
            return pause_with_state(
                service,
                repository,
                NewsStatusKind::CredentialStoreUnavailable,
                PauseReason::Credentials,
            )
            .await;
        }
    };
    let token = match blocking::load_token(token_store).await {
        Ok(Some(token)) => token,
        Ok(None) => {
            return pause_with_state(
                service,
                repository,
                NewsStatusKind::NotConfigured,
                PauseReason::Credentials,
            )
            .await;
        }
        Err(_) => {
            return pause_with_state(
                service,
                repository,
                NewsStatusKind::CredentialStoreUnavailable,
                PauseReason::Credentials,
            )
            .await;
        }
    };
    match blocking::state_snapshot(repository).await {
        Ok(state) => SessionStart::Ready(token, state),
        Err(_) => storage_pause(service),
    }
}

async fn pause_with_state(
    service: &NewsService,
    repository: Arc<dyn NewsRepository>,
    kind: NewsStatusKind,
    reason: PauseReason,
) -> SessionStart {
    match blocking::state_snapshot(repository).await {
        Ok(state) => SessionStart::Paused(kind, state, reason),
        Err(_) => storage_pause(service),
    }
}

async fn load_state_or_current(
    service: &NewsService,
    repository: Arc<dyn NewsRepository>,
) -> NewsDatabaseState {
    blocking::state_snapshot(repository)
        .await
        .unwrap_or_else(|_| current_state(service))
}

fn storage_pause(service: &NewsService) -> SessionStart {
    SessionStart::Paused(
        NewsStatusKind::StorageError,
        current_state(service),
        PauseReason::Storage,
    )
}

fn current_state(service: &NewsService) -> NewsDatabaseState {
    let current = service.status();
    let latest = current
        .latest_delivery_id
        .as_deref()
        .and_then(|id| id.parse().ok());
    NewsDatabaseState {
        cursor: latest.unwrap_or(0),
        last_seen_delivery_id: 0,
        initial_sync_complete: current.initial_sync_complete,
        synced_count: current.synced_count,
        latest_delivery_id: latest,
        unread_count: current.unread_count,
    }
}
