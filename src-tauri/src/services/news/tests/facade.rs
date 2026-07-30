use std::sync::Arc;
use std::time::Duration;

use crate::models::news::{NewsStatusKind, NewsStatusSnapshot};
use crate::services::news::{NewsService, NewsServiceErrorKind, NewsShutdownOutcome};

use super::support::{BlockingGate, RecordingEvents, Rig, TokenOutcome};

fn live_status(latest_delivery_id: &str, unread_count: u32) -> NewsStatusSnapshot {
    NewsStatusSnapshot {
        kind: NewsStatusKind::Live,
        initial_sync_complete: true,
        synced_count: 1,
        latest_delivery_id: Some(latest_delivery_id.into()),
        unread_count,
        retry_at: None,
        message: None,
    }
}

#[tokio::test]
async fn degraded_services_remain_callable_without_startup_failure() {
    let events = Arc::new(RecordingEvents::default());
    let deployment = Arc::new(NewsService::deployment_misconfigured(events.clone()));
    deployment.start();
    assert_eq!(
        deployment.status().kind,
        NewsStatusKind::DeploymentMisconfigured
    );
    assert_eq!(
        deployment.list_messages(None, 50).await.unwrap_err().kind(),
        NewsServiceErrorKind::Unavailable
    );

    let storage = Arc::new(NewsService::storage_error(events));
    storage.start();
    assert_eq!(storage.status().kind, NewsStatusKind::StorageError);
    assert_eq!(
        storage.mark_seen(1).await.unwrap_err().kind(),
        NewsServiceErrorKind::Unavailable
    );
}

#[tokio::test]
async fn list_and_mark_seen_use_only_repository_on_blocking_pool_threads() {
    let runtime_thread = std::thread::current().id();
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));

    let page = rig.service.list_messages(None, 50).await.unwrap();
    let unread = rig.service.mark_seen(9).await.unwrap();

    assert_eq!(page.items[0].text, "cached");
    assert_eq!(unread.unread_count, 0);
    assert!(rig.fetcher.calls().is_empty());
    assert!(rig.tokens.load_threads().is_empty());
    let calls = rig.repository.calls();
    assert_eq!(
        calls.iter().map(|call| call.0).collect::<Vec<_>>(),
        ["list", "mark"]
    );
    assert!(calls.iter().all(|call| call.1 != runtime_thread));
}

#[tokio::test]
async fn mark_seen_updates_matching_in_memory_status_without_emitting_status_event() {
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    rig.service.publish(live_status("9", 1));
    let status_event_count = rig.events.statuses.lock().unwrap().len();

    let unread = rig.service.mark_seen(9).await.unwrap();

    assert_eq!(unread.latest_delivery_id.as_deref(), Some("9"));
    assert_eq!(unread.unread_count, 0);
    let status = rig.service.status();
    assert_eq!(status.latest_delivery_id.as_deref(), Some("9"));
    assert_eq!(status.unread_count, 0);
    assert_eq!(
        rig.events.statuses.lock().unwrap().len(),
        status_event_count
    );
}

#[tokio::test]
async fn delayed_same_latest_publish_cannot_restore_unread_after_mark_seen() {
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    let stale = live_status("9", 1);
    rig.service.publish(stale.clone());
    rig.service.mark_seen(9).await.unwrap();
    assert_eq!(rig.service.status().unread_count, 0);

    let mut delayed = stale;
    delayed.message = Some("delayed live snapshot".into());
    rig.service.publish(delayed);

    let status = rig.service.status();
    assert_eq!(status.latest_delivery_id.as_deref(), Some("9"));
    assert_eq!(status.unread_count, 0);
    assert_eq!(status.message.as_deref(), Some("delayed live snapshot"));
    let events = rig.events.statuses.lock().unwrap();
    let event = events.last().unwrap();
    assert_eq!(event.latest_delivery_id.as_deref(), Some("9"));
    assert_eq!(event.unread_count, 0);
    assert_eq!(event.message.as_deref(), Some("delayed live snapshot"));
}

#[tokio::test]
async fn older_publish_keeps_current_anchor_while_updating_presentation_fields() {
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    let mut current = live_status("12", 2);
    current.synced_count = 12;
    rig.service.publish(current);
    let mut incoming = live_status("9", 0);
    incoming.initial_sync_complete = false;
    incoming.synced_count = 3;
    incoming.kind = NewsStatusKind::Retrying;
    incoming.retry_at = Some("2026-07-30T12:00:00.000Z".into());
    incoming.message = Some("retrying delayed snapshot".into());

    rig.service.publish(incoming);

    let status = rig.service.status();
    assert_eq!(status.latest_delivery_id.as_deref(), Some("12"));
    assert_eq!(status.unread_count, 2);
    assert!(status.initial_sync_complete);
    assert_eq!(status.synced_count, 12);
    assert_eq!(status.kind, NewsStatusKind::Retrying);
    assert_eq!(status.retry_at.as_deref(), Some("2026-07-30T12:00:00.000Z"));
    assert_eq!(status.message.as_deref(), Some("retrying delayed snapshot"));
    assert_eq!(rig.events.statuses.lock().unwrap().last(), Some(&status));
}

#[tokio::test]
async fn newer_publish_adopts_incoming_latest_and_unread() {
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    rig.service.publish(live_status("9", 0));

    rig.service.publish(live_status("12", 3));

    let status = rig.service.status();
    assert_eq!(status.latest_delivery_id.as_deref(), Some("12"));
    assert_eq!(status.unread_count, 3);
    assert_eq!(rig.events.statuses.lock().unwrap().last(), Some(&status));
}

#[tokio::test]
async fn stale_mark_seen_response_does_not_overwrite_newer_concurrent_status() {
    let rig = Rig::new(TokenOutcome::Present(b"token-a".to_vec()));
    rig.service.publish(live_status("9", 1));
    let gate = Arc::new(BlockingGate::default());
    rig.repository.block_mark(gate.clone());
    let service = rig.service.clone();
    let mark = tokio::spawn(async move { service.mark_seen(9).await.unwrap() });
    for _ in 0..10_000 {
        if gate.entered() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(gate.entered(), "mark_seen did not enter the repository");

    rig.service.publish(live_status("12", 2));
    let status_event_count = rig.events.statuses.lock().unwrap().len();
    gate.release();
    let unread = mark.await.unwrap();

    assert_eq!(unread.latest_delivery_id.as_deref(), Some("9"));
    assert_eq!(unread.unread_count, 0);
    let status = rig.service.status();
    assert_eq!(status.latest_delivery_id.as_deref(), Some("12"));
    assert_eq!(status.unread_count, 2);
    assert_eq!(
        rig.events.statuses.lock().unwrap().len(),
        status_event_count
    );
}

#[tokio::test(start_paused = true)]
async fn repeated_stop_is_safe_and_keeps_final_stopped_status() {
    let rig = Rig::new(TokenOutcome::Missing);
    rig.service.start();
    rig.wait_for_status(NewsStatusKind::NotConfigured).await;

    assert_eq!(
        rig.service.stop_and_join(Duration::from_secs(1)).await,
        NewsShutdownOutcome::Stopped
    );
    assert_eq!(
        rig.service.stop_and_join(Duration::from_secs(1)).await,
        NewsShutdownOutcome::Stopped
    );

    assert_eq!(rig.service.status().kind, NewsStatusKind::Stopped);
}
