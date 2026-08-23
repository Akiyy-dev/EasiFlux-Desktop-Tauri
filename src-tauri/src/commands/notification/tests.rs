use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::json;
use tokio::sync::RwLock;

use super::*;
use crate::error::{AppError, AppResult};
use crate::models::config::AppConfig;
use crate::models::notification::{
    NotificationCategory, NotificationContent, NotificationFilter, NotificationKind,
    NotificationRecord, NotificationScope, NotificationSeverity,
};
use crate::services::notification::{
    NotificationAvailability, NotificationEmitter, NotificationRuntime, NotificationService,
};
use crate::services::AccountLifecycleCoordinator;
use crate::storage::notification_store::{
    NotificationFileV1, NotificationPartition, NotificationPersistence,
};

const GLOBAL_ID: &str = "00000000-0000-4000-8000-000000000001";
const PRIMARY_ID: &str = "00000000-0000-4000-8000-000000000002";
const BACKUP_ID: &str = "00000000-0000-4000-8000-000000000003";
const NOW_MS: u64 = 1_700_000_000_100;

#[derive(Default)]
struct MemoryPersistence {
    fail: bool,
}

impl NotificationPersistence for MemoryPersistence {
    fn save(&self, _file: &NotificationFileV1) -> AppResult<()> {
        if self.fail {
            Err(AppError::Storage("NOTIFICATION_STORAGE_UNAVAILABLE".into()))
        } else {
            Ok(())
        }
    }
}

struct Fixture {
    runtime: Arc<NotificationRuntime>,
    config: Arc<RwLock<AppConfig>>,
    lifecycle: AccountLifecycleCoordinator,
    events: Arc<Mutex<Vec<crate::models::notification::NotificationChangedEvent>>>,
}

fn record(id: &str, scope: NotificationScope, created_at_ms: u64) -> NotificationRecord {
    NotificationRecord {
        id: id.into(),
        scope,
        category: NotificationCategory::ConnectionSystem,
        kind: NotificationKind::ConnectionUnavailable,
        severity: NotificationSeverity::Error,
        content: NotificationContent {
            message_key: "connection.unavailable".into(),
            params: BTreeMap::new(),
            fallback_title: "连接不可用".into(),
            fallback_body: "交易连接暂时不可用，请检查网络或稍后重试。".into(),
        },
        entity: None,
        action: None,
        source_event_id: None,
        dedupe_key: format!("record-{id}"),
        occurrence_count: 1,
        created_at_ms,
        updated_at_ms: created_at_ms,
        read_at_ms: None,
    }
}

fn fixture_with_persistence(persistence: MemoryPersistence, revision: u64) -> Fixture {
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&events);
    let emitter: NotificationEmitter = Arc::new(move |event| {
        captured.lock().unwrap().push(event.clone());
        Ok(())
    });
    let file = NotificationFileV1 {
        schema_version: 1,
        revision,
        source_event_index: Vec::new(),
        partitions: vec![
            NotificationPartition {
                scope: NotificationScope::Global,
                items: vec![record(GLOBAL_ID, NotificationScope::Global, NOW_MS - 3)],
            },
            NotificationPartition {
                scope: NotificationScope::Account {
                    account_id: "primary".into(),
                },
                items: vec![record(
                    PRIMARY_ID,
                    NotificationScope::Account {
                        account_id: "primary".into(),
                    },
                    NOW_MS - 2,
                )],
            },
            NotificationPartition {
                scope: NotificationScope::Account {
                    account_id: "backup".into(),
                },
                items: vec![record(
                    BACKUP_ID,
                    NotificationScope::Account {
                        account_id: "backup".into(),
                    },
                    NOW_MS - 1,
                )],
            },
        ],
    };
    let service = NotificationService::from_snapshot(file, Arc::new(persistence), emitter, NOW_MS);
    let mut config = AppConfig::default();
    config.active_account_id = "primary".into();
    config.accounts = vec!["primary".into(), "backup".into()];
    Fixture {
        runtime: Arc::new(NotificationRuntime::Available(Arc::new(service))),
        config: Arc::new(RwLock::new(config)),
        lifecycle: AccountLifecycleCoordinator::new(),
        events,
    }
}

fn fixture() -> Fixture {
    fixture_with_persistence(MemoryPersistence::default(), 7)
}

#[tokio::test]
async fn omitted_context_is_global_only_even_when_an_account_is_active() {
    let fixture = fixture();

    let page = list_notifications_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        None,
        Some(NotificationFilter::All),
        None,
        None,
        NOW_MS,
    )
    .await
    .unwrap();

    assert_eq!(
        page.items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        [GLOBAL_ID]
    );
    assert_eq!(page.unread_count, 1);
}

#[tokio::test]
async fn explicit_active_context_merges_account_and_global_with_default_and_max_limits() {
    let fixture = fixture();

    let page = list_notifications_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        Some("primary".into()),
        None,
        None,
        None,
        NOW_MS,
    )
    .await
    .unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        [PRIMARY_ID, GLOBAL_ID]
    );

    let max_page = list_notifications_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        Some("primary".into()),
        Some(NotificationFilter::All),
        None,
        Some(100),
        NOW_MS,
    )
    .await
    .unwrap();
    assert_eq!(max_page.items.len(), 2);

    let invalid = list_notifications_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        Some("primary".into()),
        None,
        None,
        Some(101),
        NOW_MS,
    )
    .await
    .unwrap_err();
    assert_eq!(invalid.code, "INVALID_NOTIFICATION_REQUEST");
}

