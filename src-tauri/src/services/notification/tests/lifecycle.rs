use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::models::notification::{
    ListNotificationsRequest, NotificationAction, NotificationCategory, NotificationChange,
    NotificationChannel, NotificationContent, NotificationEntity, NotificationEntityType,
    NotificationEnvironment, NotificationFilter, NotificationKind, NotificationScalar,
    NotificationScope, NotificationSeverity,
};
use crate::services::notification::{
    enforce_serialized_file_cap_for_test, AvailabilityState, ConnectionObservation,
    EnvironmentObservation, NotificationEmitter, NotificationService, ObservedOrderStatus,
    OrderObservation, OrderObservationOrigin, ViewContext,
};
use crate::storage::notification_store::{
    notification_file_fits_serialized_limit, NotificationFileV1, NotificationPartition,
    NotificationSourceEventIndexEntry, NotificationStore, MAX_NOTIFICATION_FILE_BYTES,
    MAX_NOTIFICATION_SOURCE_INDEX_PER_SCOPE, NOTIFICATION_SCHEMA_VERSION,
};

use super::support::{harness, input, record};

const DAY: u64 = 24 * 60 * 60 * 1_000;
const NOW: u64 = 1_700_000_000_000;
static TEST_ROOT_ID: AtomicU64 = AtomicU64::new(0);

fn request() -> ListNotificationsRequest {
    ListNotificationsRequest {
        account_id: None,
        filter: NotificationFilter::All,
        cursor: None,
        limit: 100,
    }
}

fn near_file_cap_before_created_publish(scope: NotificationScope) -> (NotificationFileV1, String) {
    near_file_cap_before_created_publish_at(scope, 70, 1)
}

fn near_file_cap_before_created_publish_at(
    scope: NotificationScope,
    revision: u64,
    prospective_overage: usize,
) -> (NotificationFileV1, String) {
    let mut carrier = record(1, scope.clone(), "carrier-source", "carrier-dedupe", NOW);
    carrier.entity = Some(NotificationEntity {
        entity_type: NotificationEntityType::Order,
        id: "x".into(),
    });
    let carrier_id = carrier.id.clone();
    let mut file = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision,
        source_event_index: vec![NotificationSourceEventIndexEntry {
            scope: scope.clone(),
            source_event_id: "carrier-source".into(),
            notification_id: carrier_id.clone(),
        }],
        partitions: vec![NotificationPartition {
            scope: scope.clone(),
            items: vec![carrier],
        }],
    };
    let pending = record(
        2,
        scope.clone(),
        "pending-source",
        "pending-dedupe",
        NOW + 1,
    );
    let mut prospective = file.clone();
    prospective.partitions[0].items.push(pending.clone());
    prospective
        .source_event_index
        .push(NotificationSourceEventIndexEntry {
            scope,
            source_event_id: "pending-source".into(),
            notification_id: pending.id,
        });
    let current_bytes = serde_json::to_vec_pretty(&file).unwrap().len();
    let publish_bytes = serde_json::to_vec_pretty(&prospective).unwrap().len() - current_bytes;
    let target_bytes = MAX_NOTIFICATION_FILE_BYTES - publish_bytes + prospective_overage;
    let padding = target_bytes - current_bytes;
    file.partitions[0].items[0]
        .entity
        .as_mut()
        .unwrap()
        .id
        .push_str(&"x".repeat(padding));
    prospective.partitions[0].items[0].entity = file.partitions[0].items[0].entity.clone();
    assert_eq!(
        serde_json::to_vec_pretty(&file).unwrap().len(),
        target_bytes
    );
    assert_eq!(
        serde_json::to_vec_pretty(&prospective).unwrap().len(),
        MAX_NOTIFICATION_FILE_BYTES + prospective_overage
    );
    (file, carrier_id)
}

fn near_file_cap_before_semantic_publish(
    scope: NotificationScope,
) -> (NotificationFileV1, String, String) {
    let mut carrier = record(
        1,
        scope.clone(),
        "carrier-source",
        "carrier-dedupe",
        NOW + 1,
    );
    carrier.entity = Some(NotificationEntity {
        entity_type: NotificationEntityType::Order,
        id: "x".into(),
    });
    let carrier_id = carrier.id.clone();
    let mut target = record(2, scope.clone(), "target-source", "semantic-target", NOW);
    target.read_at_ms = Some(NOW + 1);
    let target_id = target.id.clone();
    let mut file = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 80,
        source_event_index: vec![
            NotificationSourceEventIndexEntry {
                scope: scope.clone(),
                source_event_id: "carrier-source".into(),
                notification_id: carrier_id.clone(),
            },
            NotificationSourceEventIndexEntry {
                scope: scope.clone(),
                source_event_id: "target-source".into(),
                notification_id: target_id.clone(),
            },
        ],
        partitions: vec![NotificationPartition {
            scope: scope.clone(),
            items: vec![carrier, target],
        }],
    };
    let mut prospective = file.clone();
    let prospective_target = &mut prospective.partitions[0].items[1];
    prospective_target.occurrence_count += 1;
    prospective_target.updated_at_ms = NOW + 2;
    prospective
        .source_event_index
        .push(NotificationSourceEventIndexEntry {
            scope,
            source_event_id: "semantic-new-source".into(),
            notification_id: target_id.clone(),
        });
    let current_bytes = serde_json::to_vec_pretty(&file).unwrap().len();
    let publish_bytes = serde_json::to_vec_pretty(&prospective).unwrap().len() - current_bytes;
    let target_bytes = MAX_NOTIFICATION_FILE_BYTES - publish_bytes + 1;
    let padding = target_bytes - current_bytes;
    file.partitions[0].items[0]
        .entity
        .as_mut()
        .unwrap()
        .id
        .push_str(&"x".repeat(padding));
    prospective.partitions[0].items[0].entity = file.partitions[0].items[0].entity.clone();
    assert_eq!(
        serde_json::to_vec_pretty(&file).unwrap().len(),
        target_bytes
    );
    assert_eq!(
        serde_json::to_vec_pretty(&prospective).unwrap().len(),
        MAX_NOTIFICATION_FILE_BYTES + 1
    );
    (file, carrier_id, target_id)
}

#[tokio::test]
async fn query_filters_expired_records_before_persistent_maintenance_runs() {
    let file = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 9,
        source_event_index: Vec::new(),
        partitions: vec![NotificationPartition {
            scope: NotificationScope::Global,
            items: vec![
                record(
                    1,
                    NotificationScope::Global,
                    "expired",
                    "expired",
                    NOW - 91 * DAY,
                ),
                record(
                    2,
                    NotificationScope::Global,
                    "fresh",
                    "fresh",
                    NOW - 89 * DAY,
                ),
            ],
        }],
    };
    let harness = harness(file);
    let page = harness
        .service
        .list(ViewContext::global(), request(), NOW)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].source_event_id.as_deref(), Some("fresh"));
    assert_eq!(page.unread_count, 1);
    assert!(harness.persistence.saves().is_empty());
}

#[tokio::test]
async fn prune_removes_expired_and_orphan_account_records_in_one_reset() {
    let alpha = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let orphan = NotificationScope::Account {
        account_id: "orphan".into(),
    };
    let file = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 4,
        source_event_index: Vec::new(),
        partitions: vec![
            NotificationPartition {
                scope: NotificationScope::Global,
                items: vec![record(1, NotificationScope::Global, "g", "g", NOW)],
            },
            NotificationPartition {
                scope: alpha.clone(),
                items: vec![
                    record(2, alpha.clone(), "old", "old", NOW - 91 * DAY),
                    record(3, alpha.clone(), "fresh", "fresh", NOW),
                ],
            },
            NotificationPartition {
                scope: orphan.clone(),
                items: vec![record(4, orphan, "orphan", "orphan", NOW)],
            },
        ],
    };
    let harness = harness(file);
    let outcome = harness
        .service
        .prune(&HashSet::from(["alpha".to_string()]), NOW)
        .await
        .unwrap();
    assert_eq!(outcome.affected_count, 2);
    assert_eq!(outcome.revision, "5");
    assert_eq!(harness.persistence.saves().len(), 1);
    let saved = harness.persistence.saves().last().unwrap().clone();
    assert_eq!(saved.source_event_index.len(), 2);
    assert!(saved
        .source_event_index
        .iter()
        .all(|entry| entry.source_event_id != "old" && entry.source_event_id != "orphan"));
    let events = harness.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].change, NotificationChange::Reset);
    assert_eq!(events[0].previous_revision, "4");
    assert_eq!(events[0].revision, "5");
}

