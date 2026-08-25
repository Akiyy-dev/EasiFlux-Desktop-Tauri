use std::fs;
use std::sync::Arc;

use crate::models::notification::{
    ClientNotificationFailedStep, ClientNotificationKind, CreateClientNotificationRequest,
    ListNotificationsRequest, NotificationChange, NotificationFilter, NotificationRecord,
    NotificationScope,
};
use crate::services::notification::{
    NotificationEmitter, NotificationPolicy, NotificationService,
    NotificationStorageFailureReporter, ViewContext,
};
use crate::storage::notification_store::{
    FailurePoint, NotificationFileV1, NotificationPartition, NotificationSourceEventIndexEntry,
    NotificationStore, NOTIFICATION_SCHEMA_VERSION,
};

use super::support::{harness, input, FakePersistence};

const NOW: u64 = 1_700_000_000_000;

fn recovery_test_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir()
        .join(format!(
            "easiflux-notification-recovery-report-{}-{}-{label}",
            std::process::id(),
            uuid::Uuid::new_v4(),
        ))
        .join("notifications.v1.json")
}

#[tokio::test]
async fn startup_recovery_write_failures_share_diagnostic_throttle_and_preserve_semantics() {
    let diagnostic_events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let reporter = NotificationStorageFailureReporter::new(crate::events::EventEmitter::new_test(
        Arc::clone(&diagnostic_events),
    ));
    let changed_emitter: NotificationEmitter = Arc::new(|_| Ok(()));

    for (source, now_ms, revision, expected_logs) in [("temp", 0, 10, 1), ("backup", 1, 11, 2)] {
        let recovered_path = recovery_test_path(source);
        fs::create_dir_all(recovered_path.parent().unwrap()).unwrap();
        let mut recovered_file = NotificationFileV1::empty();
        recovered_file.revision = revision;
        let source_path = match source {
            "temp" => NotificationStore::temp_path_for_test(&recovered_path),
            "backup" => NotificationStore::backup_path_for_test(&recovered_path),
            _ => unreachable!(),
        };
        fs::write(source_path, serde_json::to_vec(&recovered_file).unwrap()).unwrap();
        let recovered = NotificationService::load_with_reporter(
            NotificationStore::with_path_and_failures(
                recovered_path.clone(),
                vec![FailurePoint::PromoteTemp],
            ),
            &[],
            now_ms,
            Arc::clone(&changed_emitter),
            reporter.clone(),
        )
        .expect("normalization write failure still returns recovered data");
        assert_eq!(recovered.revision().await, revision.to_string());
        let events = diagnostic_events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|(name, _)| name == "log:entry")
                .count(),
            expected_logs
        );
        assert_eq!(
            events
                .iter()
                .filter(|(name, _)| name == "error:occurred")
                .count(),
            1
        );
        drop(events);
        fs::remove_dir_all(recovered_path.parent().unwrap()).unwrap();
    }

    for (now_ms, expected_logs, expected_toasts) in [(59_999, 3, 1), (60_000, 4, 2)] {
        let corrupt_path = recovery_test_path("fatal");
        fs::create_dir_all(corrupt_path.parent().unwrap()).unwrap();
        let corrupt_bytes = b"corrupt-evidence-must-remain";
        fs::write(&corrupt_path, corrupt_bytes).unwrap();
        let result = NotificationService::load_with_reporter(
            NotificationStore::with_path_and_failures(
                corrupt_path.clone(),
                vec![FailurePoint::PreserveCorrupt],
            ),
            &[],
            now_ms,
            Arc::clone(&changed_emitter),
            reporter.clone(),
        );
        let error = match result {
            Ok(_) => panic!("evidence preservation write failure must be fatal"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "NOTIFICATION_STORAGE_UNAVAILABLE");
        assert_eq!(fs::read(&corrupt_path).unwrap(), corrupt_bytes);
        let events = diagnostic_events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|(name, _)| name == "log:entry")
                .count(),
            expected_logs
        );
        assert_eq!(
            events
                .iter()
                .filter(|(name, _)| name == "error:occurred")
                .count(),
            expected_toasts
        );
        assert!(events.iter().all(|(_, payload)| payload
            .to_string()
            .contains("NOTIFICATION_STORAGE_UNAVAILABLE")));
        drop(events);
        fs::remove_dir_all(corrupt_path.parent().unwrap()).unwrap();
    }
}