#[tokio::test]
async fn stale_account_context_returns_scope_mismatch() {
    let fixture = fixture();
    fixture.config.write().await.accounts = vec!["primary".into()];

    let error = mark_notification_read_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        Some("backup".into()),
        BACKUP_ID.into(),
        NOW_MS,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "NOTIFICATION_SCOPE_MISMATCH");
    assert!(fixture.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn single_item_mutations_resolve_real_scope_before_authorizing() {
    let fixture = fixture();

    let omitted = delete_notification_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        None,
        PRIMARY_ID.into(),
        NOW_MS,
    )
    .await
    .unwrap_err();
    assert_eq!(omitted.code, "NOTIFICATION_SCOPE_MISMATCH");

    let marked = mark_notification_read_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        Some("primary".into()),
        PRIMARY_ID.into(),
        NOW_MS,
    )
    .await
    .unwrap();
    assert_eq!(marked.notification.id, PRIMARY_ID);
    assert_eq!(marked.unread_count, 1);
}

#[tokio::test]
async fn all_six_command_results_match_the_public_json_shapes() {
    let list_fixture = fixture();

    let page = list_notifications_inner(
        &list_fixture.runtime,
        &list_fixture.config,
        &list_fixture.lifecycle,
        None,
        Some(NotificationFilter::All),
        None,
        None,
        NOW_MS,
    )
    .await
    .unwrap();
    let page = serde_json::to_value(page).unwrap();
    assert_eq!(
        page.as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["items", "revision", "unreadCount"]
    );

    let read_fixture = fixture();
    let marked_one = mark_notification_read_inner(
        &read_fixture.runtime,
        &read_fixture.config,
        &read_fixture.lifecycle,
        Some("primary".into()),
        PRIMARY_ID.into(),
        NOW_MS,
    )
    .await
    .unwrap();
    let marked_one = serde_json::to_value(marked_one).unwrap();
    assert_eq!(
        marked_one
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["notification", "revision", "unreadCount"]
    );
    assert_eq!(marked_one["notification"]["id"], json!(PRIMARY_ID));

    let summary = get_notification_summary_inner(
        &list_fixture.runtime,
        &list_fixture.config,
        &list_fixture.lifecycle,
        Some("primary".into()),
        NOW_MS,
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(summary).unwrap(),
        json!({ "unreadCount": 2, "revision": "7" })
    );

    let marked = mark_visible_notifications_read_inner(
        &list_fixture.runtime,
        &list_fixture.config,
        &list_fixture.lifecycle,
        Some("primary".into()),
        NOW_MS,
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(marked).unwrap(),
        json!({
            "affectedCount": 2,
            "affectedScopes": [
                { "type": "global" },
                { "type": "account", "accountId": "primary" }
            ],
            "unreadCount": 0,
            "revision": "8"
        })
    );

    let deleted = delete_notification_inner(
        &list_fixture.runtime,
        &list_fixture.config,
        &list_fixture.lifecycle,
        Some("primary".into()),
        PRIMARY_ID.into(),
        NOW_MS,
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(deleted).unwrap(),
        json!({ "unreadCount": 0, "revision": "9" })
    );

    let cleared = clear_account_notifications_inner(
        &list_fixture.runtime,
        &list_fixture.config,
        &list_fixture.lifecycle,
        "primary".into(),
        NOW_MS,
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(cleared).unwrap(),
        json!({ "affectedCount": 0, "unreadCount": 0, "revision": "9" })
    );
}

#[tokio::test]
async fn clear_requires_a_configured_current_account() {
    let fixture = fixture();

    let stale = clear_account_notifications_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        "backup".into(),
        NOW_MS,
    )
    .await
    .unwrap_err();
    assert_eq!(stale.code, "NOTIFICATION_SCOPE_MISMATCH");

    let missing = clear_account_notifications_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        "missing".into(),
        NOW_MS,
    )
    .await
    .unwrap_err();
    assert_eq!(missing.code, "NOTIFICATION_ACCOUNT_NOT_FOUND");
}

#[tokio::test]
async fn revision_is_serialized_as_a_decimal_string() {
    let fixture = fixture_with_persistence(MemoryPersistence::default(), 9_007_199_254_740_993);

    let value = serde_json::to_value(
        get_notification_summary_inner(
            &fixture.runtime,
            &fixture.config,
            &fixture.lifecycle,
            None,
            NOW_MS,
        )
        .await
        .unwrap(),
    )
    .unwrap();

    assert_eq!(value["revision"], json!("9007199254740993"));
}

#[tokio::test]
async fn failed_save_returns_structured_error_and_emits_nothing() {
    let fixture = fixture_with_persistence(MemoryPersistence { fail: true }, 7);

    let error = delete_notification_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        None,
        GLOBAL_ID.into(),
        NOW_MS,
    )
    .await
    .unwrap_err();

    assert_eq!(
        serde_json::to_value(error).unwrap(),
        json!({
            "code": "NOTIFICATION_STORAGE_UNAVAILABLE",
            "message": "通知存储不可用"
        })
    );
    assert!(fixture.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unavailable_runtime_returns_a_stable_structured_command_error() {
    let fixture = fixture();
    let runtime = Arc::new(NotificationRuntime::Unavailable(
        NotificationAvailability::new("UNSUPPORTED_NOTIFICATION_SCHEMA", "通知存储版本暂不支持"),
    ));

    let error =
        get_notification_summary_inner(&runtime, &fixture.config, &fixture.lifecycle, None, NOW_MS)
            .await
            .unwrap_err();

    assert_eq!(
        serde_json::to_value(error).unwrap(),
        json!({
            "code": "UNSUPPORTED_NOTIFICATION_SCHEMA",
            "message": "通知存储版本暂不支持"
        })
    );
}