#[tokio::test]
async fn startup_prunes_orphan_accounts_and_persists_one_reset() {
    let root = std::env::temp_dir().join(format!(
        "easiflux-notification-service-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let path = root.join("notifications.v1.json");
    let store = NotificationStore::with_path(path.clone());
    let orphan_scope = NotificationScope::Account {
        account_id: "orphan".into(),
    };
    store
        .save(&NotificationFileV1 {
            schema_version: NOTIFICATION_SCHEMA_VERSION,
            revision: 6,
            source_event_index: Vec::new(),
            partitions: vec![NotificationPartition {
                scope: orphan_scope.clone(),
                items: vec![record(1, orphan_scope, "orphan", "orphan", NOW)],
            }],
        })
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let emitter: NotificationEmitter = Arc::new(move |event| {
        sink.lock().unwrap().push(event.clone());
        Ok(())
    });

    let service = NotificationService::load(store, &["alpha".to_string()], NOW, emitter).unwrap();

    assert_eq!(service.revision().await, "7");
    let emitted = events.lock().unwrap();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].change, NotificationChange::Reset);
    drop(emitted);
    drop(service);
    let persisted = NotificationStore::with_path(path).load().unwrap().file;
    assert!(persisted.partitions.is_empty());
    assert!(persisted.source_event_index.is_empty());
    assert_eq!(persisted.revision, 7);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn startup_legal_backfill_commits_reset_immediately_and_is_restart_stable() {
    let root = std::env::temp_dir().join(format!(
        "easiflux-notification-service-backfill-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let path = root.join("notifications.v1.json");
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let legacy = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 12,
        source_event_index: Vec::new(),
        partitions: vec![NotificationPartition {
            scope: scope.clone(),
            items: vec![record(
                1,
                scope.clone(),
                "legacy-source",
                "legacy-dedupe",
                NOW,
            )],
        }],
    };
    NotificationStore::with_path(path.clone())
        .save(&legacy)
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let emitter: NotificationEmitter = Arc::new(move |event| {
        sink.lock().unwrap().push(event.clone());
        Ok(())
    });

    let service = NotificationService::load(
        NotificationStore::with_path(path.clone()),
        &["alpha".into()],
        NOW,
        emitter,
    )
    .unwrap();

    assert_eq!(service.revision().await, "13");
    let emitted = events.lock().unwrap();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].change, NotificationChange::Reset);
    assert_eq!(emitted[0].previous_revision, "12");
    assert_eq!(emitted[0].revision, "13");
    assert_eq!(emitted[0].affected_scopes, vec![scope.clone()]);
    drop(emitted);
    drop(service);

    let persisted = NotificationStore::with_path(path.clone())
        .load()
        .unwrap()
        .file;
    assert_eq!(persisted.revision, 13);
    assert_eq!(persisted.source_event_index.len(), 1);
    assert_eq!(
        persisted.source_event_index[0].source_event_id,
        "legacy-source"
    );

    let restart_events = Arc::new(Mutex::new(Vec::new()));
    let restart_sink = Arc::clone(&restart_events);
    let restart_emitter: NotificationEmitter = Arc::new(move |event| {
        restart_sink.lock().unwrap().push(event.clone());
        Ok(())
    });
    let restarted = NotificationService::load(
        NotificationStore::with_path(path.clone()),
        &["alpha".into()],
        NOW + 1,
        restart_emitter,
    )
    .unwrap();
    assert_eq!(restarted.revision().await, "13");
    assert!(restart_events.lock().unwrap().is_empty());
    let duplicate = restarted
        .publish(input(scope, "legacy-source", "different-dedupe"), NOW + 1)
        .await
        .unwrap();
    assert!(duplicate.notification.is_none());
    assert_eq!(duplicate.revision, "13");
    assert!(restart_events.lock().unwrap().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn startup_backfill_save_failure_does_not_construct_service_or_change_disk() {
    let root = std::env::temp_dir().join(format!(
        "easiflux-notification-service-backfill-failure-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let path = root.join("notifications.v1.json");
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let legacy = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 20,
        source_event_index: Vec::new(),
        partitions: vec![NotificationPartition {
            scope: scope.clone(),
            items: vec![record(1, scope, "legacy-source", "legacy-dedupe", NOW)],
        }],
    };
    NotificationStore::with_path(path.clone())
        .save(&legacy)
        .unwrap();
    let baseline = fs::read(&path).unwrap();
    let temp_path = NotificationStore::temp_path_for_test(&path);
    fs::create_dir(&temp_path).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let emitter: NotificationEmitter = Arc::new(move |event| {
        sink.lock().unwrap().push(event.clone());
        Ok(())
    });

    let error = NotificationService::load(
        NotificationStore::with_path(path.clone()),
        &["alpha".into()],
        NOW,
        emitter,
    )
    .err()
    .expect("startup normalization save must fail");

    assert_eq!(error.code(), "NOTIFICATION_STORAGE_UNAVAILABLE");
    assert_eq!(fs::read(&path).unwrap(), baseline);
    assert!(events.lock().unwrap().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn startup_backfill_reserves_reset_revision_digit_growth() {
    let root = std::env::temp_dir().join(format!(
        "easiflux-notification-service-backfill-revision-growth-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let path = root.join("notifications.v1.json");
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let mut item = record(1, scope.clone(), "legacy-source", "legacy-dedupe", NOW);
    item.category = NotificationCategory::ConnectionSystem;
    item.kind = NotificationKind::ConnectionUnavailable;
    item.severity = NotificationSeverity::Warning;
    item.content = NotificationContent::new(
        "connection.unavailable",
        [("channel", NotificationScalar::String("api".into()))],
        "连接不可用",
        "交易连接暂时不可用，请检查网络或稍后重试。",
    )
    .unwrap();
    item.action = Some(NotificationAction::OpenGeneralSettings);
    item.entity = Some(NotificationEntity {
        entity_type: NotificationEntityType::Order,
        id: "x".into(),
    });
    let notification_id = item.id.clone();
    let mut legacy = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 9,
        source_event_index: Vec::new(),
        partitions: vec![NotificationPartition {
            scope: scope.clone(),
            items: vec![item],
        }],
    };
    let mut normalized = legacy.clone();
    normalized
        .source_event_index
        .push(NotificationSourceEventIndexEntry {
            scope: scope.clone(),
            source_event_id: "legacy-source".into(),
            notification_id,
        });
    let legacy_bytes = serde_json::to_vec_pretty(&legacy).unwrap().len();
    let normalization_bytes = serde_json::to_vec_pretty(&normalized).unwrap().len() - legacy_bytes;
    let padding = MAX_NOTIFICATION_FILE_BYTES - normalization_bytes - legacy_bytes;
    legacy.partitions[0].items[0]
        .entity
        .as_mut()
        .unwrap()
        .id
        .push_str(&"x".repeat(padding));
    normalized.partitions[0].items[0].entity = legacy.partitions[0].items[0].entity.clone();
    assert_eq!(
        serde_json::to_vec_pretty(&normalized).unwrap().len(),
        MAX_NOTIFICATION_FILE_BYTES
    );
    NotificationStore::with_path(path.clone())
        .save(&legacy)
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let emitter: NotificationEmitter = Arc::new(move |event| {
        sink.lock().unwrap().push(event.clone());
        Ok(())
    });

    let service = NotificationService::load(
        NotificationStore::with_path(path.clone()),
        &["alpha".into()],
        NOW,
        emitter,
    )
    .unwrap();

    assert_eq!(service.revision().await, "10");
    let emitted = events.lock().unwrap();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].change, NotificationChange::Reset);
    assert_eq!(emitted[0].previous_revision, "9");
    assert_eq!(emitted[0].revision, "10");
    drop(emitted);
    drop(service);
    let persisted = NotificationStore::with_path(path.clone())
        .load()
        .unwrap()
        .file;
    assert!(notification_file_fits_serialized_limit(&persisted));
    assert!(persisted.partitions.is_empty());
    assert!(persisted.source_event_index.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn startup_backfill_at_source_cap_evicts_a_record_and_does_not_poison_later_saves() {
    let root = std::env::temp_dir().join(format!(
        "easiflux-notification-service-cap-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let path = root.join("notifications.v1.json");
    let store = NotificationStore::with_path(path.clone());
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let oldest = record(
        1,
        scope.clone(),
        "creating-oldest",
        "dedupe-oldest",
        NOW - 1,
    );
    let newest = record(2, scope.clone(), "creating-newest", "dedupe-newest", NOW);
    let oldest_id = oldest.id.clone();
    let newest_id = newest.id.clone();
    let source_event_index = (0..MAX_NOTIFICATION_SOURCE_INDEX_PER_SCOPE)
        .map(|index| NotificationSourceEventIndexEntry {
            scope: scope.clone(),
            source_event_id: if index == 0 {
                "creating-oldest".into()
            } else {
                format!("semantic-oldest-{index}")
            },
            notification_id: oldest_id.clone(),
        })
        .collect();
    store
        .save(&NotificationFileV1 {
            schema_version: NOTIFICATION_SCHEMA_VERSION,
            revision: 5,
            source_event_index,
            partitions: vec![NotificationPartition {
                scope: scope.clone(),
                items: vec![oldest, newest],
            }],
        })
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let emitter: NotificationEmitter = Arc::new(move |event| {
        sink.lock().unwrap().push(event.clone());
        Ok(())
    });

    let service = NotificationService::load(store, &["alpha".to_string()], NOW, emitter).unwrap();

    assert_eq!(service.revision().await, "6");
    let emitted = events.lock().unwrap();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].change, NotificationChange::Reset);
    assert_eq!(emitted[0].previous_revision, "5");
    assert_eq!(emitted[0].revision, "6");
    drop(emitted);
    let mut account_request = request();
    account_request.account_id = Some("alpha".into());
    let page = service
        .list(ViewContext::account("alpha").unwrap(), account_request, NOW)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, newest_id);
    service
        .mark_visible_read(ViewContext::account("alpha").unwrap(), NOW + 1)
        .await
        .unwrap();
    drop(service);
    let persisted = NotificationStore::with_path(path).load().unwrap().file;
    assert_eq!(persisted.revision, 7);
    assert_eq!(persisted.source_event_index.len(), 1);
    assert_eq!(
        persisted.source_event_index[0].source_event_id,
        "creating-newest"
    );
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn startup_backfill_near_file_cap_evicts_oldest_record_before_later_saves() {
    let root = std::env::temp_dir().join(format!(
        "easiflux-notification-service-byte-cap-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let path = root.join("notifications.v1.json");
    let store = NotificationStore::with_path(path.clone());
    let mut partitions = Vec::new();
    let mut source_event_index = Vec::new();
    for scope_index in 0..5_u128 {
        let scope = NotificationScope::Account {
            account_id: format!("scope-{scope_index}"),
        };
        let creating_source = format!("creating-{scope_index}");
        let item = record(
            scope_index + 1,
            scope.clone(),
            &creating_source,
            &format!("dedupe-{scope_index}"),
            NOW - 10 + scope_index as u64,
        );
        let notification_id = item.id.clone();
        partitions.push(NotificationPartition {
            scope: scope.clone(),
            items: vec![item],
        });
        source_event_index.extend((0..9_999).map(|entry_index| {
            NotificationSourceEventIndexEntry {
                scope: scope.clone(),
                source_event_id: if entry_index == 0 {
                    creating_source.clone()
                } else {
                    format!("source-{scope_index}-{entry_index}")
                },
                notification_id: notification_id.clone(),
            }
        }));
    }
    let newest_scope = NotificationScope::Account {
        account_id: "scope-new".into(),
    };
    let newest = record(
        6,
        newest_scope.clone(),
        "creating-newest",
        "dedupe-newest",
        NOW,
    );
    let newest_id = newest.id.clone();
    partitions.push(NotificationPartition {
        scope: newest_scope.clone(),
        items: vec![newest],
    });
    let mut file = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 5,
        source_event_index,
        partitions,
    };
    let mut with_backfill = file.clone();
    with_backfill
        .source_event_index
        .push(NotificationSourceEventIndexEntry {
            scope: newest_scope.clone(),
            source_event_id: "creating-newest".into(),
            notification_id: newest_id,
        });
    let backfill_bytes = serde_json::to_vec_pretty(&with_backfill).unwrap().len()
        - serde_json::to_vec_pretty(&file).unwrap().len();
    let target_bytes = MAX_NOTIFICATION_FILE_BYTES - backfill_bytes + 1;
    let current_bytes = serde_json::to_vec_pretty(&file).unwrap().len();
    let mut padding_remaining = target_bytes - current_bytes;
    for entry in &mut file.source_event_index {
        if entry.source_event_id.starts_with("creating-") {
            continue;
        }
        let padding = padding_remaining.min(256 - entry.source_event_id.len());
        entry.source_event_id.push_str(&"x".repeat(padding));
        padding_remaining -= padding;
        if padding_remaining == 0 {
            break;
        }
    }
    assert_eq!(padding_remaining, 0);
    assert_eq!(
        serde_json::to_vec_pretty(&file).unwrap().len(),
        target_bytes
    );
    store.save(&file).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let emitter: NotificationEmitter = Arc::new(move |event| {
        sink.lock().unwrap().push(event.clone());
        Ok(())
    });
    let configured: Vec<_> = (0..5)
        .map(|index| format!("scope-{index}"))
        .chain(std::iter::once("scope-new".into()))
        .collect();

    let service = NotificationService::load(store, &configured, NOW, emitter).unwrap();

    assert_eq!(service.revision().await, "6");
    let emitted = events.lock().unwrap();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].change, NotificationChange::Reset);
    drop(emitted);
    service
        .mark_visible_read(ViewContext::account("scope-new").unwrap(), NOW + 1)
        .await
        .unwrap();
    drop(service);
    let persisted = NotificationStore::with_path(path).load().unwrap().file;
    assert_eq!(persisted.revision, 7);
    assert!(!persisted.partitions.iter().any(|partition| {
        partition.scope
            == (NotificationScope::Account {
                account_id: "scope-0".into(),
            })
    }));
    assert!(persisted
        .partitions
        .iter()
        .any(|partition| partition.scope == newest_scope));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn serialized_cap_repair_uses_logarithmic_probes_when_many_records_must_be_evicted() {
    let mut partitions = Vec::new();
    let mut source_event_index = Vec::new();
    for scope_index in 0..5_u128 {
        let scope = NotificationScope::Account {
            account_id: format!("scope-{scope_index}"),
        };
        let mut items = Vec::new();
        for record_index in 0..20_u128 {
            let number = scope_index * 20 + record_index + 1;
            let mut item = record(
                number,
                scope.clone(),
                "unindexed",
                &format!("dedupe-{scope_index}-{record_index}"),
                NOW + number as u64,
            );
            item.source_event_id = None;
            let notification_id = item.id.clone();
            items.push(item);
            for entry_index in 0..500_u128 {
                let prefix = format!("s{scope_index}-{record_index}-{entry_index}-");
                let source_event_id = format!("{prefix}{}", "x".repeat(256 - prefix.len()));
                source_event_index.push(NotificationSourceEventIndexEntry {
                    scope: scope.clone(),
                    source_event_id,
                    notification_id: notification_id.clone(),
                });
            }
        }
        partitions.push(NotificationPartition { scope, items });
    }
    let mut file = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 9,
        source_event_index,
        partitions,
    };
    assert!(!notification_file_fits_serialized_limit(&file));
    let before_records: usize = file
        .partitions
        .iter()
        .map(|partition| partition.items.len())
        .sum();

    let probes = enforce_serialized_file_cap_for_test(&mut file);

    let after_records: usize = file
        .partitions
        .iter()
        .map(|partition| partition.items.len())
        .sum();
    assert!(before_records - after_records > 16);
    assert!(probes <= 16, "serialized cap repair used {probes} probes");
    assert!(notification_file_fits_serialized_limit(&file));
}

#[tokio::test]
async fn prune_defensively_converges_an_oversized_runtime_snapshot() {
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let mut oversized = record(
        1,
        scope.clone(),
        "oversized-source",
        "oversized-dedupe",
        NOW,
    );
    oversized.entity = Some(NotificationEntity {
        entity_type: NotificationEntityType::Order,
        id: "x".repeat(MAX_NOTIFICATION_FILE_BYTES),
    });
    let oversized_id = oversized.id.clone();
    let file = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 9,
        source_event_index: vec![NotificationSourceEventIndexEntry {
            scope: scope.clone(),
            source_event_id: "oversized-source".into(),
            notification_id: oversized_id,
        }],
        partitions: vec![NotificationPartition {
            scope: scope.clone(),
            items: vec![oversized],
        }],
    };
    assert!(!notification_file_fits_serialized_limit(&file));
    let harness = harness(NotificationFileV1::empty());
    harness.service.state.lock().await.file = file;

    let outcome = harness
        .service
        .prune(&HashSet::from(["alpha".into()]), NOW + 1)
        .await
        .unwrap();

    assert_eq!(outcome.affected_count, 1);
    assert_eq!(outcome.affected_scopes, vec![scope]);
    assert_eq!(outcome.revision, "10");
    let saves = harness.persistence.saves();
    assert_eq!(saves.len(), 1);
    assert!(notification_file_fits_serialized_limit(&saves[0]));
    assert!(saves[0].partitions.is_empty());
    assert!(saves[0].source_event_index.is_empty());
    let events = harness.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].change, NotificationChange::Reset);
    assert_eq!(events[0].previous_revision, "9");
    assert_eq!(events[0].revision, "10");
}

#[tokio::test]
async fn publishing_enforces_oldest_first_one_thousand_limit_for_account_and_global() {
    for scope in [
        NotificationScope::Global,
        NotificationScope::Account {
            account_id: "alpha".into(),
        },
    ] {
        let items = (0..1_000)
            .map(|index| {
                record(
                    index + 1,
                    scope.clone(),
                    &format!("source-{index}"),
                    &format!("dedupe-{index}"),
                    NOW + 10_000 + index as u64,
                )
            })
            .collect();
        let file = NotificationFileV1 {
            schema_version: NOTIFICATION_SCHEMA_VERSION,
            revision: 1,
            source_event_index: Vec::new(),
            partitions: vec![NotificationPartition {
                scope: scope.clone(),
                items,
            }],
        };
        let harness = harness(file);
        harness
            .service
            .publish(input(scope.clone(), "new", "new"), NOW)
            .await
            .unwrap();
        let saved = harness.persistence.saves();
        assert_eq!(saved.len(), 2);
        let partition = &saved.last().unwrap().partitions[0];
        assert_eq!(partition.items.len(), 1_000);
        assert_eq!(saved.last().unwrap().source_event_index.len(), 1_000);
        assert!(partition
            .items
            .iter()
            .any(|item| item.source_event_id.as_deref() == Some("new")));
        assert!(!partition
            .items
            .iter()
            .any(|item| item.source_event_id.as_deref() == Some("source-0")));
        let events = harness.events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].change, NotificationChange::Reset);
        assert_eq!(events[0].previous_revision, "1");
        assert_eq!(events[0].revision, "2");
        assert!(events[0].toast_candidate.is_none());
        assert_eq!(events[1].change, NotificationChange::Created);
        assert_eq!(events[1].previous_revision, "2");
        assert_eq!(events[1].revision, "3");
        assert!(events[1].toast_candidate.is_some());
    }
}

