use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::api::news_client::NewsFetchErrorKind;
use crate::models::news::NewsStatusKind;
use crate::services::news::{NewsService, NewsShutdownOutcome};
use crate::storage::{NewsDatabaseState, NewsStorageErrorKind};

use super::support::{empty_page, page, BlockingGate, FetchAction, Rig, TokenOutcome};

#[tokio::test(start_paused = true)]
async fn missing_and_failing_credentials_pause_without_any_request() {
    let runtime_thread = std::thread::current().id();
    for (token, expected) in [
        (TokenOutcome::Missing, NewsStatusKind::NotConfigured),
        (
            TokenOutcome::Error,
            NewsStatusKind::CredentialStoreUnavailable,
        ),
    ] {
        let rig = Rig::new(token);
        rig.service.start();
        rig.wait_for_status(expected).await;
        assert!(rig.fetcher.calls().is_empty());
        assert_eq!(rig.tokens.load_threads().len(), 1);
        assert_ne!(rig.tokens.load_threads()[0], runtime_thread);
        assert!(rig
            .repository
            .calls()
            .iter()
            .all(|call| call.1 != runtime_thread));
        assert_stopped(&rig.service).await;
    }
}

#[tokio::test(start_paused = true)]
async fn credential_recheck_observes_mutable_store_and_retained_progress_resumes_at_cursor() {
    let rig = Rig::new(TokenOutcome::Missing);
    rig.repository.seed_state(NewsDatabaseState {
        cursor: 12,
        last_seen_delivery_id: 0,
        initial_sync_complete: false,
        synced_count: 2,
        latest_delivery_id: Some(12),
        unread_count: 0,
    });
    rig.service.start();
    rig.wait_for_status(NewsStatusKind::NotConfigured).await;
    assert_eq!(rig.service.status().synced_count, 2);

    rig.tokens
        .set(TokenOutcome::Present(b"replacement-token".to_vec()));
    rig.fetcher
        .push(FetchAction::Pending(Arc::new(AtomicBool::new(false))));
    let before = rig.service.recheck_credentials();
    assert_eq!(before.kind, NewsStatusKind::NotConfigured);
    rig.service.retry_sync();
    rig.wait_for_calls(1).await;

    assert_eq!(rig.fetcher.calls(), [(12, 100)]);
    assert_eq!(rig.tokens.load_threads().len(), 2);
    assert_stopped(&rig.service).await;
}

#[tokio::test(start_paused = true)]
async fn source_mismatch_and_database_failures_pause_before_network_and_recover_on_retry() {
    let mismatch = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    mismatch
        .repository
        .set_prepare_error(Some(NewsStorageErrorKind::SourceMismatch));
    mismatch.service.start();
    mismatch
        .wait_for_status(NewsStatusKind::ContractError)
        .await;
    assert!(mismatch.fetcher.calls().is_empty());
    assert!(mismatch.tokens.load_threads().is_empty());
    mismatch.repository.set_prepare_error(None);
    mismatch
        .fetcher
        .push(FetchAction::Pending(Arc::new(AtomicBool::new(false))));
    mismatch.service.retry_sync();
    mismatch.wait_for_calls(1).await;
    assert_stopped(&mismatch.service).await;

    let storage = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    storage.repository.fail_next_state();
    storage.service.start();
    storage.wait_for_status(NewsStatusKind::StorageError).await;
    assert!(storage.fetcher.calls().is_empty());
    storage
        .fetcher
        .push(FetchAction::Pending(Arc::new(AtomicBool::new(false))));
    storage.service.retry_sync();
    storage.wait_for_calls(1).await;
    assert_stopped(&storage.service).await;
}

#[tokio::test(start_paused = true)]
async fn initial_sync_fetches_all_pages_immediately_and_emits_actual_commit_outcomes() {
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    rig.fetcher.push(FetchAction::Page(page(&[1, 2], true)));
    rig.fetcher.push(FetchAction::Page(page(&[3], false)));

    rig.service.start();
    rig.wait_for_status(NewsStatusKind::Live).await;

    assert_eq!(rig.fetcher.calls(), [(0, 100), (2, 100)]);
    let state = rig.repository.snapshot();
    assert_eq!(state.synced_count, 3);
    assert!(state.initial_sync_complete);
    assert_eq!(state.unread_count, 0);
    let events = rig.events.committed.lock().unwrap().clone();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].inserted_count, 2);
    assert_eq!(events[0].newest_delivery_id.as_deref(), Some("2"));
    assert!(!events[0].initial_sync_complete);
    assert_eq!(events[1].inserted_count, 1);
    assert!(events[1].initial_sync_complete);
    assert_stopped(&rig.service).await;
}

