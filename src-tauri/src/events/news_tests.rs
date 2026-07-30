use std::sync::Mutex;

use serde_json::json;

use super::{emit_news_payload, NEWS_MESSAGES_COMMITTED_EVENT, NEWS_STATUS_CHANGED_EVENT};
use crate::models::news::{NewsMessagesCommittedEvent, NewsStatusKind, NewsStatusSnapshot};
use crate::services::news::ports::NewsEventError;

#[test]
fn event_names_and_payloads_are_exact_and_camel_case() {
    assert_eq!(NEWS_MESSAGES_COMMITTED_EVENT, "news://messages-committed");
    assert_eq!(NEWS_STATUS_CHANGED_EVENT, "news://status-changed");

    let captured = Mutex::new(Vec::new());
    let committed = NewsMessagesCommittedEvent {
        inserted_count: 2,
        newest_delivery_id: Some("9223372036854775807".into()),
        unread_count: 2,
        initial_sync_complete: true,
    };
    emit_news_payload(
        |name, payload| {
            captured
                .lock()
                .unwrap()
                .push((name.to_owned(), serde_json::to_value(payload).unwrap()));
            Ok::<(), ()>(())
        },
        NEWS_MESSAGES_COMMITTED_EVENT,
        &committed,
    )
    .unwrap();

    let status = NewsStatusSnapshot {
        kind: NewsStatusKind::Retrying,
        initial_sync_complete: true,
        synced_count: 12,
        latest_delivery_id: Some("12".into()),
        unread_count: 3,
        retry_at: Some("2026-07-30T00:00:03.000Z".into()),
        message: Some("新闻服务暂时不可用，正在重试".into()),
    };
    emit_news_payload(
        |name, payload| {
            captured
                .lock()
                .unwrap()
                .push((name.to_owned(), serde_json::to_value(payload).unwrap()));
            Ok::<(), ()>(())
        },
        NEWS_STATUS_CHANGED_EVENT,
        &status,
    )
    .unwrap();

    assert_eq!(
        *captured.lock().unwrap(),
        vec![
            (
                "news://messages-committed".into(),
                json!({
                    "insertedCount": 2,
                    "newestDeliveryId": "9223372036854775807",
                    "unreadCount": 2,
                    "initialSyncComplete": true,
                }),
            ),
            (
                "news://status-changed".into(),
                json!({
                    "kind": "retrying",
                    "initialSyncComplete": true,
                    "syncedCount": 12,
                    "latestDeliveryId": "12",
                    "unreadCount": 3,
                    "retryAt": "2026-07-30T00:00:03.000Z",
                    "message": "新闻服务暂时不可用，正在重试",
                }),
            ),
        ]
    );
}

#[test]
fn adapter_failure_is_mapped_to_the_redacted_event_error() {
    let payload = NewsMessagesCommittedEvent {
        inserted_count: 0,
        newest_delivery_id: None,
        unread_count: 0,
        initial_sync_complete: false,
    };
    let error = emit_news_payload(
        |_, _| Err("private backend token=raw-token"),
        NEWS_MESSAGES_COMMITTED_EVENT,
        &payload,
    )
    .unwrap_err();

    assert_eq!(error, NewsEventError);
    assert_eq!(error.to_string(), "news event emission failed");
    assert!(!error.to_string().contains("raw-token"));
}