#[tokio::test]
async fn publish_commits_expiry_housekeeping_reset_before_created() {
    let file = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 8,
        source_event_index: Vec::new(),
        partitions: vec![NotificationPartition {
            scope: NotificationScope::Global,
            items: vec![record(
                1,
                NotificationScope::Global,
                "expired",
                "expired",
                NOW - 91 * DAY,
            )],
        }],
    };
    let harness = harness(file);

    let outcome = harness
        .service
        .publish(input(NotificationScope::Global, "fresh", "fresh"), NOW)
        .await
        .unwrap();

    assert_eq!(outcome.revision, "10");
    assert_eq!(harness.persistence.saves().len(), 2);
    let events = harness.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].change, NotificationChange::Reset);
    assert_eq!(events[0].previous_revision, "8");
    assert_eq!(events[0].revision, "9");
    assert!(events[0].toast_candidate.is_none());
    assert_eq!(events[1].change, NotificationChange::Created);
    assert_eq!(events[1].previous_revision, "9");
    assert_eq!(events[1].revision, "10");
    assert!(events[1].toast_candidate.is_some());
}

#[tokio::test]
async fn identical_source_is_a_noop_before_expiry_housekeeping() {
    let file = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 8,
        source_event_index: Vec::new(),
        partitions: vec![NotificationPartition {
            scope: NotificationScope::Global,
            items: vec![
                record(
                    1,
                    NotificationScope::Global,
                    "expired",
                    "expired",
                    NOW - 91 * DAY,
                ),
                record(
                    2,
                    NotificationScope::Global,
                    "already-seen",
                    "retained",
                    NOW,
                ),
            ],
        }],
    };
    let harness = harness(file);

    let outcome = harness
        .service
        .publish(
            input(NotificationScope::Global, "already-seen", "different"),
            NOW,
        )
        .await
        .unwrap();

    assert!(outcome.notification.is_none());
    assert_eq!(outcome.revision, "8");
    assert!(harness.persistence.saves().is_empty());
    assert!(harness.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn housekeeping_and_publish_failures_have_separate_copy_on_write_boundaries() {
    let seeded = || NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 3,
        source_event_index: Vec::new(),
        partitions: vec![NotificationPartition {
            scope: NotificationScope::Global,
            items: vec![record(
                1,
                NotificationScope::Global,
                "expired",
                "expired",
                NOW - 91 * DAY,
            )],
        }],
    };

    let housekeeping_failure = harness(seeded());
    housekeeping_failure.persistence.fail_next();
    let error = housekeeping_failure
        .service
        .publish(input(NotificationScope::Global, "fresh", "fresh"), NOW)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "NOTIFICATION_STORAGE_UNAVAILABLE");
    assert_eq!(housekeeping_failure.service.revision().await, "3");
    assert!(housekeeping_failure.persistence.saves().is_empty());
    assert!(housekeeping_failure.events.lock().unwrap().is_empty());
    assert_eq!(
        housekeeping_failure
            .service
            .publish(input(NotificationScope::Global, "fresh", "fresh"), NOW)
            .await
            .unwrap()
            .revision,
        "5"
    );

    let publish_failure = harness(seeded());
    publish_failure.persistence.fail_after_successes(1);
    let error = publish_failure
        .service
        .publish(input(NotificationScope::Global, "fresh", "fresh"), NOW)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "NOTIFICATION_STORAGE_UNAVAILABLE");
    assert_eq!(publish_failure.service.revision().await, "4");
    assert_eq!(publish_failure.persistence.saves().len(), 1);
    {
        let events = publish_failure.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].change, NotificationChange::Reset);
        assert_eq!(events[0].revision, "4");
    }
    let page = publish_failure
        .service
        .list(ViewContext::global(), request(), NOW)
        .await
        .unwrap();
    assert!(page.items.is_empty());
    assert_eq!(
        publish_failure
            .service
            .publish(input(NotificationScope::Global, "fresh", "fresh"), NOW)
            .await
            .unwrap()
            .revision,
        "5"
    );
}