#[tokio::test(start_paused = true)]
async fn live_polling_waits_exactly_three_seconds() {
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    rig.fetcher.push(FetchAction::Page(empty_page(0)));
    rig.fetcher
        .push(FetchAction::Pending(Arc::new(AtomicBool::new(false))));
    rig.service.start();
    rig.wait_for_status(NewsStatusKind::Live).await;

    tokio::time::advance(Duration::from_millis(2_999)).await;
    tokio::task::yield_now().await;
    assert_eq!(rig.fetcher.calls().len(), 1);
    tokio::time::advance(Duration::from_millis(1)).await;
    rig.wait_for_calls(2).await;
    assert_stopped(&rig.service).await;
}

#[tokio::test(start_paused = true)]
async fn transient_backoff_is_deterministic_and_success_resets_it() {
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    rig.fetcher
        .push(FetchAction::Error(NewsFetchErrorKind::Transient, None));
    rig.fetcher
        .push(FetchAction::Error(NewsFetchErrorKind::Transient, None));
    rig.fetcher.push(FetchAction::Page(empty_page(0)));
    rig.fetcher
        .push(FetchAction::Error(NewsFetchErrorKind::Transient, None));
    rig.service.start();
    rig.wait_for_status(NewsStatusKind::Retrying).await;

    tokio::time::advance(Duration::from_secs(3)).await;
    rig.wait_for_calls(2).await;
    tokio::time::advance(Duration::from_secs(6)).await;
    rig.wait_for_calls(3).await;
    rig.wait_for_status(NewsStatusKind::Live).await;
    tokio::time::advance(Duration::from_secs(3)).await;
    rig.wait_for_calls(4).await;
    rig.wait_for_status(NewsStatusKind::Retrying).await;
    assert!(rig.service.status().retry_at.is_some());
    assert_eq!(rig.tokens.load_threads().len(), 1);
    assert_stopped(&rig.service).await;
}

#[tokio::test(start_paused = true)]
async fn retry_after_uses_the_larger_delay_and_caps_at_sixty_seconds() {
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    rig.fetcher.push(FetchAction::Error(
        NewsFetchErrorKind::Transient,
        Some(Duration::from_secs(40)),
    ));
    rig.fetcher.push(FetchAction::Error(
        NewsFetchErrorKind::Transient,
        Some(Duration::from_secs(90)),
    ));
    rig.service.start();
    rig.wait_for_calls(1).await;
    tokio::time::advance(Duration::from_secs(39)).await;
    tokio::task::yield_now().await;
    assert_eq!(rig.fetcher.calls().len(), 1);
    tokio::time::advance(Duration::from_secs(1)).await;
    rig.wait_for_calls(2).await;
    tokio::time::advance(Duration::from_secs(59)).await;
    tokio::task::yield_now().await;
    assert_eq!(rig.fetcher.calls().len(), 2);
    tokio::time::advance(Duration::from_secs(1)).await;
    rig.wait_for_calls(3).await;
    assert_stopped(&rig.service).await;
}

#[tokio::test(start_paused = true)]
async fn credential_and_contract_pauses_require_the_correct_manual_signal_and_reload_token() {
    let credential = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    credential.fetcher.push(FetchAction::Error(
        NewsFetchErrorKind::CredentialInvalid,
        None,
    ));
    credential
        .fetcher
        .push(FetchAction::Pending(Arc::new(AtomicBool::new(false))));
    credential.service.start();
    credential
        .wait_for_status(NewsStatusKind::CredentialInvalid)
        .await;
    credential.service.retry_sync();
    tokio::task::yield_now().await;
    assert_eq!(credential.fetcher.calls().len(), 1);
    credential.service.recheck_credentials();
    credential.service.retry_sync();
    credential.wait_for_calls(2).await;
    assert_eq!(credential.tokens.load_threads().len(), 2);
    assert_stopped(&credential.service).await;

    let contract = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    contract
        .fetcher
        .push(FetchAction::Error(NewsFetchErrorKind::Contract, None));
    contract
        .fetcher
        .push(FetchAction::Pending(Arc::new(AtomicBool::new(false))));
    contract.service.start();
    contract
        .wait_for_status(NewsStatusKind::ContractError)
        .await;
    contract.service.recheck_credentials();
    tokio::task::yield_now().await;
    assert_eq!(contract.fetcher.calls().len(), 1);
    contract.service.retry_sync();
    contract.service.recheck_credentials();
    contract.wait_for_calls(2).await;
    assert_stopped(&contract.service).await;
}