#[tokio::test]
async fn storage_failures_always_log_throttle_toast_and_reset_after_durable_write() {
    let persistence = Arc::new(FakePersistence::default());
    let changed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let changed_sink = Arc::clone(&changed);
    let changed_emitter: NotificationEmitter = Arc::new(move |event| {
        changed_sink.lock().unwrap().push(event.clone());
        Ok(())
    });
    let diagnostic_events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let event_emitter = crate::events::EventEmitter::new_test(Arc::clone(&diagnostic_events));
    let reporter = NotificationStorageFailureReporter::new(event_emitter);
    let service = NotificationService::from_snapshot_with_reporter(
        NotificationFileV1::empty(),
        Arc::clone(&persistence),
        changed_emitter,
        reporter,
        0,
    );
    let first = input(
        NotificationScope::Account {
            account_id: "alpha".into(),
        },
        "storage-source-1",
        "storage-dedupe-1",
    );

    for (now_ms, expected_logs, expected_toasts) in [(0, 1, 1), (59_999, 2, 1), (60_000, 3, 2)] {
        persistence.fail_next();
        let error = service.publish(first.clone(), now_ms).await.unwrap_err();
        assert_eq!(error.code(), "NOTIFICATION_STORAGE_UNAVAILABLE");
        let events = diagnostic_events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|(name, _)| name == "log:entry")
                .count(),
            expected_logs
        );
        assert_eq!(
            events
                .iter()
                .filter(|(name, _)| name == "error:occurred")
                .count(),
            expected_toasts
        );
        assert!(events.iter().all(|(_, payload)| {
            payload
                .to_string()
                .contains("NOTIFICATION_STORAGE_UNAVAILABLE")
        }));
        drop(events);
        assert_eq!(service.revision().await, "0");
        assert!(changed.lock().unwrap().is_empty());
    }

    service.publish(first, 60_001).await.unwrap();
    assert_eq!(service.revision().await, "1");
    assert_eq!(changed.lock().unwrap().len(), 1);

    persistence.fail_next();
    let second = input(
        NotificationScope::Account {
            account_id: "alpha".into(),
        },
        "storage-source-2",
        "storage-dedupe-2",
    );
    service.publish(second, 60_002).await.unwrap_err();
    let events = diagnostic_events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|(name, _)| name == "log:entry")
            .count(),
        4
    );
    assert_eq!(
        events
            .iter()
            .filter(|(name, _)| name == "error:occurred")
            .count(),
        3
    );
    assert_eq!(service.revision().await, "1");
    assert_eq!(changed.lock().unwrap().len(), 1);
}

fn client_input(kind: ClientNotificationKind) -> crate::models::notification::NotificationInput {
    NotificationPolicy
        .client_account_failure(CreateClientNotificationRequest {
            account_id: "alpha".into(),
            session_epoch: 7,
            attempt_id: "10000000-0000-4000-8000-000000000001".into(),
            kind,
            failed_steps: vec![ClientNotificationFailedStep::Config],
        })
        .unwrap()
}

fn record_from_input(
    input: crate::models::notification::NotificationInput,
    id: &str,
) -> NotificationRecord {
    NotificationRecord {
        id: id.into(),
        scope: input.scope,
        category: input.category,
        kind: input.kind,
        severity: input.severity,
        content: input.content,
        entity: input.entity,
        action: input.action,
        source_event_id: input.source_event_id,
        dedupe_key: input.dedupe_key,
        occurrence_count: 1,
        created_at_ms: NOW - 1,
        updated_at_ms: NOW - 1,
        read_at_ms: None,
    }
}

fn request(
    account_id: Option<&str>,
    filter: NotificationFilter,
    cursor: Option<String>,
    limit: u32,
) -> ListNotificationsRequest {
    ListNotificationsRequest {
        account_id: account_id.map(str::to_owned),
        filter,
        cursor,
        limit,
    }
}