#[tokio::test]
async fn serialized_reservation_evicts_before_created_and_is_restart_idempotent() {
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let (file, carrier_id) = near_file_cap_before_created_publish(scope.clone());
    let first = harness(file);

    let outcome = first
        .service
        .publish(
            input(scope.clone(), "pending-source", "pending-dedupe"),
            NOW + 1,
        )
        .await
        .unwrap();

    assert_eq!(outcome.revision, "72");
    let created_id = outcome.notification.unwrap().id;
    let saves = first.persistence.saves();
    assert_eq!(saves.len(), 2);
    assert!(notification_file_fits_serialized_limit(&saves[0]));
    assert!(notification_file_fits_serialized_limit(&saves[1]));
    assert!(!saves[0]
        .partitions
        .iter()
        .any(|partition| { partition.items.iter().any(|record| record.id == carrier_id) }));
    assert!(!saves[0]
        .partitions
        .iter()
        .any(|partition| { partition.items.iter().any(|record| record.id == created_id) }));
    assert!(saves[1]
        .partitions
        .iter()
        .any(|partition| { partition.items.iter().any(|record| record.id == created_id) }));
    let events = first.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].change, NotificationChange::Reset);
    assert_eq!(events[0].previous_revision, "70");
    assert_eq!(events[0].revision, "71");
    assert_eq!(events[1].change, NotificationChange::Created);
    assert_eq!(events[1].previous_revision, "71");
    assert_eq!(events[1].revision, "72");
    drop(events);

    let root = std::env::temp_dir().join(format!(
        "easiflux-notification-service-byte-restart-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let path = root.join("notifications.v1.json");
    NotificationStore::with_path(path.clone())
        .save(&saves[1])
        .unwrap();
    let baseline = fs::read(&path).unwrap();
    let restart_events = Arc::new(Mutex::new(Vec::new()));
    let restart_sink = Arc::clone(&restart_events);
    let restart_emitter: NotificationEmitter = Arc::new(move |event| {
        restart_sink.lock().unwrap().push(event.clone());
        Ok(())
    });
    let restarted = NotificationService::load(
        NotificationStore::with_path(path.clone()),
        &["alpha".into()],
        NOW + 2,
        restart_emitter,
    )
    .unwrap();
    let duplicate = restarted
        .publish(input(scope, "pending-source", "pending-dedupe"), NOW + 2)
        .await
        .unwrap();
    assert!(duplicate.notification.is_none());
    assert_eq!(duplicate.revision, "72");
    assert!(restart_events.lock().unwrap().is_empty());
    drop(restarted);
    assert_eq!(fs::read(&path).unwrap(), baseline);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn serialized_reservation_accounts_for_revision_digit_growth() {
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let (file, carrier_id) = near_file_cap_before_created_publish_at(scope.clone(), 9, 0);
    let harness = harness(file);

    let outcome = harness
        .service
        .publish(input(scope, "pending-source", "pending-dedupe"), NOW + 1)
        .await
        .unwrap();

    assert_eq!(outcome.revision, "11");
    let created_id = outcome.notification.unwrap().id;
    let saves = harness.persistence.saves();
    assert_eq!(saves.len(), 2);
    assert_eq!(saves[0].revision, 10);
    assert_eq!(saves[1].revision, 11);
    assert!(notification_file_fits_serialized_limit(&saves[0]));
    assert!(notification_file_fits_serialized_limit(&saves[1]));
    assert!(!saves[0]
        .partitions
        .iter()
        .any(|partition| { partition.items.iter().any(|record| record.id == carrier_id) }));
    assert!(saves[1]
        .partitions
        .iter()
        .any(|partition| { partition.items.iter().any(|record| record.id == created_id) }));
    let events = harness.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].change, NotificationChange::Reset);
    assert_eq!(events[0].previous_revision, "9");
    assert_eq!(events[0].revision, "10");
    assert_eq!(events[1].change, NotificationChange::Created);
    assert_eq!(events[1].previous_revision, "10");
    assert_eq!(events[1].revision, "11");
}

#[tokio::test]
async fn serialized_reservation_protects_semantic_target_and_resets_before_updated() {
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let (file, carrier_id, target_id) = near_file_cap_before_semantic_publish(scope.clone());
    let harness = harness(file);

    let outcome = harness
        .service
        .publish(
            input(scope.clone(), "semantic-new-source", "semantic-target"),
            NOW + 2,
        )
        .await
        .unwrap();

    assert_eq!(outcome.revision, "82");
    let merged = outcome.notification.unwrap();
    assert_eq!(merged.id, target_id);
    assert_eq!(merged.occurrence_count, 2);
    assert_eq!(merged.read_at_ms, Some(NOW + 1));
    let saves = harness.persistence.saves();
    assert_eq!(saves.len(), 2);
    assert!(notification_file_fits_serialized_limit(&saves[1]));
    assert!(!saves[1]
        .partitions
        .iter()
        .any(|partition| { partition.items.iter().any(|record| record.id == carrier_id) }));
    assert!(saves[1]
        .partitions
        .iter()
        .any(|partition| { partition.items.iter().any(|record| record.id == target_id) }));
    let events = harness.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].change, NotificationChange::Reset);
    assert_eq!(events[0].previous_revision, "80");
    assert_eq!(events[0].revision, "81");
    assert_eq!(events[1].change, NotificationChange::Updated);
    assert_eq!(events[1].previous_revision, "81");
    assert_eq!(events[1].revision, "82");
    assert!(events[1].toast_candidate.is_none());
    drop(events);

    let root = std::env::temp_dir().join(format!(
        "easiflux-notification-service-byte-merge-restart-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let path = root.join("notifications.v1.json");
    NotificationStore::with_path(path.clone())
        .save(&saves[1])
        .unwrap();
    let baseline = fs::read(&path).unwrap();
    let restart_events = Arc::new(Mutex::new(Vec::new()));
    let restart_sink = Arc::clone(&restart_events);
    let restart_emitter: NotificationEmitter = Arc::new(move |event| {
        restart_sink.lock().unwrap().push(event.clone());
        Ok(())
    });
    let restarted = NotificationService::load(
        NotificationStore::with_path(path.clone()),
        &["alpha".into()],
        NOW + 3,
        restart_emitter,
    )
    .unwrap();

    let duplicate = restarted
        .publish(
            input(scope, "semantic-new-source", "semantic-target"),
            NOW + 3,
        )
        .await
        .unwrap();

    assert!(duplicate.notification.is_none());
    assert_eq!(duplicate.revision, "82");
    assert!(restart_events.lock().unwrap().is_empty());
    drop(restarted);
    assert_eq!(fs::read(&path).unwrap(), baseline);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn serialized_reservation_phase_failures_keep_copy_on_write_boundaries() {
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let pending = || input(scope.clone(), "pending-source", "pending-dedupe");

    let (file, carrier_id) = near_file_cap_before_created_publish(scope.clone());
    let phase_one = harness(file);
    phase_one.persistence.fail_next();
    let error = phase_one
        .service
        .publish(pending(), NOW + 1)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "NOTIFICATION_STORAGE_UNAVAILABLE");
    assert_eq!(phase_one.service.revision().await, "70");
    assert!(phase_one.persistence.saves().is_empty());
    assert!(phase_one.events.lock().unwrap().is_empty());
    let mut account_request = request();
    account_request.account_id = Some("alpha".into());
    let page = phase_one
        .service
        .list(
            ViewContext::account("alpha").unwrap(),
            account_request,
            NOW + 1,
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, carrier_id);

    let (file, _) = near_file_cap_before_created_publish(scope.clone());
    let phase_two = harness(file);
    phase_two.persistence.fail_after_successes(1);
    let error = phase_two
        .service
        .publish(pending(), NOW + 1)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "NOTIFICATION_STORAGE_UNAVAILABLE");
    assert_eq!(phase_two.service.revision().await, "71");
    assert_eq!(phase_two.persistence.saves().len(), 1);
    let events = phase_two.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].change, NotificationChange::Reset);
    assert_eq!(events[0].revision, "71");
    drop(events);
    let mut account_request = request();
    account_request.account_id = Some("alpha".into());
    assert!(phase_two
        .service
        .list(
            ViewContext::account("alpha").unwrap(),
            account_request,
            NOW + 1,
        )
        .await
        .unwrap()
        .items
        .is_empty());
    let retry = phase_two.service.publish(pending(), NOW + 2).await.unwrap();
    assert_eq!(retry.revision, "72");
    assert_eq!(phase_two.events.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn serialized_reservation_returns_domain_capacity_error_without_a_safe_candidate() {
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let target = record(1, scope.clone(), "target-source", "semantic-target", NOW);
    let target_id = target.id.clone();
    let harness = harness(NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 90,
        source_event_index: vec![NotificationSourceEventIndexEntry {
            scope: scope.clone(),
            source_event_id: "target-source".into(),
            notification_id: target_id.clone(),
        }],
        partitions: vec![NotificationPartition {
            scope: scope.clone(),
            items: vec![target],
        }],
    });
    let mut oversized = input(scope, "oversized-source", "semantic-target");
    oversized.category = NotificationCategory::ConnectionSystem;
    oversized.kind = NotificationKind::ConnectionUnavailable;
    oversized.severity = NotificationSeverity::Warning;
    oversized.content = NotificationContent::new(
        "connection.unavailable",
        [("channel", NotificationScalar::String("api".into()))],
        "连接不可用",
        "交易连接暂时不可用，请检查网络或稍后重试。",
    )
    .unwrap();
    oversized.action = Some(NotificationAction::OpenGeneralSettings);
    oversized.entity = Some(NotificationEntity {
        entity_type: NotificationEntityType::Order,
        id: "x".repeat(MAX_NOTIFICATION_FILE_BYTES),
    });

    let error = harness
        .service
        .publish(oversized, NOW + 1)
        .await
        .unwrap_err();

    assert_eq!(error.code(), "NOTIFICATION_FILE_CAPACITY_EXCEEDED");
    assert_eq!(harness.service.revision().await, "90");
    assert!(harness.persistence.saves().is_empty());
    assert!(harness.events.lock().unwrap().is_empty());
    let mut account_request = request();
    account_request.account_id = Some("alpha".into());
    let page = harness
        .service
        .list(
            ViewContext::account("alpha").unwrap(),
            account_request,
            NOW + 1,
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, target_id);
    assert_eq!(page.items[0].occurrence_count, 1);
}

