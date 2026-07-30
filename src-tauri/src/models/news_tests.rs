use serde_json::{json, Value};

use super::{
    NewsMessageDto, NewsMessagesCommittedEvent, NewsPage, NewsStatusKind, NewsStatusSnapshot,
    NewsUnreadSnapshot,
};

#[test]
fn message_dto_serializes_decimal_string_id_and_only_public_fields() {
    let value = serde_json::to_value(NewsMessageDto {
        delivery_id: "9007199254740993".to_owned(),
        created_at: "2026-07-30T10:11:12Z".to_owned(),
        text: "market update".to_owned(),
    })
    .expect("serialize message DTO");

    assert_eq!(
        value,
        json!({
            "deliveryId": "9007199254740993",
            "createdAt": "2026-07-30T10:11:12Z",
            "text": "market update"
        })
    );
    assert_public_news_shape(&value);
}

#[test]
fn page_snapshot_and_event_use_camel_case_cross_tauri_fields() {
    let page = serde_json::to_value(NewsPage {
        items: vec![],
        has_more: true,
        latest_delivery_id: Some("42".to_owned()),
        unread_count: 7,
    })
    .expect("serialize page");
    let unread = serde_json::to_value(NewsUnreadSnapshot {
        latest_delivery_id: Some("42".to_owned()),
        unread_count: 7,
    })
    .expect("serialize unread snapshot");
    let committed = serde_json::to_value(NewsMessagesCommittedEvent {
        inserted_count: 2,
        newest_delivery_id: Some("42".to_owned()),
        unread_count: 7,
        initial_sync_complete: true,
    })
    .expect("serialize committed event");

    assert_eq!(
        page,
        json!({
            "items": [],
            "hasMore": true,
            "latestDeliveryId": "42",
            "unreadCount": 7
        })
    );
    assert_eq!(unread, json!({"latestDeliveryId": "42", "unreadCount": 7}));
    assert_eq!(
        committed,
        json!({
            "insertedCount": 2,
            "newestDeliveryId": "42",
            "unreadCount": 7,
            "initialSyncComplete": true
        })
    );
    assert_public_news_shape(&page);
    assert_public_news_shape(&unread);
    assert_public_news_shape(&committed);
}

#[test]
fn every_status_kind_serializes_to_exact_lower_camel_case() {
    let cases = [
        (
            NewsStatusKind::DeploymentMisconfigured,
            "deploymentMisconfigured",
        ),
        (NewsStatusKind::NotConfigured, "notConfigured"),
        (
            NewsStatusKind::CredentialStoreUnavailable,
            "credentialStoreUnavailable",
        ),
        (NewsStatusKind::InitialSync, "initialSync"),
        (NewsStatusKind::Live, "live"),
        (NewsStatusKind::Retrying, "retrying"),
        (NewsStatusKind::CredentialInvalid, "credentialInvalid"),
        (NewsStatusKind::ContractError, "contractError"),
        (NewsStatusKind::StorageError, "storageError"),
        (NewsStatusKind::Stopped, "stopped"),
    ];

    for (kind, expected) in cases {
        assert_eq!(
            serde_json::to_value(kind).expect("serialize status kind"),
            Value::String(expected.to_owned())
        );
    }
}

#[test]
fn status_snapshot_uses_camel_case_and_contains_no_upstream_metadata() {
    let value = serde_json::to_value(NewsStatusSnapshot {
        kind: NewsStatusKind::Retrying,
        initial_sync_complete: false,
        synced_count: 12,
        latest_delivery_id: Some("77".to_owned()),
        unread_count: 3,
        retry_at: Some("2026-07-30T10:11:12Z".to_owned()),
        message: Some("temporary failure".to_owned()),
    })
    .expect("serialize status snapshot");

    assert_eq!(
        value,
        json!({
            "kind": "retrying",
            "initialSyncComplete": false,
            "syncedCount": 12,
            "latestDeliveryId": "77",
            "unreadCount": 3,
            "retryAt": "2026-07-30T10:11:12Z",
            "message": "temporary failure"
        })
    );
    assert_public_news_shape(&value);
}

#[test]
fn absent_optional_cross_tauri_fields_are_omitted_instead_of_null() {
    let page = serde_json::to_value(NewsPage {
        items: vec![],
        has_more: false,
        latest_delivery_id: None,
        unread_count: 0,
    })
    .unwrap();
    let unread = serde_json::to_value(NewsUnreadSnapshot {
        latest_delivery_id: None,
        unread_count: 0,
    })
    .unwrap();
    let status = serde_json::to_value(NewsStatusSnapshot {
        kind: NewsStatusKind::Live,
        initial_sync_complete: true,
        synced_count: 0,
        latest_delivery_id: None,
        unread_count: 0,
        retry_at: None,
        message: None,
    })
    .unwrap();
    let committed = serde_json::to_value(NewsMessagesCommittedEvent {
        inserted_count: 0,
        newest_delivery_id: None,
        unread_count: 0,
        initial_sync_complete: true,
    })
    .unwrap();

    assert_eq!(
        page,
        json!({"items": [], "hasMore": false, "unreadCount": 0})
    );
    assert_eq!(unread, json!({"unreadCount": 0}));
    assert_eq!(
        status,
        json!({
            "kind": "live",
            "initialSyncComplete": true,
            "syncedCount": 0,
            "unreadCount": 0
        })
    );
    assert_eq!(
        committed,
        json!({
            "insertedCount": 0,
            "unreadCount": 0,
            "initialSyncComplete": true
        })
    );
}

fn assert_public_news_shape(value: &Value) {
    let serialized = serde_json::to_string(value).expect("serialize JSON value");
    for forbidden in [
        "source",
        "backend",
        "chat",
        "messageId",
        "user",
        "group",
        "media",
        "token",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "public news contract leaked forbidden field {forbidden}: {serialized}"
        );
    }
}