#[tokio::test]
async fn identical_scope_and_source_event_is_an_absolute_no_op() {
    let harness = harness(NotificationFileV1::empty());
    let item = input(
        NotificationScope::Account {
            account_id: "alpha".into(),
        },
        "source-1",
        "alpha:order-1:filled",
    );

    let first = harness.service.publish(item.clone(), NOW).await.unwrap();
    let second = harness.service.publish(item, NOW + 1).await.unwrap();

    let first_id = first.notification.unwrap().id;
    assert!(first.committed);
    assert_eq!(second.notification.unwrap().id, first_id);
    assert!(!second.committed);
    assert_eq!(second.revision, "1");
    assert_eq!(harness.persistence.saves().len(), 1);
    assert_eq!(harness.events.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn concurrent_and_restart_source_replay_return_the_stable_live_record() {
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let item = input(
        scope.clone(),
        "session:7:expired",
        "alpha:session:7:expired",
    );
    let first_run = harness(NotificationFileV1::empty());

    let (left, right) = tokio::join!(
        first_run.service.publish(item.clone(), NOW),
        first_run.service.publish(item.clone(), NOW),
    );
    let left = left.unwrap();
    let right = right.unwrap();
    let left_id = left.notification.unwrap().id;
    let right_id = right.notification.unwrap().id;
    assert_eq!(left_id, right_id);
    assert_eq!(
        [left.committed, right.committed]
            .into_iter()
            .filter(|value| *value)
            .count(),
        1
    );
    assert_eq!(first_run.persistence.saves().len(), 1);
    assert_eq!(first_run.events.lock().unwrap().len(), 1);

    let persisted = first_run.persistence.saves().pop().unwrap();
    let restarted = harness(persisted);
    let replay = restarted.service.publish(item, NOW + 1).await.unwrap();
    assert_eq!(replay.notification.unwrap().id, left_id);
    assert!(!replay.committed);
    assert!(restarted.persistence.saves().is_empty());
    assert!(restarted.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn effective_session_token_dedupes_one_edge_and_rearms_after_recovery_or_restart() {
    let first_run = harness(NotificationFileV1::empty());
    let token_one = "10000000-0000-4000-8000-000000000001";
    let (left, right) = tokio::join!(
        first_run
            .service
            .observe_session_expired("alpha".into(), 7, token_one, NOW),
        first_run
            .service
            .observe_session_expired("alpha".into(), 7, token_one, NOW),
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_eq!(
        left.notification.as_ref().unwrap().id,
        right.notification.as_ref().unwrap().id
    );
    assert_eq!(
        first_run
            .service
            .summary(ViewContext::account("alpha").unwrap(), NOW)
            .await
            .unwrap()
            .unread_count,
        1
    );
    assert_eq!(first_run.persistence.saves().len(), 1);
    {
        let events = first_run.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].change, NotificationChange::Created);
        assert!(events[0].toast_candidate.is_some());
    }

    let replay = first_run
        .service
        .observe_session_expired("alpha".into(), 7, token_one, NOW + 1)
        .await
        .unwrap();
    assert!(!replay.committed);
    assert_eq!(first_run.persistence.saves().len(), 1);
    assert_eq!(first_run.events.lock().unwrap().len(), 1);

    let recovered_session = first_run
        .service
        .observe_session_expired(
            "alpha".into(),
            7,
            "20000000-0000-4000-8000-000000000002",
            NOW + 2,
        )
        .await
        .unwrap();
    assert!(recovered_session.committed);
    assert_eq!(
        first_run
            .service
            .summary(ViewContext::account("alpha").unwrap(), NOW + 2)
            .await
            .unwrap()
            .unread_count,
        2
    );
    assert_eq!(first_run.persistence.saves().len(), 2);
    {
        let events = first_run.events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].change, NotificationChange::Created);
        assert!(events[1].toast_candidate.is_some());
    }

    let persisted = first_run.persistence.saves().last().unwrap().clone();
    let restarted = harness(persisted);
    let restarted_session = restarted
        .service
        .observe_session_expired(
            "alpha".into(),
            7,
            "30000000-0000-4000-8000-000000000003",
            NOW + 3,
        )
        .await
        .unwrap();
    assert!(restarted_session.committed);
    assert_eq!(
        restarted
            .service
            .summary(ViewContext::account("alpha").unwrap(), NOW + 3)
            .await
            .unwrap()
            .unread_count,
        3
    );
    assert_eq!(restarted.persistence.saves().len(), 1);
    {
        let events = restarted.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].change, NotificationChange::Created);
        assert!(events[0].toast_candidate.is_some());
    }
}

#[tokio::test]
async fn service_roundtrips_every_canonical_account_id_shape() {
    let harness = harness(NotificationFileV1::empty());
    for (index, account_id) in [
        "trader.name@example.com".to_string(),
        "desk.alpha".to_string(),
        "账户-甲".to_string(),
        "a".repeat(96),
        "token-secret".to_string(),
    ]
    .into_iter()
    .enumerate()
    {
        let outcome = harness
            .service
            .observe_session_expired(
                account_id.clone(),
                7,
                &format!("10000000-0000-4000-8000-{index:012}"),
                NOW + index as u64,
            )
            .await
            .unwrap();
        let notification = outcome.notification.unwrap();
        let page = harness
            .service
            .list(
                ViewContext::account(&account_id).unwrap(),
                ListNotificationsRequest {
                    account_id: Some(account_id.clone()),
                    filter: NotificationFilter::All,
                    cursor: None,
                    limit: 100,
                },
                NOW + index as u64,
            )
            .await
            .unwrap();

        assert_eq!(page.items[0].id, notification.id);
        assert_eq!(
            page.items[0].scope,
            NotificationScope::Account {
                account_id: account_id.clone(),
            }
        );
        assert!(!page.items[0].dedupe_key.contains(&account_id));
    }
    assert_eq!(harness.persistence.saves().len(), 5);
}