#[tokio::test]
async fn source_index_scope_cap_evicts_oldest_record_as_a_reset_before_publish() {
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let mut unindexed = record(3, scope.clone(), "unused", "unindexed", NOW - 1);
    unindexed.source_event_id = None;
    let unindexed_id = unindexed.id.clone();
    let old = record(1, scope.clone(), "source-0", "old", NOW);
    let current = record(2, scope.clone(), "source-1", "current", NOW + 1);
    let mut source_event_index = vec![NotificationSourceEventIndexEntry {
        scope: scope.clone(),
        source_event_id: "source-0".into(),
        notification_id: old.id.clone(),
    }];
    source_event_index.extend((1..10_000).map(|index| NotificationSourceEventIndexEntry {
        scope: scope.clone(),
        source_event_id: format!("source-{index}"),
        notification_id: current.id.clone(),
    }));
    let harness = harness(NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 20,
        source_event_index,
        partitions: vec![NotificationPartition {
            scope: scope.clone(),
            items: vec![unindexed, old, current],
        }],
    });

    let outcome = harness
        .service
        .publish(input(scope, "source-new", "new"), NOW + 2)
        .await
        .unwrap();

    assert_eq!(outcome.revision, "22");
    let saved = harness.persistence.saves();
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0].source_event_index.len(), 9_999);
    assert_eq!(saved[1].source_event_index.len(), 10_000);
    assert!(saved[1].partitions[0]
        .items
        .iter()
        .any(|item| item.id == unindexed_id));
    assert!(!saved[1].partitions[0]
        .items
        .iter()
        .any(|item| item.source_event_id.as_deref() == Some("source-0")));
    let events = harness.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].change, NotificationChange::Reset);
    assert_eq!(events[1].change, NotificationChange::Created);
}

#[tokio::test]
async fn source_cap_rejects_a_new_semantic_source_when_target_owns_all_history_across_restart() {
    let root = std::env::temp_dir().join(format!(
        "easiflux-notification-service-semantic-cap-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let path = root.join("notifications.v1.json");
    let store = NotificationStore::with_path(path.clone());
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let mut target = record(1, scope.clone(), "source-0", "semantic-target", NOW);
    target.occurrence_count = 3;
    target.read_at_ms = Some(NOW);
    let target_id = target.id.clone();
    let source_event_index = (0..MAX_NOTIFICATION_SOURCE_INDEX_PER_SCOPE)
        .map(|index| NotificationSourceEventIndexEntry {
            scope: scope.clone(),
            source_event_id: format!("source-{index}"),
            notification_id: target_id.clone(),
        })
        .collect();
    store
        .save(&NotificationFileV1 {
            schema_version: NOTIFICATION_SCHEMA_VERSION,
            revision: 50,
            source_event_index,
            partitions: vec![NotificationPartition {
                scope: scope.clone(),
                items: vec![target],
            }],
        })
        .unwrap();
    let baseline = fs::read(&path).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let emitter: NotificationEmitter = Arc::new(move |event| {
        sink.lock().unwrap().push(event.clone());
        Ok(())
    });
    let service = NotificationService::load(store, &["alpha".into()], NOW, emitter).unwrap();

    let error = service
        .publish(
            input(scope.clone(), "source-new", "semantic-target"),
            NOW + 1,
        )
        .await
        .unwrap_err();

    assert_eq!(error.code(), "NOTIFICATION_SOURCE_INDEX_CAPACITY_EXCEEDED");
    assert_eq!(service.revision().await, "50");
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(fs::read(&path).unwrap(), baseline);
    drop(service);

    let restarted_events = Arc::new(Mutex::new(Vec::new()));
    let restarted_sink = Arc::clone(&restarted_events);
    let restarted_emitter: NotificationEmitter = Arc::new(move |event| {
        restarted_sink.lock().unwrap().push(event.clone());
        Ok(())
    });
    let restarted = NotificationService::load(
        NotificationStore::with_path(path.clone()),
        &["alpha".into()],
        NOW + 2,
        restarted_emitter,
    )
    .unwrap();
    let duplicate = restarted
        .publish(
            input(scope.clone(), "source-9999", "semantic-target"),
            NOW + 2,
        )
        .await
        .unwrap();
    assert!(duplicate.notification.is_none());
    assert_eq!(duplicate.revision, "50");
    let error = restarted
        .publish(
            input(scope.clone(), "source-new", "semantic-target"),
            NOW + 3,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "NOTIFICATION_SOURCE_INDEX_CAPACITY_EXCEEDED");
    assert_eq!(restarted.revision().await, "50");
    assert!(restarted_events.lock().unwrap().is_empty());
    assert_eq!(fs::read(&path).unwrap(), baseline);
    let mut account_request = request();
    account_request.account_id = Some("alpha".into());
    let page = restarted
        .list(
            ViewContext::account("alpha").unwrap(),
            account_request,
            NOW + 3,
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, target_id);
    assert_eq!(page.items[0].occurrence_count, 3);
    assert_eq!(page.items[0].read_at_ms, Some(NOW));
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn source_cap_protects_semantic_target_when_another_indexed_record_can_be_evicted() {
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let mut target = record(1, scope.clone(), "source-0", "semantic-target", NOW);
    target.occurrence_count = 3;
    target.read_at_ms = Some(NOW);
    let target_id = target.id.clone();
    let carrier = record(2, scope.clone(), "source-1", "semantic-carrier", NOW + 1);
    let carrier_id = carrier.id.clone();
    let mut source_event_index = vec![NotificationSourceEventIndexEntry {
        scope: scope.clone(),
        source_event_id: "source-0".into(),
        notification_id: target_id.clone(),
    }];
    source_event_index.extend((1..MAX_NOTIFICATION_SOURCE_INDEX_PER_SCOPE).map(|index| {
        NotificationSourceEventIndexEntry {
            scope: scope.clone(),
            source_event_id: format!("source-{index}"),
            notification_id: carrier_id.clone(),
        }
    }));
    let harness = harness(NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 60,
        source_event_index,
        partitions: vec![NotificationPartition {
            scope: scope.clone(),
            items: vec![target, carrier],
        }],
    });

    let outcome = harness
        .service
        .publish(input(scope, "source-new", "semantic-target"), NOW + 2)
        .await
        .unwrap();

    let merged = outcome.notification.unwrap();
    assert_eq!(merged.id, target_id);
    assert_eq!(merged.occurrence_count, 4);
    assert_eq!(merged.read_at_ms, Some(NOW));
    assert_eq!(outcome.revision, "62");
    let saved = harness.persistence.saves();
    assert_eq!(saved.len(), 2);
    assert!(!saved[0].partitions[0]
        .items
        .iter()
        .any(|item| item.id == carrier_id));
    assert!(saved[1].partitions[0]
        .items
        .iter()
        .any(|item| item.id == target_id));
    let events = harness.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].change, NotificationChange::Reset);
    assert_eq!(events[1].change, NotificationChange::Updated);
    assert!(events[1].toast_candidate.is_none());
}

