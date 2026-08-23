use std::collections::HashSet;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::models::notification::{
    ListNotificationsRequest, NotificationChange, NotificationChannel, NotificationEnvironment,
    NotificationFilter, NotificationKind, NotificationScope,
};
use crate::services::notification::{
    AvailabilityState, ConnectionObservation, EnvironmentObservation, NotificationEmitter,
    NotificationService, ObservedOrderStatus, OrderObservation, OrderObservationOrigin,
    ViewContext,
};
use crate::storage::notification_store::{
    NotificationFileV1, NotificationPartition, NotificationStore, NOTIFICATION_SCHEMA_VERSION,
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
                    NOW + index as u64,
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
            .publish(input(scope, "new", "new"), NOW + 2_000)
            .await
            .unwrap();
        let saved = harness.persistence.saves();
        let partition = &saved.last().unwrap().partitions[0];
        assert_eq!(partition.items.len(), 1_000);
        assert_eq!(saved.last().unwrap().source_event_index.len(), 1_000);
        assert!(!partition
            .items
            .iter()
            .any(|item| item.source_event_id.as_deref() == Some("source-0")));
    }
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
async fn deleting_account_without_a_partition_clears_seeded_order_state() {
    let harness = harness(NotificationFileV1::empty());
    let seeded = OrderObservation {
        account_id: "alpha".into(),
        session_epoch: 1,
        order_id: Some("order-1".into()),
        submission_id: None,
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

    let environment_error = harness
        .service
        .observe_environment(
            EnvironmentObservation {
                account_id: "token-secret".into(),
                session_epoch: 1,
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
    let restarted = harness(persisted);

    let next_incident = restarted
        .service
        .observe_connection(unavailable, NOW + 1)
        .await
        .unwrap();

    assert!(next_incident.notification.is_some());
}

#[tokio::test]
async fn order_observer_honors_snapshot_command_and_realtime_terminal_rules() {
    let harness = harness(NotificationFileV1::empty());
    let snapshot_terminal = OrderObservation {
        account_id: "alpha".into(),
        session_epoch: 9,
        order_id: Some("order-10".into()),
        submission_id: None,
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