#[tokio::test]
async fn source_publish_persistence_failure_returns_no_replay_marker_or_state() {
    let harness = harness(NotificationFileV1::empty());
    harness.persistence.fail_next();
    let item = input(
        NotificationScope::Account {
            account_id: "alpha".into(),
        },
        "session:7:expired",
        "alpha:session:7:expired",
    );

    let error = harness
        .service
        .publish(item.clone(), NOW)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "NOTIFICATION_STORAGE_UNAVAILABLE");
    assert!(harness.persistence.saves().is_empty());
    assert!(harness.events.lock().unwrap().is_empty());

    let retry = harness.service.publish(item, NOW + 1).await.unwrap();
    assert!(retry.committed);
    assert!(retry.notification.is_some());
    assert_eq!(harness.persistence.saves().len(), 1);
    assert_eq!(harness.events.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn client_replay_rejects_an_indexed_opposite_kind_without_any_mutation() {
    let expected = client_input(ClientNotificationKind::AccountRecoveryFailed);
    let target = record_from_input(
        client_input(ClientNotificationKind::AccountReconciliationFailed),
        "00000000-0000-4000-8000-000000000001",
    );
    let scope = expected.scope.clone();
    let source_event_id = expected.source_event_id.clone().unwrap();
    let harness = harness(NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 9,
        source_event_index: vec![NotificationSourceEventIndexEntry {
            scope: scope.clone(),
            source_event_id,
            notification_id: target.id.clone(),
        }],
        partitions: vec![NotificationPartition {
            scope,
            items: vec![target],
        }],
    });
    let context = ViewContext::account("alpha").unwrap();
    let before = harness
        .service
        .list(
            context.clone(),
            request(Some("alpha"), NotificationFilter::All, None, 20),
            NOW,
        )
        .await
        .unwrap();

    let error = harness
        .service
        .publish_client_account_failure(expected, NOW)
        .await
        .unwrap_err();

    assert_eq!(error.code(), "NOTIFICATION_STATE_CONFLICT");
    assert_eq!(harness.service.revision().await, "9");
    assert_eq!(
        harness
            .service
            .list(
                context,
                request(Some("alpha"), NotificationFilter::All, None, 20),
                NOW
            )
            .await
            .unwrap(),
        before
    );
    assert!(harness.persistence.saves().is_empty());
    assert!(harness.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn semantic_dedupe_preserves_read_state_and_never_toasts_twice() {
    let harness = harness(NotificationFileV1::empty());
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let first = harness
        .service
        .publish(input(scope.clone(), "source-1", "same-semantic"), NOW)
        .await
        .unwrap();
    let id = first.notification.unwrap().id;
    harness
        .service
        .mark_read(ViewContext::account("alpha").unwrap(), &id, NOW + 1)
        .await
        .unwrap();
    let updated = harness
        .service
        .publish(input(scope, "source-2", "same-semantic"), NOW + 2)
        .await
        .unwrap()
        .notification
        .unwrap();

    assert_eq!(updated.occurrence_count, 2);
    assert_eq!(updated.read_at_ms, Some(NOW + 1));
    let events = harness.events.lock().unwrap();
    assert_eq!(events[0].change, NotificationChange::Created);
    assert!(events[0].toast_candidate.is_some());
    assert_eq!(events[2].change, NotificationChange::Updated);
    assert!(events[2].toast_candidate.is_none());
}

#[tokio::test]
async fn source_id_remains_an_absolute_no_op_after_merge_and_unrelated_creation() {
    let harness = harness(NotificationFileV1::empty());
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    harness
        .service
        .publish(input(scope.clone(), "source-1", "semantic"), NOW)
        .await
        .unwrap();
    harness
        .service
        .publish(input(scope.clone(), "source-2", "semantic"), NOW + 1)
        .await
        .unwrap();
    harness
        .service
        .publish(input(scope.clone(), "source-3", "unrelated"), NOW + 2)
        .await
        .unwrap();
    let replay = harness
        .service
        .publish(input(scope, "source-1", "semantic"), NOW + 3)
        .await
        .unwrap();

    assert!(replay.notification.is_some());
    assert!(!replay.committed);
    assert_eq!(replay.revision, "3");
    assert_eq!(harness.persistence.saves().len(), 3);
    assert_eq!(harness.events.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn creating_source_id_remains_idempotent_after_a_merge_and_restart() {
    let first = harness(NotificationFileV1::empty());
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    first
        .service
        .publish(input(scope.clone(), "source-1", "semantic"), NOW)
        .await
        .unwrap();
    first
        .service
        .publish(input(scope.clone(), "source-2", "semantic"), NOW + 1)
        .await
        .unwrap();
    let persisted = first.persistence.saves().last().unwrap().clone();
    let restarted = harness(persisted);

    let replay = restarted
        .service
        .publish(input(scope, "source-1", "semantic"), NOW + 2)
        .await
        .unwrap();

    assert!(replay.notification.is_some());
    assert!(!replay.committed);
    assert_eq!(replay.revision, "2");
    assert!(restarted.persistence.saves().is_empty());
    assert!(restarted.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn semantic_merge_later_source_id_is_an_absolute_no_op_after_restart() {
    let first = harness(NotificationFileV1::empty());
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    first
        .service
        .publish(input(scope.clone(), "source-1", "semantic"), NOW)
        .await
        .unwrap();
    first
        .service
        .publish(input(scope.clone(), "source-2", "semantic"), NOW + 1)
        .await
        .unwrap();
    let persisted = first.persistence.saves().last().unwrap().clone();
    let restarted = harness(persisted);

    let replay = restarted
        .service
        .publish(input(scope, "source-2", "semantic"), NOW + 2)
        .await
        .unwrap();

    assert!(replay.notification.is_some());
    assert!(!replay.committed);
    assert_eq!(replay.revision, "2");
    assert!(restarted.persistence.saves().is_empty());
    assert!(restarted.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn expired_semantic_record_does_not_swallow_a_new_created_toast() {
    const DAY: u64 = 24 * 60 * 60 * 1_000;
    let file = NotificationFileV1 {
        schema_version: crate::storage::notification_store::NOTIFICATION_SCHEMA_VERSION,
        revision: 4,
        source_event_index: Vec::new(),
        partitions: vec![crate::storage::notification_store::NotificationPartition {
            scope: NotificationScope::Global,
            items: vec![super::support::record(
                1,
                NotificationScope::Global,
                "old-source",
                "semantic",
                NOW - 91 * DAY,
            )],
        }],
    };
    let harness = harness(file);

    let outcome = harness
        .service
        .publish(
            input(NotificationScope::Global, "new-source", "semantic"),
            NOW,
        )
        .await
        .unwrap();

    let created = outcome.notification.unwrap();
    assert_eq!(created.created_at_ms, NOW);
    assert_eq!(created.occurrence_count, 1);
    assert_eq!(outcome.revision, "6");
    let events = harness.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].change, NotificationChange::Reset);
    assert!(events[0].toast_candidate.is_none());
    assert_eq!(events[1].change, NotificationChange::Created);
    assert!(events[1].toast_candidate.is_some());
}

#[tokio::test]
async fn save_failure_preserves_memory_revision_and_emits_nothing() {
    let harness = harness(NotificationFileV1::empty());
    harness.persistence.fail_next();
    let error = harness
        .service
        .publish(
            input(NotificationScope::Global, "source-1", "global:test"),
            NOW,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "NOTIFICATION_STORAGE_UNAVAILABLE");
    assert_eq!(harness.service.revision().await, "0");
    assert!(harness.events.lock().unwrap().is_empty());
    let page = harness
        .service
        .list(
            ViewContext::global(),
            ListNotificationsRequest {
                account_id: None,
                filter: NotificationFilter::All,
                cursor: None,
                limit: 50,
            },
            NOW,
        )
        .await
        .unwrap();
    assert!(page.items.is_empty());
}

#[tokio::test]
async fn unsafe_session_epoch_is_rejected_before_save_or_event() {
    let harness = harness(NotificationFileV1::empty());
    let mut item = input(NotificationScope::Global, "source-1", "global:test");
    item.session_epoch = Some(crate::models::notification::MAX_JAVASCRIPT_SAFE_INTEGER + 1);

    let error = harness.service.publish(item, NOW).await.unwrap_err();

    assert_eq!(error.code(), "INVALID_NOTIFICATION_CONTENT");
    assert!(harness.persistence.saves().is_empty());
    assert!(harness.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn emitter_failure_is_diagnostic_only_after_state_and_disk_commit() {
    let persistence = Arc::new(FakePersistence::default());
    let emitter: NotificationEmitter = Arc::new(|_| Err("synthetic emit failure".into()));
    let service = NotificationService::from_snapshot(
        NotificationFileV1::empty(),
        Arc::clone(&persistence),
        emitter,
        NOW,
    );

    let outcome = service
        .publish(
            input(NotificationScope::Global, "source-1", "global:test"),
            NOW,
        )
        .await
        .unwrap();

    assert!(outcome.notification.is_some());
    assert_eq!(outcome.revision, "1");
    assert_eq!(service.revision().await, "1");
    assert_eq!(persistence.saves().len(), 1);
}

#[tokio::test]
async fn mutation_save_failure_preserves_record_and_revision() {
    let harness = harness(NotificationFileV1::empty());
    let record = harness
        .service
        .publish(
            input(NotificationScope::Global, "source-1", "global:test"),
            NOW,
        )
        .await
        .unwrap()
        .notification
        .unwrap();
    harness.persistence.fail_next();

    let error = harness
        .service
        .mark_read(ViewContext::global(), &record.id, NOW + 1)
        .await
        .unwrap_err();

    assert_eq!(error.code(), "NOTIFICATION_STORAGE_UNAVAILABLE");
    assert_eq!(harness.service.revision().await, "1");
    assert_eq!(harness.events.lock().unwrap().len(), 1);
    let page = harness
        .service
        .list(
            ViewContext::global(),
            request(None, NotificationFilter::All, None, 50),
            NOW + 1,
        )
        .await
        .unwrap();
    assert!(page.items[0].read_at_ms.is_none());
}

#[tokio::test]
async fn concurrent_publishers_commit_and_emit_consecutive_revisions() {
    let harness = harness(NotificationFileV1::empty());
    let one = Arc::clone(&harness.service);
    let two = Arc::clone(&harness.service);
    let a = tokio::spawn(async move {
        one.publish(
            input(NotificationScope::Global, "source-a", "global:a"),
            NOW,
        )
        .await
        .unwrap();
    });
    let b = tokio::spawn(async move {
        two.publish(
            input(NotificationScope::Global, "source-b", "global:b"),
            NOW + 1,
        )
        .await
        .unwrap();
    });
    a.await.unwrap();
    b.await.unwrap();

    let pairs: Vec<_> = harness
        .events
        .lock()
        .unwrap()
        .iter()
        .map(|event| (event.previous_revision.clone(), event.revision.clone()))
        .collect();
    assert_eq!(
        pairs,
        vec![("0".into(), "1".into()), ("1".into(), "2".into())]
    );
}

#[tokio::test]
async fn current_account_view_merges_global_without_exposing_other_accounts() {
    let harness = harness(NotificationFileV1::empty());
    let global = harness
        .service
        .publish(input(NotificationScope::Global, "g", "g"), NOW + 1)
        .await
        .unwrap()
        .notification
        .unwrap();
    let alpha = harness
        .service
        .publish(
            input(
                NotificationScope::Account {
                    account_id: "alpha".into(),
                },
                "a",
                "a",
            ),
            NOW + 2,
        )
        .await
        .unwrap()
        .notification
        .unwrap();
    let beta = harness
        .service
        .publish(
            input(
                NotificationScope::Account {
                    account_id: "beta".into(),
                },
                "b",
                "b",
            ),
            NOW + 3,
        )
        .await
        .unwrap()
        .notification
        .unwrap();

    let page = harness
        .service
        .list(
            ViewContext::account("alpha").unwrap(),
            request(Some("alpha"), NotificationFilter::All, None, 50),
            NOW + 4,
        )
        .await
        .unwrap();
    let ids: Vec<_> = page.items.iter().map(|item| item.id.as_str()).collect();
    assert_eq!(ids, vec![alpha.id.as_str(), global.id.as_str()]);
    assert!(!ids.contains(&beta.id.as_str()));
    assert_eq!(page.unread_count, 2);

    let global_only = harness
        .service
        .list(
            ViewContext::global(),
            request(None, NotificationFilter::All, None, 50),
            NOW + 4,
        )
        .await
        .unwrap();
    assert_eq!(global_only.items[0].id, global.id);
}

#[tokio::test]
async fn same_timestamp_sort_is_created_at_then_id_descending() {
    let harness = harness(NotificationFileV1::empty());
    let one = harness
        .service
        .publish(input(NotificationScope::Global, "one", "one"), NOW)
        .await
        .unwrap()
        .notification
        .unwrap();
    let two = harness
        .service
        .publish(input(NotificationScope::Global, "two", "two"), NOW)
        .await
        .unwrap()
        .notification
        .unwrap();
    let mut expected = vec![one.id, two.id];
    expected.sort_by(|left, right| right.cmp(left));
    let page = harness
        .service
        .list(
            ViewContext::global(),
            request(None, NotificationFilter::All, None, 50),
            NOW,
        )
        .await
        .unwrap();
    assert_eq!(
        page.items
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        expected
    );
}

#[tokio::test]
async fn unread_filter_and_count_use_all_visible_records_not_only_the_page() {
    let harness = harness(NotificationFileV1::empty());
    let first = harness
        .service
        .publish(input(NotificationScope::Global, "one", "one"), NOW)
        .await
        .unwrap()
        .notification
        .unwrap();
    harness
        .service
        .publish(input(NotificationScope::Global, "two", "two"), NOW + 1)
        .await
        .unwrap();
    harness
        .service
        .publish(input(NotificationScope::Global, "three", "three"), NOW + 2)
        .await
        .unwrap();
    harness
        .service
        .mark_read(ViewContext::global(), &first.id, NOW + 3)
        .await
        .unwrap();

    let page = harness
        .service
        .list(
            ViewContext::global(),
            request(None, NotificationFilter::Unread, None, 1),
            NOW + 3,
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.unread_count, 2);
    assert!(page.items[0].read_at_ms.is_none());
    assert!(page.next_cursor.as_deref().unwrap().starts_with("n1."));
}

#[tokio::test]
async fn limit_bounds_return_the_stable_request_error() {
    let harness = harness(NotificationFileV1::empty());
    for limit in [0, 101] {
        let error = harness
            .service
            .list(
                ViewContext::global(),
                request(None, NotificationFilter::All, None, limit),
                NOW,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), "INVALID_NOTIFICATION_REQUEST");
    }
}

#[tokio::test]
async fn opaque_cursor_cannot_cross_account_or_filter() {
    let harness = harness(NotificationFileV1::empty());
    for index in 0..3 {
        harness
            .service
            .publish(
                input(
                    NotificationScope::Global,
                    &format!("source-{index}"),
                    &format!("dedupe-{index}"),
                ),
                NOW + index,
            )
            .await
            .unwrap();
    }
    let first = harness
        .service
        .list(
            ViewContext::account("alpha").unwrap(),
            request(Some("alpha"), NotificationFilter::All, None, 1),
            NOW + 3,
        )
        .await
        .unwrap();
    let cursor = first.next_cursor.unwrap();
    assert!(cursor.starts_with("n1."));

    let wrong_filter = harness
        .service
        .list(
            ViewContext::account("alpha").unwrap(),
            request(
                Some("alpha"),
                NotificationFilter::Unread,
                Some(cursor.clone()),
                1,
            ),
            NOW + 3,
        )
        .await
        .unwrap_err();
    assert_eq!(wrong_filter.code(), "INVALID_NOTIFICATION_CURSOR");

    let wrong_account = harness
        .service
        .list(
            ViewContext::account("beta").unwrap(),
            request(Some("beta"), NotificationFilter::All, Some(cursor), 1),
            NOW + 3,
        )
        .await
        .unwrap_err();
    assert_eq!(wrong_account.code(), "INVALID_NOTIFICATION_CURSOR");
}

#[tokio::test]
async fn malformed_cursor_returns_the_stable_cursor_error() {
    let harness = harness(NotificationFileV1::empty());
    for malformed in ["n2.00", "n1.0", "n1.zz", "n1.7b7d", "n1.aéb"] {
        let error = harness
            .service
            .list(
                ViewContext::global(),
                request(
                    None,
                    NotificationFilter::All,
                    Some(malformed.to_string()),
                    50,
                ),
                NOW,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), "INVALID_NOTIFICATION_CURSOR");
    }
}

#[tokio::test]
async fn keyset_cursor_is_stable_when_a_new_item_is_inserted_between_pages() {
    let harness = harness(NotificationFileV1::empty());
    let mut original = Vec::new();
    for index in 0..4 {
        original.push(
            harness
                .service
                .publish(
                    input(
                        NotificationScope::Global,
                        &format!("source-{index}"),
                        &format!("dedupe-{index}"),
                    ),
                    NOW + index,
                )
                .await
                .unwrap()
                .notification
                .unwrap()
                .id,
        );
    }
    let first = harness
        .service
        .list(
            ViewContext::global(),
            request(None, NotificationFilter::All, None, 2),
            NOW + 4,
        )
        .await
        .unwrap();
    let first_ids: Vec<_> = first.items.iter().map(|item| item.id.clone()).collect();
    harness
        .service
        .publish(input(NotificationScope::Global, "new", "new"), NOW + 10)
        .await
        .unwrap();
    let second = harness
        .service
        .list(
            ViewContext::global(),
            request(None, NotificationFilter::All, first.next_cursor, 2),
            NOW + 10,
        )
        .await
        .unwrap();
    let second_ids: Vec<_> = second.items.into_iter().map(|item| item.id).collect();
    assert!(first_ids.iter().all(|id| !second_ids.contains(id)));
    assert_eq!(second_ids, vec![original[1].clone(), original[0].clone()]);
}

#[tokio::test]
async fn visible_mutations_enforce_scope_and_global_read_state_is_shared() {
    let harness = harness(NotificationFileV1::empty());
    let global = harness
        .service
        .publish(input(NotificationScope::Global, "global", "global"), NOW)
        .await
        .unwrap()
        .notification
        .unwrap();
    let beta = harness
        .service
        .publish(
            input(
                NotificationScope::Account {
                    account_id: "beta".into(),
                },
                "beta",
                "beta",
            ),
            NOW + 1,
        )
        .await
        .unwrap()
        .notification
        .unwrap();
    let mismatch = harness
        .service
        .delete_visible(ViewContext::account("alpha").unwrap(), &beta.id, NOW + 2)
        .await
        .unwrap_err();
    assert_eq!(mismatch.code(), "NOTIFICATION_SCOPE_MISMATCH");

    harness
        .service
        .mark_read(ViewContext::account("alpha").unwrap(), &global.id, NOW + 2)
        .await
        .unwrap();
    let beta_summary = harness
        .service
        .summary(ViewContext::account("beta").unwrap(), NOW + 2)
        .await
        .unwrap();
    assert_eq!(beta_summary.unread_count, 1);
}

#[tokio::test]
async fn mark_visible_and_clear_account_mutate_only_the_requested_view_partition() {
    let harness = harness(NotificationFileV1::empty());
    for (source, scope) in [
        ("global", NotificationScope::Global),
        (
            "alpha",
            NotificationScope::Account {
                account_id: "alpha".into(),
            },
        ),
        (
            "beta",
            NotificationScope::Account {
                account_id: "beta".into(),
            },
        ),
    ] {
        harness
            .service
            .publish(input(scope, source, source), NOW)
            .await
            .unwrap();
    }
    let marked = harness
        .service
        .mark_visible_read(ViewContext::account("alpha").unwrap(), NOW + 1)
        .await
        .unwrap();
    assert_eq!(marked.affected_count, 2);
    assert_eq!(marked.unread_count, 0);

    let mismatch = harness
        .service
        .clear_account("alpha", "beta", NOW + 2)
        .await
        .unwrap_err();
    assert_eq!(mismatch.code(), "NOTIFICATION_SCOPE_MISMATCH");
    let cleared = harness
        .service
        .clear_account("alpha", "alpha", NOW + 2)
        .await
        .unwrap();
    assert_eq!(cleared.affected_count, 1);
    let global = harness
        .service
        .list(
            ViewContext::global(),
            request(None, NotificationFilter::All, None, 50),
            NOW + 2,
        )
        .await
        .unwrap();
    assert_eq!(global.items.len(), 1);
}

#[tokio::test]
async fn delete_and_clear_remove_only_their_records_source_event_index_entries() {
    let harness = harness(NotificationFileV1::empty());
    let global = harness
        .service
        .publish(
            input(NotificationScope::Global, "global-source", "global"),
            NOW,
        )
        .await
        .unwrap()
        .notification
        .unwrap();
    let alpha = harness
        .service
        .publish(
            input(
                NotificationScope::Account {
                    account_id: "alpha".into(),
                },
                "alpha-source",
                "alpha",
            ),
            NOW,
        )
        .await
        .unwrap()
        .notification
        .unwrap();

    harness
        .service
        .delete_visible(ViewContext::global(), &global.id, NOW + 1)
        .await
        .unwrap();
    let after_delete = harness.persistence.saves().last().unwrap().clone();
    assert_eq!(after_delete.source_event_index.len(), 1);
    assert_eq!(
        after_delete.source_event_index[0].source_event_id,
        "alpha-source"
    );

    harness
        .service
        .clear_account("alpha", "alpha", NOW + 2)
        .await
        .unwrap();
    assert!(harness
        .persistence
        .saves()
        .last()
        .unwrap()
        .source_event_index
        .is_empty());

    let recreated_global = harness
        .service
        .publish(
            input(NotificationScope::Global, "global-source", "global"),
            NOW + 3,
        )
        .await
        .unwrap();
    let recreated_alpha = harness
        .service
        .publish(
            input(
                NotificationScope::Account {
                    account_id: "alpha".into(),
                },
                "alpha-source",
                "alpha",
            ),
            NOW + 4,
        )
        .await
        .unwrap();
    assert!(recreated_global.committed);
    assert!(recreated_alpha.committed);
    assert_ne!(recreated_global.notification.unwrap().id, global.id);
    assert_ne!(recreated_alpha.notification.unwrap().id, alpha.id);
}