#[tokio::test]
async fn source_index_total_cap_evicts_the_globally_oldest_record_before_publish() {
    let mut partitions = Vec::new();
    let mut source_event_index = Vec::new();
    for scope_index in 0..5_u128 {
        let scope = NotificationScope::Account {
            account_id: format!("scope-{scope_index}"),
        };
        let source = format!("source-{scope_index}-0");
        let record = record(
            scope_index + 1,
            scope.clone(),
            &source,
            &format!("dedupe-{scope_index}"),
            NOW + scope_index as u64,
        );
        let notification_id = record.id.clone();
        partitions.push(NotificationPartition {
            scope: scope.clone(),
            items: vec![record],
        });
        source_event_index.extend((0..10_000).map(|entry_index| {
            NotificationSourceEventIndexEntry {
                scope: scope.clone(),
                source_event_id: format!("source-{scope_index}-{entry_index}"),
                notification_id: notification_id.clone(),
            }
        }));
    }
    let harness = harness(NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 30,
        source_event_index,
        partitions,
    });
    let target = NotificationScope::Account {
        account_id: "scope-5".into(),
    };

    let outcome = harness
        .service
        .publish(input(target, "source-new", "new"), NOW + 10)
        .await
        .unwrap();

    assert_eq!(outcome.revision, "32");
    let saved = harness.persistence.saves();
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0].source_event_index.len(), 40_000);
    assert_eq!(saved[1].source_event_index.len(), 40_001);
    assert!(!saved[1].partitions.iter().any(|partition| {
        partition.scope
            == (NotificationScope::Account {
                account_id: "scope-0".into(),
            })
    }));
    let events = harness.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].change, NotificationChange::Reset);
    assert_eq!(
        events[0].affected_scopes,
        vec![NotificationScope::Account {
            account_id: "scope-0".into(),
        }]
    );
    assert_eq!(events[1].change, NotificationChange::Created);
}

#[tokio::test]
async fn pruning_an_empty_orphan_partition_is_a_persisted_reset() {
    let orphan = NotificationScope::Account {
        account_id: "orphan".into(),
    };
    let harness = harness(NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 12,
        source_event_index: Vec::new(),
        partitions: vec![NotificationPartition {
            scope: orphan.clone(),
            items: Vec::new(),
        }],
    });

    let outcome = harness
        .service
        .prune(&HashSet::from(["alpha".to_string()]), NOW)
        .await
        .unwrap();

    assert_eq!(outcome.affected_count, 0);
    assert_eq!(outcome.affected_scopes, vec![orphan]);
    assert_eq!(outcome.revision, "13");
    assert_eq!(harness.persistence.saves().len(), 1);
    let events = harness.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].change, NotificationChange::Reset);
}

#[tokio::test]
async fn maintenance_due_uses_a_twenty_four_hour_boundary() {
    let harness = harness(NotificationFileV1::empty());
    assert!(!harness.service.maintenance_due(NOW + DAY - 1).await);
    assert!(harness.service.maintenance_due(NOW + DAY).await);
    harness
        .service
        .prune(&HashSet::new(), NOW + DAY)
        .await
        .unwrap();
    assert!(!harness.service.maintenance_due(NOW + DAY + 1).await);
}

#[tokio::test]
async fn delete_account_partition_is_copy_on_write_and_preserves_global() {
    let harness = harness(NotificationFileV1::empty());
    harness
        .service
        .publish(input(NotificationScope::Global, "g", "g"), NOW)
        .await
        .unwrap();
    harness
        .service
        .publish(
            input(
                NotificationScope::Account {
                    account_id: "alpha".into(),
                },
                "a",
                "a",
            ),
            NOW,
        )
        .await
        .unwrap();
    harness
        .service
        .delete_account_partition("alpha", NOW + 1)
        .await
        .unwrap();
    let persisted = harness.persistence.saves().last().unwrap().clone();
    assert!(persisted
        .source_event_index
        .iter()
        .all(|entry| entry.scope == NotificationScope::Global));
    let global = harness
        .service
        .list(ViewContext::global(), request(), NOW + 1)
        .await
        .unwrap();
    assert_eq!(global.items.len(), 1);
}

#[tokio::test]
async fn deleting_an_existing_empty_account_partition_is_a_persisted_reset() {
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let harness = harness(NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 9,
        source_event_index: Vec::new(),
        partitions: vec![NotificationPartition {
            scope,
            items: Vec::new(),
        }],
    });

    harness
        .service
        .delete_account_partition("alpha", NOW)
        .await
        .unwrap();

    assert_eq!(harness.service.revision().await, "10");
    assert_eq!(harness.persistence.saves().len(), 1);
    let events = harness.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].change, NotificationChange::Reset);
}

#[tokio::test]
async fn deleting_account_without_a_partition_clears_seeded_order_state() {
    let harness = harness(NotificationFileV1::empty());
    let seeded = OrderObservation {
        account_id: "alpha".into(),
        session_epoch: 1,
        order_id: Some("order-1".into()),
        submission_id: None,
        order_link_id: None,
        status: ObservedOrderStatus::New,
        origin: OrderObservationOrigin::Snapshot,
    };
    harness
        .service
        .observe_order(seeded.clone(), NOW)
        .await
        .unwrap();
    harness
        .service
        .delete_account_partition("alpha", NOW + 1)
        .await
        .unwrap();

    let outcome = harness
        .service
        .observe_order(
            OrderObservation {
                session_epoch: 2,
                status: ObservedOrderStatus::Filled,
                origin: OrderObservationOrigin::Realtime,
                ..seeded
            },
            NOW + 2,
        )
        .await
        .unwrap();

    assert!(outcome.notification.is_none());
    assert!(harness.persistence.saves().is_empty());
}

#[tokio::test]
async fn api_websocket_and_environment_incidents_are_independent_edges() {
    let harness = harness(NotificationFileV1::empty());
    let api = ConnectionObservation {
        account_id: "alpha".into(),
        session_epoch: 7,
        channel: NotificationChannel::Api,
        state: AvailabilityState::Unavailable,
    };
    let websocket = ConnectionObservation {
        channel: NotificationChannel::Websocket,
        ..api.clone()
    };
    let environment = EnvironmentObservation {
        account_id: "alpha".into(),
        session_epoch: 7,
        environment_key: "env-v1-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .into(),
        environment: NotificationEnvironment::Production,
        state: AvailabilityState::Unavailable,
    };

    assert!(harness
        .service
        .observe_connection(api.clone(), NOW)
        .await
        .unwrap()
        .notification
        .is_some());
    assert!(harness
        .service
        .observe_connection(api.clone(), NOW + 1)
        .await
        .unwrap()
        .notification
        .is_none());
    assert!(harness
        .service
        .observe_connection(websocket, NOW + 2)
        .await
        .unwrap()
        .notification
        .is_some());
    assert!(harness
        .service
        .observe_environment(environment, NOW + 3)
        .await
        .unwrap()
        .notification
        .is_some());

    let recovered = ConnectionObservation {
        state: AvailabilityState::Available,
        ..api
    };
    let outcome = harness
        .service
        .observe_connection(recovered.clone(), NOW + 4)
        .await
        .unwrap();
    assert_eq!(
        outcome.notification.unwrap().kind,
        NotificationKind::ConnectionRecovered
    );
    assert!(harness
        .service
        .observe_connection(recovered, NOW + 5)
        .await
        .unwrap()
        .notification
        .is_none());
}

#[tokio::test]
async fn healthy_noop_observations_still_require_a_valid_account_scope() {
    let harness = harness(NotificationFileV1::empty());
    let connection_error = harness
        .service
        .observe_connection(
            ConnectionObservation {
                account_id: "".into(),
                session_epoch: 1,
                channel: NotificationChannel::Api,
                state: AvailabilityState::Available,
            },
            NOW,
        )
        .await
        .unwrap_err();
    assert_eq!(connection_error.code(), "INVALID_NOTIFICATION_SCOPE");

    let environment_error =
        harness
            .service
            .observe_environment(
                EnvironmentObservation {
                    account_id: "token-secret".into(),
                    session_epoch: 1,
                    environment_key:
                        "env-v1-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            .into(),
                    environment: NotificationEnvironment::Production,
                    state: AvailabilityState::Available,
                },
                NOW,
            )
            .await
            .unwrap_err();
    assert_eq!(environment_error.code(), "INVALID_NOTIFICATION_SCOPE");
}

#[tokio::test]
async fn stored_unavailable_history_rebuilds_incident_and_startup_health_closes_it() {
    let first = harness(NotificationFileV1::empty());
    first
        .service
        .observe_connection(
            ConnectionObservation {
                account_id: "alpha".into(),
                session_epoch: 7,
                channel: NotificationChannel::Api,
                state: AvailabilityState::Unavailable,
            },
            NOW,
        )
        .await
        .unwrap();
    let persisted = first.persistence.saves().last().unwrap().clone();
    let restarted = harness(persisted);
    let recovered = restarted
        .service
        .observe_connection(
            ConnectionObservation {
                account_id: "alpha".into(),
                session_epoch: 8,
                channel: NotificationChannel::Api,
                state: AvailabilityState::Available,
            },
            NOW + 1,
        )
        .await
        .unwrap();
    assert_eq!(
        recovered.notification.unwrap().kind,
        NotificationKind::ConnectionRecovered
    );

    let clean = harness(NotificationFileV1::empty());
    let no_incident = clean
        .service
        .observe_connection(
            ConnectionObservation {
                account_id: "alpha".into(),
                session_epoch: 8,
                channel: NotificationChannel::Api,
                state: AvailabilityState::Available,
            },
            NOW,
        )
        .await
        .unwrap();
    assert!(no_incident.notification.is_none());
}