#[tokio::test(start_paused = true)]
async fn commit_storage_failure_pauses_until_retry_and_event_failure_never_rolls_back() {
    let storage = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    storage.repository.fail_next_commit();
    storage.fetcher.push(FetchAction::Page(page(&[1], false)));
    storage
        .fetcher
        .push(FetchAction::Pending(Arc::new(AtomicBool::new(false))));
    storage.service.start();
    storage.wait_for_status(NewsStatusKind::StorageError).await;
    storage.service.retry_sync();
    storage.wait_for_calls(2).await;
    assert_stopped(&storage.service).await;

    let events = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    events.events.set_fail(true);
    events.fetcher.push(FetchAction::Page(page(&[7], false)));
    events.service.start();
    events.wait_for_status(NewsStatusKind::Live).await;
    assert_eq!(events.repository.snapshot().synced_count, 1);
    assert_eq!(events.events.committed.lock().unwrap().len(), 1);
    assert_stopped(&events.service).await;
}

#[tokio::test(start_paused = true)]
async fn either_manual_action_restarts_an_active_poll_cycle() {
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    let first = Arc::new(AtomicBool::new(false));
    let second = Arc::new(AtomicBool::new(false));
    rig.fetcher.push(FetchAction::Pending(first.clone()));
    rig.fetcher.push(FetchAction::Pending(second.clone()));
    rig.fetcher
        .push(FetchAction::Pending(Arc::new(AtomicBool::new(false))));
    rig.service.start();
    rig.wait_for_calls(1).await;

    rig.service.recheck_credentials();
    rig.wait_for_calls(2).await;
    assert!(first.load(Ordering::SeqCst));

    rig.service.retry_sync();
    rig.wait_for_calls(3).await;
    assert!(second.load(Ordering::SeqCst));
    assert_eq!(rig.tokens.load_threads().len(), 3);
    assert_stopped(&rig.service).await;
}

#[tokio::test(start_paused = true)]
async fn either_manual_action_interrupts_backoff_and_restarts_the_session() {
    for recheck_credentials in [true, false] {
        let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
        rig.fetcher
            .push(FetchAction::Error(NewsFetchErrorKind::Transient, None));
        rig.fetcher
            .push(FetchAction::Pending(Arc::new(AtomicBool::new(false))));
        rig.service.start();
        rig.wait_for_status(NewsStatusKind::Retrying).await;

        if recheck_credentials {
            rig.service.recheck_credentials();
        } else {
            rig.service.retry_sync();
        }
        rig.wait_for_calls(2).await;
        assert_eq!(rig.tokens.load_threads().len(), 2);
        assert_stopped(&rig.service).await;
    }
}

#[tokio::test(start_paused = true)]
async fn repeated_start_owns_one_task_and_cancellation_drops_pending_fetch() {
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    let dropped = Arc::new(AtomicBool::new(false));
    rig.fetcher.push(FetchAction::Pending(dropped.clone()));
    rig.service.start();
    rig.service.start();
    rig.service.start();
    rig.wait_for_calls(1).await;

    assert_stopped(&rig.service).await;

    assert_eq!(rig.fetcher.calls().len(), 1);
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(rig.service.status().kind, NewsStatusKind::Stopped);
}

#[tokio::test(start_paused = true)]
async fn shutdown_waits_for_inflight_commit_but_timeout_aborts_only_async_task() {
    let complete = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    let complete_gate = Arc::new(BlockingGate::default());
    complete.repository.block_commit(complete_gate.clone());
    complete.fetcher.push(FetchAction::Page(page(&[4], false)));
    complete.service.start();
    wait_for_gate(&complete_gate).await;
    let complete_service = complete.service.clone();
    let stopping = tokio::spawn(async move {
        complete_service
            .stop_and_join(Duration::from_secs(20))
            .await
    });
    tokio::task::yield_now().await;
    assert!(!stopping.is_finished());
    complete_gate.release();
    assert_eq!(stopping.await.unwrap(), NewsShutdownOutcome::Stopped);
    assert_eq!(complete.repository.snapshot().synced_count, 1);

    let timeout = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    let timeout_gate = Arc::new(BlockingGate::default());
    timeout.repository.block_commit(timeout_gate.clone());
    timeout.fetcher.push(FetchAction::Page(page(&[5], false)));
    timeout.service.start();
    wait_for_gate(&timeout_gate).await;
    let timeout_service = timeout.service.clone();
    let stopping =
        tokio::spawn(async move { timeout_service.stop_and_join(Duration::from_secs(5)).await });
    tokio::task::yield_now().await;
    let waiter_service = timeout.service.clone();
    let waiter =
        tokio::spawn(async move { waiter_service.stop_and_join(Duration::from_secs(20)).await });
    tokio::time::advance(Duration::from_secs(5)).await;
    let stopping_outcome = stopping.await.unwrap();
    let waiter_outcome = waiter.await.unwrap();
    timeout_gate.release();
    assert_eq!(stopping_outcome, NewsShutdownOutcome::TimedOut);
    assert_eq!(waiter_outcome, NewsShutdownOutcome::Stopped);
    assert_eq!(timeout.service.status().kind, NewsStatusKind::Stopped);
}

