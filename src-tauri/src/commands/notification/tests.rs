use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::json;
use tokio::sync::RwLock;

use super::*;
use crate::error::{AppError, AppResult};
use crate::models::config::AppConfig;
use crate::models::notification::{
    ListNotificationsRequest, NotificationCategory, NotificationContent, NotificationFilter,
    NotificationKind, NotificationRecord, NotificationScope, NotificationSeverity,
    DEFAULT_NOTIFICATION_PAGE_LIMIT,
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

#[derive(Default, Clone)]
struct MemoryPersistence {
    fail: bool,
    saves: Arc<Mutex<Vec<NotificationFileV1>>>,
}

impl NotificationPersistence for MemoryPersistence {
    fn save(&self, file: &NotificationFileV1) -> AppResult<()> {
        if self.fail {
            Err(AppError::Storage("NOTIFICATION_STORAGE_UNAVAILABLE".into()))
        } else {
            self.saves.lock().unwrap().push(file.clone());
            Ok(())
        }
    }
}

struct Fixture {
    runtime: Arc<NotificationRuntime>,
    config: Arc<RwLock<AppConfig>>,
    lifecycle: AccountLifecycleCoordinator,
    events: Arc<Mutex<Vec<crate::models::notification::NotificationChangedEvent>>>,
    persistence: MemoryPersistence,
}

#[derive(Deserialize)]
struct ListNotificationsCommandArgs {
    request: ListNotificationsRequest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateClientNotificationCommandArgs {
    request: crate::models::notification::CreateClientNotificationRequest,
}

#[test]
fn list_notifications_public_ipc_shape_is_one_nested_camel_case_request() {
    let args: ListNotificationsCommandArgs = serde_json::from_value(json!({
        "request": {
            "accountId": "primary",
            "filter": "unread",
            "cursor": "opaque-cursor",
            "limit": 100
        }
    }))
    .unwrap();

    assert_eq!(args.request.account_id.as_deref(), Some("primary"));
    assert_eq!(args.request.filter, NotificationFilter::Unread);
    assert_eq!(args.request.cursor.as_deref(), Some("opaque-cursor"));
    assert_eq!(args.request.limit, 100);

    let _public_command: fn(State<'static, AppState>, ListNotificationsRequest) -> _ =
        list_notifications;
}

#[test]
fn list_notifications_nested_request_owns_filter_and_limit_defaults() {
    let args: ListNotificationsCommandArgs =
        serde_json::from_value(json!({ "request": { "accountId": "primary" } })).unwrap();

    assert_eq!(args.request.filter, NotificationFilter::All);
    assert_eq!(args.request.limit, DEFAULT_NOTIFICATION_PAGE_LIMIT);
}

#[test]
fn list_notifications_rejects_flat_and_unknown_request_shapes() {
    let flat = serde_json::from_value::<ListNotificationsCommandArgs>(json!({
        "accountId": "primary",
        "filter": "all",
        "cursor": null,
        "limit": 50
    }));
    assert!(flat.is_err());

    let unknown_envelope =
        serde_json::from_value::<ListNotificationsCommandArgs>(json!({ "payload": {} }));
    assert!(unknown_envelope.is_err());

    let unknown_filter = serde_json::from_value::<ListNotificationsCommandArgs>(json!({
        "request": { "filter": "futureFilter" }
    }));
    assert!(unknown_filter.is_err());
}

#[test]
fn list_notifications_rejects_unknown_nested_fields() {
    let unknown_nested = serde_json::from_value::<ListNotificationsCommandArgs>(json!({
        "request": { "accountId": "primary", "futureOption": true }
    }));
    assert!(unknown_nested.is_err());
}

#[test]
fn client_bridge_raw_ipc_accepts_only_the_nested_closed_request() {
    let args: CreateClientNotificationCommandArgs = serde_json::from_value(json!({
        "request": {
            "accountId": "primary",
            "sessionEpoch": 0,
            "attemptId": "10000000-0000-4000-8000-000000000001",
            "kind": "accountRecoveryFailed",
            "failedSteps": ["bootstrap", "config"]
        }
    }))
    .unwrap();

    assert_eq!(args.request.account_id, "primary");
    assert_eq!(args.request.session_epoch, 0);
    assert_eq!(
        args.request.kind,
        crate::models::notification::ClientNotificationKind::AccountRecoveryFailed
    );

    let _public_command: fn(
        tauri::ipc::Request<'static>,
        State<'static, AppState>,
        crate::models::notification::CreateClientNotificationRequest,
    ) -> _ = create_client_notification;
}

#[test]
fn client_bridge_raw_ipc_rejects_display_policy_and_envelope_injection() {
    for injected in [
        json!({ "title": "forged" }),
        json!({ "body": "forged" }),
        json!({ "severity": "success" }),
        json!({ "action": { "type": "openTrading" } }),
        json!({ "messageKey": "order.filled" }),
        json!({ "rawError": "secret detail" }),
        json!({ "category": "trading" }),
        json!({ "scope": { "type": "global" } }),
        json!({ "sourceEventId": "forged" }),
        json!({ "dedupeKey": "forged" }),
    ] {
        let mut request = json!({
            "accountId": "primary",
            "sessionEpoch": 0,
            "attemptId": "10000000-0000-4000-8000-000000000001",
            "kind": "accountRecoveryFailed",
            "failedSteps": ["config"]
        });
        request
            .as_object_mut()
            .unwrap()
            .extend(injected.as_object().unwrap().clone());
        assert!(
            serde_json::from_value::<CreateClientNotificationCommandArgs>(json!({
                "request": request
            }))
            .is_err()
        );
    }

    for invalid in [
        json!({
            "request": {
                "accountId": "primary",
                "sessionEpoch": 0,
                "attemptId": "10000000-0000-4000-8000-000000000001",
                "kind": "accountRecoveryFailed",
                "failedSteps": ["config"]
            },
            "title": "outer-forgery"
        }),
        json!({
            "accountId": "primary",
            "sessionEpoch": 0,
            "attemptId": "10000000-0000-4000-8000-000000000001",
            "kind": "accountRecoveryFailed",
            "failedSteps": ["config"]
        }),
    ] {
        assert!(
            validate_client_notification_envelope(&tauri::ipc::InvokeBody::Json(invalid.clone()))
                .is_err()
        );
        assert!(serde_json::from_value::<CreateClientNotificationCommandArgs>(invalid).is_err());
    }
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
    let service =
        NotificationService::from_snapshot(file, Arc::new(persistence.clone()), emitter, NOW_MS);
    let mut config = AppConfig::default();
    config.active_account_id = "primary".into();
    config.accounts = vec!["primary".into(), "backup".into()];
    Fixture {
        runtime: Arc::new(NotificationRuntime::Available(Arc::new(service))),
        config: Arc::new(RwLock::new(config)),
        lifecycle: AccountLifecycleCoordinator::new(),
        events,
        persistence,
    }
}

fn fixture() -> Fixture {
    fixture_with_persistence(MemoryPersistence::default(), 7)
}

fn client_request(
    kind: crate::models::notification::ClientNotificationKind,
    failed_steps: Vec<crate::models::notification::ClientNotificationFailedStep>,
) -> crate::models::notification::CreateClientNotificationRequest {
    crate::models::notification::CreateClientNotificationRequest {
        account_id: "primary".into(),
        session_epoch: 0,
        attempt_id: "10000000-0000-4000-8000-000000000001".into(),
        kind,
        failed_steps,
    }
}

#[tokio::test]
async fn client_bridge_allowed_kinds_derive_closed_policy_and_deterministic_steps() {
    use crate::models::notification::{
        AccountNotificationSection, ClientNotificationFailedStep as Step,
        ClientNotificationKind as ClientKind, NotificationAction, NotificationKind,
        NotificationScalar, NotificationSeverity,
    };

    for (client_kind, expected_kind, severity, key) in [
        (
            ClientKind::AccountRecoveryFailed,
            NotificationKind::AccountRecoveryFailed,
            NotificationSeverity::Error,
            "account.recoveryFailed",
        ),
        (
            ClientKind::AccountReconciliationFailed,
            NotificationKind::AccountReconciliationFailed,
            NotificationSeverity::Critical,
            "account.reconciliationFailed",
        ),
    ] {
        let fixture = fixture();
        let result = create_client_notification_inner(
            &fixture.runtime,
            &fixture.config,
            &fixture.lifecycle,
            client_request(client_kind, vec![Step::Bootstrap, Step::Config]),
            NOW_MS,
        )
        .await
        .unwrap();

        assert_eq!(result.notification.kind, expected_kind);
        assert_eq!(result.notification.severity, severity);
        assert_eq!(result.notification.content.message_key, key);
        assert_eq!(
            result.notification.content.params.get("failedSteps"),
            Some(&NotificationScalar::String("config,bootstrap".into()))
        );
        assert!(matches!(
            result.notification.action,
            Some(NotificationAction::OpenAccountSettings {
                account_section: AccountNotificationSection::Api,
            })
        ));
        assert_eq!(result.notification.occurrence_count, 1);
        let suffix = match client_kind {
            ClientKind::AccountRecoveryFailed => "recovery",
            ClientKind::AccountReconciliationFailed => "reconciliation",
        };
        let expected_source = format!("client:10000000-0000-4000-8000-000000000001:{suffix}");
        assert_eq!(
            result.notification.source_event_id.as_deref(),
            Some(expected_source.as_str())
        );
        assert_eq!(
            result.notification.dedupe_key,
            format!("primary:10000000-0000-4000-8000-000000000001:{suffix}")
        );
        assert_eq!(result.unread_count, 3);
        assert_eq!(result.revision, "8");
        assert_eq!(fixture.persistence.saves.lock().unwrap().len(), 1);
        assert_eq!(fixture.events.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn client_bridge_replay_returns_the_existing_committed_id_without_mutation() {
    use crate::models::notification::{
        ClientNotificationFailedStep as Step, ClientNotificationKind as ClientKind,
    };

    let fixture = fixture();
    let request = client_request(
        ClientKind::AccountRecoveryFailed,
        vec![Step::Connection, Step::Config],
    );
    let first = create_client_notification_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        request.clone(),
        NOW_MS,
    )
    .await
    .unwrap();
    let replay = create_client_notification_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        request,
        NOW_MS + 1,
    )
    .await
    .unwrap();

    assert_eq!(replay.notification.id, first.notification.id);
    assert_eq!(replay.notification, first.notification);
    assert_eq!(replay.revision, first.revision);
    assert_eq!(replay.unread_count, first.unread_count);
    assert_eq!(fixture.persistence.saves.lock().unwrap().len(), 1);
    assert_eq!(fixture.events.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn client_bridge_same_attempt_with_different_kind_is_not_a_false_replay() {
    use crate::models::notification::{
        ClientNotificationFailedStep as Step, ClientNotificationKind as ClientKind,
    };

    let fixture = fixture();
    let recovery = create_client_notification_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        client_request(ClientKind::AccountRecoveryFailed, vec![Step::Connection]),
        NOW_MS,
    )
    .await
    .unwrap();
    let reconciliation = create_client_notification_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        client_request(
            ClientKind::AccountReconciliationFailed,
            vec![Step::Connection],
        ),
        NOW_MS + 1,
    )
    .await
    .unwrap();

    assert_ne!(reconciliation.notification.id, recovery.notification.id);
    assert_eq!(reconciliation.revision, "9");
    assert_eq!(fixture.persistence.saves.lock().unwrap().len(), 2);
    assert_eq!(fixture.events.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn client_bridge_replay_is_restart_idempotent_and_returns_the_real_record() {
    use crate::models::notification::{
        ClientNotificationFailedStep as Step, ClientNotificationKind as ClientKind,
    };

    let fixture = fixture();
    let request = client_request(
        ClientKind::AccountReconciliationFailed,
        vec![Step::Profiles, Step::Bootstrap],
    );
    let first = create_client_notification_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        request.clone(),
        NOW_MS,
    )
    .await
    .unwrap();
    let persisted = fixture
        .persistence
        .saves
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    let restart_persistence = MemoryPersistence::default();
    let restart_events = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&restart_events);
    let emitter: NotificationEmitter = Arc::new(move |event| {
        captured.lock().unwrap().push(event.clone());
        Ok(())
    });
    let restart_runtime = Arc::new(NotificationRuntime::Available(Arc::new(
        NotificationService::from_snapshot(
            persisted,
            Arc::new(restart_persistence.clone()),
            emitter,
            NOW_MS + 1,
        ),
    )));

    let replay = create_client_notification_inner(
        &restart_runtime,
        &fixture.config,
        &AccountLifecycleCoordinator::new(),
        request,
        NOW_MS + 1,
    )
    .await
    .unwrap();

    assert_eq!(replay.notification.id, first.notification.id);
    assert_eq!(replay.revision, first.revision);
    assert!(restart_persistence.saves.lock().unwrap().is_empty());
    assert!(restart_events.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_bridge_result_is_atomic_with_its_record_unread_count_and_revision() {
    use crate::models::notification::{
        ClientNotificationFailedStep as Step, ClientNotificationKind as ClientKind,
    };

    let persistence = MemoryPersistence::default();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first_event = Arc::new(AtomicBool::new(true));
    let gate = Arc::new(Mutex::new(Some(release_rx)));
    let emitter: NotificationEmitter = Arc::new({
        let first_event = Arc::clone(&first_event);
        let gate = Arc::clone(&gate);
        move |_| {
            if first_event.swap(false, Ordering::SeqCst) {
                started_tx.send(()).unwrap();
                gate.lock().unwrap().take().unwrap().recv().unwrap();
            }
            Ok(())
        }
    });
    let service = Arc::new(NotificationService::from_snapshot(
        NotificationFileV1::empty(),
        Arc::new(persistence),
        emitter,
        NOW_MS,
    ));
    let runtime = Arc::new(NotificationRuntime::Available(Arc::clone(&service)));
    let mut config = AppConfig::default();
    config.active_account_id = "primary".into();
    config.accounts = vec!["primary".into()];
    let config = Arc::new(RwLock::new(config));
    let lifecycle = Arc::new(AccountLifecycleCoordinator::new());

    let first = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        let config = Arc::clone(&config);
        let lifecycle = Arc::clone(&lifecycle);
        async move {
            create_client_notification_inner(
                &runtime,
                &config,
                lifecycle.as_ref(),
                client_request(ClientKind::AccountRecoveryFailed, vec![Step::Connection]),
                NOW_MS,
            )
            .await
        }
    });
    tokio::task::spawn_blocking(move || started_rx.recv().unwrap())
        .await
        .unwrap();
    let second = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        let config = Arc::clone(&config);
        let lifecycle = Arc::clone(&lifecycle);
        async move {
            let mut request = client_request(
                ClientKind::AccountReconciliationFailed,
                vec![Step::Bootstrap],
            );
            request.attempt_id = "10000000-0000-4000-8000-000000000002".into();
            create_client_notification_inner(
                &runtime,
                &config,
                lifecycle.as_ref(),
                request,
                NOW_MS + 1,
            )
            .await
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    release_tx.send(()).unwrap();

    let first = first.await.unwrap().unwrap();
    let second = second.await.unwrap().unwrap();
    assert_eq!(first.revision, "1");
    assert_eq!(first.unread_count, 1);
    assert_eq!(second.revision, "2");
    assert_eq!(second.unread_count, 2);
}

#[tokio::test]
async fn client_bridge_rejects_malformed_stale_and_wrong_owner_before_store_mutation() {
    use crate::models::notification::{
        ClientNotificationFailedStep as Step, ClientNotificationKind as ClientKind,
    };

    let fixture = fixture();
    let malformed_requests = vec![
        client_request(ClientKind::AccountRecoveryFailed, Vec::new()),
        client_request(
            ClientKind::AccountRecoveryFailed,
            vec![Step::Config, Step::Config],
        ),
    ];
    let mut malformed_attempt =
        client_request(ClientKind::AccountRecoveryFailed, vec![Step::Config]);
    malformed_attempt.attempt_id = "not-a-uuid".into();
    let mut noncanonical_account =
        client_request(ClientKind::AccountRecoveryFailed, vec![Step::Config]);
    noncanonical_account.account_id = " primary ".into();

    for request in malformed_requests
        .into_iter()
        .chain([malformed_attempt, noncanonical_account])
    {
        let error = create_client_notification_inner(
            &fixture.runtime,
            &fixture.config,
            &fixture.lifecycle,
            request,
            NOW_MS,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "INVALID_NOTIFICATION_REQUEST");
        assert_eq!(error.message, "客户端通知请求无效");
    }

    let mut wrong_account = client_request(ClientKind::AccountRecoveryFailed, vec![Step::Config]);
    wrong_account.account_id = "backup".into();
    let error = create_client_notification_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        wrong_account,
        NOW_MS,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "NOTIFICATION_SCOPE_MISMATCH");
    assert_eq!(error.message, "通知账户范围不匹配");

    let mut wrong_epoch = client_request(ClientKind::AccountRecoveryFailed, vec![Step::Config]);
    wrong_epoch.session_epoch = 1;
    let error = create_client_notification_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        wrong_epoch,
        NOW_MS,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "NOTIFICATION_SESSION_MISMATCH");
    assert_eq!(error.message, "通知会话代次不匹配");

    assert_eq!(fixture.runtime.service().unwrap().revision().await, "7");
    assert!(fixture.persistence.saves.lock().unwrap().is_empty());
    assert!(fixture.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn client_bridge_unavailable_runtime_returns_only_the_stable_sanitized_error() {
    use crate::models::notification::{
        ClientNotificationFailedStep as Step, ClientNotificationKind as ClientKind,
    };

    let fixture = fixture();
    let runtime = Arc::new(NotificationRuntime::Unavailable(
        NotificationAvailability::new("UNSUPPORTED_NOTIFICATION_SCHEMA", "通知存储版本暂不支持"),
    ));
    let error = create_client_notification_inner(
        &runtime,
        &fixture.config,
        &fixture.lifecycle,
        client_request(ClientKind::AccountRecoveryFailed, vec![Step::Connection]),
        NOW_MS,
    )
    .await
    .unwrap_err();

    assert_eq!(
        serde_json::to_value(error).unwrap(),
        json!({
            "code": "UNSUPPORTED_NOTIFICATION_SCHEMA",
            "message": "通知存储版本暂不支持"
        })
    );
    assert!(fixture.persistence.saves.lock().unwrap().is_empty());
    assert!(fixture.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn client_bridge_storage_failure_is_sanitized_copy_on_write_without_a_fake_id() {
    use crate::models::notification::{
        ClientNotificationFailedStep as Step, ClientNotificationKind as ClientKind,
    };

    let fixture = fixture_with_persistence(
        MemoryPersistence {
            fail: true,
            ..MemoryPersistence::default()
        },
        7,
    );
    let error = create_client_notification_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        client_request(ClientKind::AccountRecoveryFailed, vec![Step::Connection]),
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
    assert_eq!(fixture.runtime.service().unwrap().revision().await, "7");
    assert!(fixture.persistence.saves.lock().unwrap().is_empty());
    assert!(fixture.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn omitted_context_is_global_only_even_when_an_account_is_active() {
    let fixture = fixture();

    let page = list_notifications_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        ListNotificationsRequest {
            account_id: None,
            filter: NotificationFilter::All,
            cursor: None,
            limit: DEFAULT_NOTIFICATION_PAGE_LIMIT,
        },
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
        ListNotificationsRequest {
            account_id: Some("primary".into()),
            filter: NotificationFilter::All,
            cursor: None,
            limit: DEFAULT_NOTIFICATION_PAGE_LIMIT,
        },
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
        ListNotificationsRequest {
            account_id: Some("primary".into()),
            filter: NotificationFilter::All,
            cursor: None,
            limit: 100,
        },
        NOW_MS,
    )
    .await
    .unwrap();
    assert_eq!(max_page.items.len(), 2);

    let invalid = list_notifications_inner(
        &fixture.runtime,
        &fixture.config,
        &fixture.lifecycle,
        ListNotificationsRequest {
            account_id: Some("primary".into()),
            filter: NotificationFilter::All,
            cursor: None,
            limit: 101,
        },
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
        ListNotificationsRequest {
            account_id: None,
            filter: NotificationFilter::All,
            cursor: None,
            limit: DEFAULT_NOTIFICATION_PAGE_LIMIT,
        },
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
    let fixture = fixture_with_persistence(
        MemoryPersistence {
            fail: true,
            ..MemoryPersistence::default()
        },
        7,
    );

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