#[tokio::test]
async fn equal_timestamp_history_rebuilds_unavailable_before_recovered() {
    let first = harness(NotificationFileV1::empty());
    let unavailable = ConnectionObservation {
        account_id: "alpha".into(),
        session_epoch: 7,
        channel: NotificationChannel::Api,
        state: AvailabilityState::Unavailable,
    };
    first
        .service
        .observe_connection(unavailable.clone(), NOW)
        .await
        .unwrap();
    first
        .service
        .observe_connection(
            ConnectionObservation {
                state: AvailabilityState::Available,
                ..unavailable.clone()
            },
            NOW,
        )
        .await
        .unwrap();
    let mut persisted = first.persistence.saves().last().unwrap().clone();
    for record in &mut persisted.partitions[0].items {
        record.id = match record.kind {
            NotificationKind::ConnectionUnavailable => {
                "00000000-0000-4000-8000-000000000002".into()
            }
            NotificationKind::ConnectionRecovered => "00000000-0000-4000-8000-000000000001".into(),
            _ => unreachable!(),
        };
    }
    persisted.source_event_index.clear();
    let restarted = harness(persisted);

    let next_incident = restarted
        .service
        .observe_connection(unavailable, NOW + 1)
        .await
        .unwrap();

    assert!(next_incident.notification.is_some());
}

#[tokio::test]
async fn legacy_same_millisecond_multi_cycle_fallback_keeps_the_unclosed_incident() {
    let first = harness(NotificationFileV1::empty());
    let unavailable = ConnectionObservation {
        account_id: "alpha".into(),
        session_epoch: 7,
        channel: NotificationChannel::Api,
        state: AvailabilityState::Unavailable,
    };
    first
        .service
        .observe_connection(unavailable.clone(), NOW)
        .await
        .unwrap();
    first
        .service
        .observe_connection(
            ConnectionObservation {
                state: AvailabilityState::Available,
                ..unavailable.clone()
            },
            NOW,
        )
        .await
        .unwrap();
    first
        .service
        .observe_connection(unavailable.clone(), NOW)
        .await
        .unwrap();
    let mut legacy = first.persistence.saves().last().unwrap().clone();
    let expected_incident = legacy
        .source_event_index
        .last()
        .unwrap()
        .source_event_id
        .split(':')
        .nth(2)
        .unwrap()
        .to_string();
    legacy.source_event_index.clear();

    let restarted = harness(legacy);
    let recovered = restarted
        .service
        .observe_connection(
            ConnectionObservation {
                state: AvailabilityState::Available,
                ..unavailable
            },
            NOW + 1,
        )
        .await
        .unwrap()
        .notification
        .unwrap();

    assert_eq!(recovered.kind, NotificationKind::ConnectionRecovered);
    assert!(recovered.dedupe_key.contains(&expected_incident));
}

#[tokio::test]
async fn partial_legacy_incident_order_stays_stable_after_backfill_is_saved_and_restarted() {
    let first = harness(NotificationFileV1::empty());
    let unavailable = ConnectionObservation {
        account_id: "alpha".into(),
        session_epoch: 7,
        channel: NotificationChannel::Api,
        state: AvailabilityState::Unavailable,
    };
    first
        .service
        .observe_connection(unavailable.clone(), NOW)
        .await
        .unwrap();
    first
        .service
        .observe_connection(
            ConnectionObservation {
                state: AvailabilityState::Available,
                ..unavailable.clone()
            },
            NOW,
        )
        .await
        .unwrap();
    first
        .service
        .observe_connection(unavailable.clone(), NOW)
        .await
        .unwrap();
    let mut partial = first.persistence.saves().last().unwrap().clone();
    let expected_incident = partial.source_event_index[2]
        .source_event_id
        .split(':')
        .nth(2)
        .unwrap()
        .to_string();
    partial.source_event_index.remove(0);
    let original_revision = partial.revision;
    let root = std::env::temp_dir().join(format!(
        "easiflux-notification-service-partial-incident-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let path = root.join("notifications.v1.json");
    NotificationStore::with_path(path.clone())
        .save(&partial)
        .unwrap();
    let normalization_events = Arc::new(Mutex::new(Vec::new()));
    let normalization_sink = Arc::clone(&normalization_events);
    let normalization_emitter: NotificationEmitter = Arc::new(move |event| {
        normalization_sink.lock().unwrap().push(event.clone());
        Ok(())
    });
    let normalized = NotificationService::load(
        NotificationStore::with_path(path.clone()),
        &["alpha".into()],
        NOW + 1,
        normalization_emitter,
    )
    .unwrap();
    assert_eq!(
        normalized.revision().await,
        (original_revision + 1).to_string()
    );
    let emitted = normalization_events.lock().unwrap();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].change, NotificationChange::Reset);
    drop(emitted);
    drop(normalized);

    let restart_events = Arc::new(Mutex::new(Vec::new()));
    let restart_sink = Arc::clone(&restart_events);
    let restart_emitter: NotificationEmitter = Arc::new(move |event| {
        restart_sink.lock().unwrap().push(event.clone());
        Ok(())
    });
    let restarted = NotificationService::load(
        NotificationStore::with_path(path.clone()),
        &["alpha".into()],
        NOW + 2,
        restart_emitter,
    )
    .unwrap();
    assert_eq!(
        restarted.revision().await,
        (original_revision + 1).to_string()
    );
    assert!(restart_events.lock().unwrap().is_empty());
    let recovered = restarted
        .observe_connection(
            ConnectionObservation {
                state: AvailabilityState::Available,
                ..unavailable
            },
            NOW + 3,
        )
        .await
        .unwrap()
        .notification
        .unwrap();

    assert_eq!(recovered.kind, NotificationKind::ConnectionRecovered);
    assert!(recovered.dedupe_key.contains(&expected_incident));
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn order_observer_honors_snapshot_command_and_realtime_terminal_rules() {
    let harness = harness(NotificationFileV1::empty());
    let snapshot_terminal = OrderObservation {
        account_id: "alpha".into(),
        session_epoch: 9,
        order_id: Some("order-10".into()),
        submission_id: None,
        order_link_id: None,
        status: ObservedOrderStatus::Filled,
        origin: OrderObservationOrigin::Snapshot,
    };
    assert!(harness
        .service
        .observe_order(snapshot_terminal, NOW)
        .await
        .unwrap()
        .notification
        .is_none());

    let command = OrderObservation {
        account_id: "alpha".into(),
        session_epoch: 9,
        order_id: Some("order-11".into()),
        submission_id: None,
        order_link_id: None,
        status: ObservedOrderStatus::Canceled,
        origin: OrderObservationOrigin::Command,
    };
    assert!(harness
        .service
        .observe_order(command.clone(), NOW + 1)
        .await
        .unwrap()
        .notification
        .is_some());
    assert!(harness
        .service
        .observe_order(command, NOW + 2)
        .await
        .unwrap()
        .notification
        .is_none());

    let realtime_new = OrderObservation {
        account_id: "alpha".into(),
        session_epoch: 9,
        order_id: Some("order-12".into()),
        submission_id: None,
        order_link_id: None,
        status: ObservedOrderStatus::New,
        origin: OrderObservationOrigin::Realtime,
    };
    harness
        .service
        .observe_order(realtime_new.clone(), NOW + 3)
        .await
        .unwrap();
    let realtime_filled = OrderObservation {
        status: ObservedOrderStatus::Filled,
        ..realtime_new
    };
    assert!(harness
        .service
        .observe_order(realtime_filled, NOW + 4)
        .await
        .unwrap()
        .notification
        .is_some());
}

#[tokio::test]
async fn command_terminal_still_publishes_after_same_order_snapshot_seeded_terminal() {
    let harness = harness(NotificationFileV1::empty());
    let snapshot = OrderObservation {
        account_id: "alpha".into(),
        session_epoch: 9,
        order_id: Some("order-20".into()),
        submission_id: None,
        order_link_id: None,
        status: ObservedOrderStatus::Filled,
        origin: OrderObservationOrigin::Snapshot,
    };
    harness
        .service
        .observe_order(snapshot.clone(), NOW)
        .await
        .unwrap();

    let command = harness
        .service
        .observe_order(
            OrderObservation {
                origin: OrderObservationOrigin::Command,
                ..snapshot
            },
            NOW + 1,
        )
        .await
        .unwrap();

    assert!(command.notification.is_some());
    assert_eq!(harness.persistence.saves().len(), 1);
}

#[tokio::test]
async fn terminal_order_state_absorbs_late_nonterminal_and_conflicting_terminal_observations() {
    let harness = harness(NotificationFileV1::empty());
    let snapshot = OrderObservation {
        account_id: "alpha".into(),
        session_epoch: 9,
        order_id: Some("order-30".into()),
        submission_id: None,
        order_link_id: None,
        status: ObservedOrderStatus::Filled,
        origin: OrderObservationOrigin::Snapshot,
    };
    harness
        .service
        .observe_order(snapshot.clone(), NOW)
        .await
        .unwrap();

    for (status, origin, observed_at_ms) in [
        (
            ObservedOrderStatus::New,
            OrderObservationOrigin::Realtime,
            NOW + 1,
        ),
        (
            ObservedOrderStatus::PartiallyFilled,
            OrderObservationOrigin::Snapshot,
            NOW + 2,
        ),
        (
            ObservedOrderStatus::Canceled,
            OrderObservationOrigin::Command,
            NOW + 3,
        ),
        (
            ObservedOrderStatus::Canceled,
            OrderObservationOrigin::Realtime,
            NOW + 4,
        ),
    ] {
        let ignored = harness
            .service
            .observe_order(
                OrderObservation {
                    status,
                    origin,
                    ..snapshot.clone()
                },
                observed_at_ms,
            )
            .await
            .unwrap();
        assert!(ignored.notification.is_none());
    }
    assert!(harness.persistence.saves().is_empty());
    assert!(harness.events.lock().unwrap().is_empty());

    let matching = harness
        .service
        .observe_order(
            OrderObservation {
                origin: OrderObservationOrigin::Command,
                ..snapshot.clone()
            },
            NOW + 5,
        )
        .await
        .unwrap();
    assert_eq!(
        matching.notification.unwrap().kind,
        NotificationKind::OrderFilled
    );

    let duplicate = harness
        .service
        .observe_order(
            OrderObservation {
                origin: OrderObservationOrigin::Command,
                ..snapshot
            },
            NOW + 6,
        )
        .await
        .unwrap();
    assert!(duplicate.notification.is_none());
    assert_eq!(harness.persistence.saves().len(), 1);
    assert_eq!(harness.events.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn observer_memory_transitions_roll_back_when_persistence_fails() {
    let incident = harness(NotificationFileV1::empty());
    let unavailable = ConnectionObservation {
        account_id: "alpha".into(),
        session_epoch: 1,
        channel: NotificationChannel::Api,
        state: AvailabilityState::Unavailable,
    };
    incident.persistence.fail_next();
    assert_eq!(
        incident
            .service
            .observe_connection(unavailable.clone(), NOW)
            .await
            .unwrap_err()
            .code(),
        "NOTIFICATION_STORAGE_UNAVAILABLE"
    );
    assert!(incident
        .service
        .observe_connection(unavailable, NOW + 1)
        .await
        .unwrap()
        .notification
        .is_some());

    let order = harness(NotificationFileV1::empty());
    let terminal = OrderObservation {
        account_id: "alpha".into(),
        session_epoch: 1,
        order_id: Some("order-rollback-1".into()),
        submission_id: None,
        order_link_id: None,
        status: ObservedOrderStatus::Filled,
        origin: OrderObservationOrigin::Command,
    };
    order.persistence.fail_next();
    assert_eq!(
        order
            .service
            .observe_order(terminal.clone(), NOW)
            .await
            .unwrap_err()
            .code(),
        "NOTIFICATION_STORAGE_UNAVAILABLE"
    );
    assert!(order
        .service
        .observe_order(terminal, NOW + 1)
        .await
        .unwrap()
        .notification
        .is_some());
}

#[tokio::test]
async fn observer_state_does_not_leak_into_a_committed_housekeeping_phase() {
    let seeded = || NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 40,
        source_event_index: Vec::new(),
        partitions: vec![NotificationPartition {
            scope: NotificationScope::Global,
            items: vec![record(
                1,
                NotificationScope::Global,
                "expired",
                "expired",
                NOW - 91 * DAY,
            )],
        }],
    };

    let incident = harness(seeded());
    let unavailable = ConnectionObservation {
        account_id: "alpha".into(),
        session_epoch: 1,
        channel: NotificationChannel::Api,
        state: AvailabilityState::Unavailable,
    };
    incident.persistence.fail_after_successes(1);
    assert_eq!(
        incident
            .service
            .observe_connection(unavailable.clone(), NOW)
            .await
            .unwrap_err()
            .code(),
        "NOTIFICATION_STORAGE_UNAVAILABLE"
    );
    assert_eq!(incident.service.revision().await, "41");
    assert!(incident
        .service
        .observe_connection(unavailable, NOW + 1)
        .await
        .unwrap()
        .notification
        .is_some());

    let order = harness(seeded());
    let terminal = OrderObservation {
        account_id: "alpha".into(),
        session_epoch: 1,
        order_id: Some("order-housekeeping-1".into()),
        submission_id: None,
        order_link_id: None,
        status: ObservedOrderStatus::Filled,
        origin: OrderObservationOrigin::Command,
    };
    order.persistence.fail_after_successes(1);
    assert_eq!(
        order
            .service
            .observe_order(terminal.clone(), NOW)
            .await
            .unwrap_err()
            .code(),
        "NOTIFICATION_STORAGE_UNAVAILABLE"
    );
    assert_eq!(order.service.revision().await, "41");
    assert!(order
        .service
        .observe_order(terminal, NOW + 1)
        .await
        .unwrap()
        .notification
        .is_some());
}

#[tokio::test]
async fn incident_state_does_not_leak_when_byte_reservation_phase_two_fails() {
    let scope = NotificationScope::Account {
        account_id: "alpha".into(),
    };
    let (mut file, carrier_id) = near_file_cap_before_created_publish(scope);
    let current_bytes = serde_json::to_vec_pretty(&file).unwrap().len();
    file.partitions[0].items[0]
        .entity
        .as_mut()
        .unwrap()
        .id
        .push_str(&"x".repeat(MAX_NOTIFICATION_FILE_BYTES - current_bytes));
    assert_eq!(
        serde_json::to_vec_pretty(&file).unwrap().len(),
        MAX_NOTIFICATION_FILE_BYTES
    );
    let harness = harness(file);
    let unavailable = ConnectionObservation {
        account_id: "alpha".into(),
        session_epoch: 1,
        channel: NotificationChannel::Api,
        state: AvailabilityState::Unavailable,
    };
    harness.persistence.fail_after_successes(1);

    let error = harness
        .service
        .observe_connection(unavailable.clone(), NOW + 1)
        .await
        .unwrap_err();

    assert_eq!(error.code(), "NOTIFICATION_STORAGE_UNAVAILABLE");
    assert_eq!(harness.service.revision().await, "71");
    let saves = harness.persistence.saves();
    assert_eq!(saves.len(), 1);
    assert!(notification_file_fits_serialized_limit(&saves[0]));
    assert!(!saves[0]
        .partitions
        .iter()
        .any(|partition| { partition.items.iter().any(|record| record.id == carrier_id) }));
    let events = harness.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].change, NotificationChange::Reset);
    assert_eq!(events[0].previous_revision, "70");
    assert_eq!(events[0].revision, "71");
    drop(events);

    let retry = harness
        .service
        .observe_connection(unavailable, NOW + 2)
        .await
        .unwrap();
    assert_eq!(retry.revision, "72");
    assert_eq!(
        retry.notification.unwrap().kind,
        NotificationKind::ConnectionUnavailable
    );
    let events = harness.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].change, NotificationChange::Created);
    assert!(events[1].toast_candidate.is_some());
}