#[tokio::test(start_paused = true)]
async fn concurrent_stop_callers_share_commit_aware_completion_and_stopped_is_terminal() {
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    let gate = Arc::new(BlockingGate::default());
    rig.repository.block_commit(gate.clone());
    rig.fetcher.push(FetchAction::Page(page(&[6], false)));
    rig.service.start();
    wait_for_gate(&gate).await;

    let first_service = rig.service.clone();
    let first =
        tokio::spawn(async move { first_service.stop_and_join(Duration::from_secs(20)).await });
    let second_service = rig.service.clone();
    let second =
        tokio::spawn(async move { second_service.stop_and_join(Duration::from_secs(20)).await });
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }

    let first_returned_before_commit = first.is_finished();
    let second_returned_before_commit = second.is_finished();
    let stopped_before_commit = rig.service.status().kind == NewsStatusKind::Stopped;
    gate.release();
    assert_eq!(first.await.unwrap(), NewsShutdownOutcome::Stopped);
    assert_eq!(second.await.unwrap(), NewsShutdownOutcome::Stopped);

    assert!(!first_returned_before_commit);
    assert!(!second_returned_before_commit);
    assert!(!stopped_before_commit);
    assert_eq!(rig.service.status().kind, NewsStatusKind::Stopped);
    let statuses = rig.events.statuses.lock().unwrap().clone();
    let stopped_index = statuses
        .iter()
        .position(|status| status.kind == NewsStatusKind::Stopped)
        .expect("stopped status");
    assert_eq!(
        statuses
            .iter()
            .filter(|status| status.kind == NewsStatusKind::Stopped)
            .count(),
        1
    );
    assert!(statuses[stopped_index + 1..].iter().all(|status| !matches!(
        status.kind,
        NewsStatusKind::Live | NewsStatusKind::InitialSync
    )));
}

#[tokio::test(start_paused = true)]
async fn concurrent_waiter_timeout_does_not_finalize_owner_shutdown() {
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    let gate = Arc::new(BlockingGate::default());
    rig.repository.block_commit(gate.clone());
    rig.fetcher.push(FetchAction::Page(page(&[7], false)));
    rig.service.start();
    wait_for_gate(&gate).await;

    let owner_service = rig.service.clone();
    let owner =
        tokio::spawn(async move { owner_service.stop_and_join(Duration::from_secs(20)).await });
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(!owner.is_finished());

    let waiter_service = rig.service.clone();
    let waiter =
        tokio::spawn(async move { waiter_service.stop_and_join(Duration::from_secs(5)).await });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(5)).await;
    assert_eq!(waiter.await.unwrap(), NewsShutdownOutcome::TimedOut);

    assert!(!owner.is_finished());
    assert_ne!(rig.service.status().kind, NewsStatusKind::Stopped);
    gate.release();
    assert_eq!(owner.await.unwrap(), NewsShutdownOutcome::Stopped);
    assert_eq!(rig.service.status().kind, NewsStatusKind::Stopped);
}

async fn wait_for_gate(gate: &Arc<BlockingGate>) {
    let gate = gate.clone();
    let entered =
        tokio::task::spawn_blocking(move || gate.wait_until_entered(Duration::from_secs(5)))
            .await
            .expect("blocking gate waiter panicked");
    assert!(entered, "blocking commit did not begin");
}

async fn assert_stopped(service: &NewsService) {
    assert_eq!(
        service.stop_and_join(Duration::from_secs(1)).await,
        NewsShutdownOutcome::Stopped
    );
}