#[tokio::test]
async fn same_millisecond_multi_cycle_incidents_rebuild_in_source_index_causal_order() {
    let first = harness(NotificationFileV1::empty());
    let api = ConnectionObservation {
        account_id: "alpha".into(),
        session_epoch: 7,
        channel: NotificationChannel::Api,
        state: AvailabilityState::Unavailable,
    };
    let websocket = ConnectionObservation {
        channel: NotificationChannel::Websocket,
        ..api.clone()
    };
    let environment = EnvironmentObservation {
        account_id: "alpha".into(),
        session_epoch: 7,
        environment_key: "env-v1-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .into(),
        environment: NotificationEnvironment::Production,
        state: AvailabilityState::Unavailable,
    };

    for observation in [api.clone(), websocket.clone()] {
        first
            .service
            .observe_connection(observation.clone(), NOW)
            .await
            .unwrap();
        first
            .service
            .observe_connection(
                ConnectionObservation {
                    state: AvailabilityState::Available,
                    ..observation.clone()
                },
                NOW,
            )
            .await
            .unwrap();
        first
            .service
            .observe_connection(observation, NOW)
            .await
            .unwrap();
    }
    first
        .service
        .observe_environment(environment.clone(), NOW)
        .await
        .unwrap();
    first
        .service
        .observe_environment(
            EnvironmentObservation {
                state: AvailabilityState::Available,
                ..environment.clone()
            },
            NOW,
        )
        .await
        .unwrap();
    first
        .service
        .observe_environment(environment.clone(), NOW)
        .await
        .unwrap();

    let mut persisted = first.persistence.saves().last().unwrap().clone();
    assert_eq!(persisted.source_event_index.len(), 9);
    let expected_incidents: Vec<_> = [2, 5, 8]
        .into_iter()
        .map(|index| {
            persisted.source_event_index[index]
                .source_event_id
                .split(':')
                .nth(2)
                .unwrap()
                .to_string()
        })
        .collect();
    let mut replacement_ids = HashMap::new();
    for (group, base) in [(0, 100_u128), (1, 200), (2, 300)] {
        for (offset, suffix) in [(0, 2_u128), (1, 3), (2, 1)] {
            let old = persisted.source_event_index[group * 3 + offset]
                .notification_id
                .clone();
            replacement_ids.insert(
                old,
                format!("00000000-0000-4000-8000-{:012x}", base + suffix),
            );
        }
    }
    for partition in &mut persisted.partitions {
        for record in &mut partition.items {
            record.id = replacement_ids.get(&record.id).unwrap().clone();
        }
    }
    for entry in &mut persisted.source_event_index {
        entry.notification_id = replacement_ids.get(&entry.notification_id).unwrap().clone();
    }

    let restarted = harness(persisted);
    for (observation, expected_incident) in [api, websocket].into_iter().zip(&expected_incidents) {
        let recovered = restarted
            .service
            .observe_connection(
                ConnectionObservation {
                    state: AvailabilityState::Available,
                    ..observation
                },
                NOW + 1,
            )
            .await
            .unwrap();
        let recovered = recovered.notification.unwrap();
        assert_eq!(recovered.kind, NotificationKind::ConnectionRecovered);
        assert!(recovered.dedupe_key.contains(expected_incident));
    }
    let recovered = restarted
        .service
        .observe_environment(
            EnvironmentObservation {
                state: AvailabilityState::Available,
                ..environment
            },
            NOW + 1,
        )
        .await
        .unwrap();
    let recovered = recovered.notification.unwrap();
    assert_eq!(recovered.kind, NotificationKind::EnvironmentRecovered);
    assert!(recovered.dedupe_key.contains(&expected_incidents[2]));
}
