use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    read_candidate_from_open_file, Candidate, FailurePoint, NotificationFileV1,
    NotificationLoadStatus, NotificationPartition, NotificationPersistence,
    NotificationSourceEventIndexEntry, NotificationStore, RecoverySource,
    MAX_NOTIFICATION_FILE_BYTES, NOTIFICATION_SCHEMA_VERSION,
};
use crate::error::AppError;
use crate::models::notification::{
    NotificationCategory, NotificationContent, NotificationKind, NotificationRecord,
    NotificationScalar, NotificationScope, NotificationSeverity, MAX_JAVASCRIPT_SAFE_INTEGER,
};

static TEST_ROOT_ID: AtomicU64 = AtomicU64::new(0);

fn test_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "easiflux-notification-store-{}-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed),
        label,
    ))
}

fn store_path(root: &Path) -> PathBuf {
    root.join("notifications.v1.json")
}

fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

fn sample_record(id: &str, scope: NotificationScope, created_at_ms: u64) -> NotificationRecord {
    NotificationRecord {
        id: id.into(),
        scope,
        category: NotificationCategory::Trading,
        kind: NotificationKind::OrderFilled,
        severity: NotificationSeverity::Success,
        content: NotificationContent::new(
            "order.filled",
            [("orderId", NotificationScalar::String("order-42".into()))],
            "订单已成交",
            "订单已完全成交，请前往交易页查看。",
        )
        .unwrap(),
        entity: None,
        action: None,
        source_event_id: Some(format!("event-{created_at_ms}")),
        dedupe_key: format!("order-filled-{created_at_ms}"),
        occurrence_count: 1,
        created_at_ms,
        updated_at_ms: created_at_ms,
        read_at_ms: None,
    }
}

fn sample_file(revision: u64) -> NotificationFileV1 {
    NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision,
        source_event_index: Vec::new(),
        partitions: vec![NotificationPartition {
            scope: NotificationScope::Global,
            items: vec![sample_record(
                "00000000-0000-4000-8000-000000000001",
                NotificationScope::Global,
                100,
            )],
        }],
    }
}

fn oversized_file(revision: u64) -> NotificationFileV1 {
    NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision,
        source_event_index: Vec::new(),
        partitions: vec![NotificationPartition {
            scope: NotificationScope::Global,
            items: (0..=1_000)
                .map(|index| {
                    sample_record(
                        &format!("00000000-0000-4000-8000-{index:012}"),
                        NotificationScope::Global,
                        index + 1,
                    )
                })
                .collect(),
        }],
    }
}

fn source_index_file(entries_per_scope: &[usize], padding: usize) -> NotificationFileV1 {
    let mut partitions = Vec::new();
    let mut source_event_index = Vec::new();
    for (scope_index, entry_count) in entries_per_scope.iter().copied().enumerate() {
        let scope = NotificationScope::Account {
            account_id: format!("scope-{scope_index}"),
        };
        let record = sample_record(
            &format!("00000000-0000-4000-8000-{:012x}", scope_index + 1),
            scope.clone(),
            100 + scope_index as u64,
        );
        let notification_id = record.id.clone();
        partitions.push(NotificationPartition {
            scope: scope.clone(),
            items: vec![record],
        });
        source_event_index.extend((0..entry_count).map(|entry_index| {
            NotificationSourceEventIndexEntry {
                scope: scope.clone(),
                source_event_id: format!(
                    "semantic-{scope_index}-{entry_index}-{}",
                    "x".repeat(padding)
                ),
                notification_id: notification_id.clone(),
            }
        }));
    }
    NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 1,
        source_event_index,
        partitions,
    }
}

fn write_json(path: &Path, value: &impl serde::Serialize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn read_file(path: &Path) -> NotificationFileV1 {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn assert_storage_unavailable(error: AppError) {
    assert!(
        matches!(error, AppError::Storage(ref message) if message == "NOTIFICATION_STORAGE_UNAVAILABLE")
    );
}

fn assert_persistence_is_send_sync<T: NotificationPersistence>() {}

#[test]
fn round_trip_preserves_revision_partitions_and_records() {
    let root = test_root("round-trip");
    let store = NotificationStore::with_path(store_path(&root));
    assert_persistence_is_send_sync::<NotificationStore>();
    let file = NotificationFileV1 {
        schema_version: NOTIFICATION_SCHEMA_VERSION,
        revision: 7,
        source_event_index: Vec::new(),
        partitions: vec![
            NotificationPartition {
                scope: NotificationScope::Global,
                items: vec![sample_record(
                    "00000000-0000-4000-8000-000000000001",
                    NotificationScope::Global,
                    100,
                )],
            },
            NotificationPartition {
                scope: NotificationScope::Account {
                    account_id: "account_1".into(),
                },
                items: vec![sample_record(
                    "00000000-0000-4000-8000-000000000002",
                    NotificationScope::Account {
                        account_id: "account_1".into(),
                    },
                    200,
                )],
            },
        ],
    };

    store.save(&file).unwrap();
    let outcome = store.load().unwrap();

    assert_eq!(outcome.status, NotificationLoadStatus::Clean);
    assert_eq!(outcome.file, file);
    cleanup(&root);
}

#[test]
fn load_prefers_main_then_tmp_then_backup() {
    for (label, main, temp, backup, expected_revision, expected_status) in [
        (
            "main",
            Some(sample_file(1)),
            Some(sample_file(2)),
            Some(sample_file(3)),
            1,
            NotificationLoadStatus::Clean,
        ),
        (
            "temp",
            None,
            Some(sample_file(2)),
            Some(sample_file(3)),
            2,
            NotificationLoadStatus::Recovered {
                source: RecoverySource::Temp,
            },
        ),
        (
            "backup",
            None,
            None,
            Some(sample_file(3)),
            3,
            NotificationLoadStatus::Recovered {
                source: RecoverySource::Backup,
            },
        ),
    ] {
        let root = test_root(label);
        let path = store_path(&root);
        if let Some(file) = main {
            write_json(&path, &file);
        }
        if let Some(file) = temp {
            write_json(&NotificationStore::temp_path_for_test(&path), &file);
        }
        if let Some(file) = backup {
            write_json(&NotificationStore::backup_path_for_test(&path), &file);
        }

        let outcome = NotificationStore::with_path(path).load().unwrap();

        assert_eq!(outcome.file.revision, expected_revision);
        assert_eq!(outcome.status, expected_status);
        cleanup(&root);
    }
}

#[test]
fn future_main_schema_is_not_downgraded_to_an_older_backup() {
    let root = test_root("future-main");
    let path = store_path(&root);
    let future_bytes = br#"{"schemaVersion":2,"futureData":{"kept":true}}"#;
    fs::create_dir_all(&root).unwrap();
    fs::write(&path, future_bytes).unwrap();
    write_json(
        &NotificationStore::backup_path_for_test(&path),
        &sample_file(4),
    );

    let outcome = NotificationStore::with_path(path.clone()).load().unwrap();

    assert_eq!(
        outcome.status,
        NotificationLoadStatus::UnsupportedSchema { found: 2 }
    );
    assert_eq!(outcome.file, NotificationFileV1::empty());
    assert_eq!(fs::read(&path).unwrap(), future_bytes);
    assert_eq!(
        read_file(&NotificationStore::backup_path_for_test(&path)).revision,
        4
    );
    cleanup(&root);
}

#[test]
fn save_does_not_overwrite_or_rename_a_future_main_schema() {
    let root = test_root("future-main-save");
    let path = store_path(&root);
    let future_bytes = br#"{"schemaVersion":3,"futureData":{"kept":true}}"#;
    fs::create_dir_all(&root).unwrap();
    fs::write(&path, future_bytes).unwrap();

    assert_storage_unavailable(
        NotificationStore::with_path(path.clone())
            .save(&sample_file(8))
            .unwrap_err(),
    );

    assert_eq!(fs::read(&path).unwrap(), future_bytes);
    assert!(!NotificationStore::backup_path_for_test(&path).exists());
    assert!(!NotificationStore::temp_path_for_test(&path).exists());
    cleanup(&root);
}

#[test]
fn save_preserves_a_future_temp_when_no_current_schema_main_is_authoritative() {
    let root = test_root("future-temp-save");
    let path = store_path(&root);
    let temp_path = NotificationStore::temp_path_for_test(&path);
    let future_bytes = br#"{"schemaVersion":3,"futureData":{"kept":true}}"#;
    fs::create_dir_all(&root).unwrap();
    fs::write(&path, b"corrupt-main").unwrap();
    fs::write(&temp_path, future_bytes).unwrap();

    assert_storage_unavailable(
        NotificationStore::with_path(path.clone())
            .save(&sample_file(8))
            .unwrap_err(),
    );

    assert_eq!(fs::read(&path).unwrap(), b"corrupt-main");
    assert_eq!(fs::read(&temp_path).unwrap(), future_bytes);
    assert!(!NotificationStore::backup_path_for_test(&path).exists());
    cleanup(&root);
}

#[test]
fn valid_main_remains_authoritative_over_a_lower_future_temp_on_save() {
    let root = test_root("current-main-future-temp");
    let path = store_path(&root);
    let store = NotificationStore::with_path(path.clone());
    store.save(&sample_file(1)).unwrap();
    fs::write(
        NotificationStore::temp_path_for_test(&path),
        br#"{"schemaVersion":3,"futureData":{"stale":true}}"#,
    )
    .unwrap();

    store.save(&sample_file(2)).unwrap();

    assert_eq!(read_file(&path).revision, 2);
    assert_eq!(
        read_file(&NotificationStore::backup_path_for_test(&path)).revision,
        1
    );
    assert!(!NotificationStore::temp_path_for_test(&path).exists());
    cleanup(&root);
}

#[test]
fn valid_main_remains_authoritative_over_lower_oversized_unknown_artifacts() {
    for label in ["temp", "backup"] {
        let root = test_root(&format!("current-main-oversized-unknown-{label}"));
        let path = store_path(&root);
        let store = NotificationStore::with_path(path.clone());
        store.save(&sample_file(1)).unwrap();
        let artifact_path = if label == "temp" {
            NotificationStore::temp_path_for_test(&path)
        } else {
            NotificationStore::backup_path_for_test(&path)
        };
        let oversized_unknown = vec![b' '; MAX_NOTIFICATION_FILE_BYTES + 1];
        fs::write(&artifact_path, &oversized_unknown).unwrap();

        store.save(&sample_file(2)).unwrap();

        assert_eq!(read_file(&path).revision, 2);
        assert_eq!(
            read_file(&NotificationStore::backup_path_for_test(&path)).revision,
            1
        );
        assert!(!NotificationStore::temp_path_for_test(&path).exists());
        cleanup(&root);
    }
}

#[test]
fn all_corrupt_v1_candidates_are_preserved_and_empty_state_is_returned() {
    let root = test_root("all-corrupt");
    let path = store_path(&root);
    let temp = NotificationStore::temp_path_for_test(&path);
    let backup = NotificationStore::backup_path_for_test(&path);
    fs::create_dir_all(&root).unwrap();
    fs::write(&path, b"not-json-main").unwrap();
    fs::write(&temp, br#"{"schemaVersion":1,"partitions":[]}"#).unwrap();
    fs::write(&backup, br#"{"schemaVersion":0}"#).unwrap();

    let outcome = NotificationStore::with_path(path.clone()).load().unwrap();

    assert_eq!(outcome.status, NotificationLoadStatus::ResetFromCorruption);
    assert_eq!(outcome.file, NotificationFileV1::empty());
    assert!(!path.exists());
    assert!(!temp.exists());
    assert!(!backup.exists());
    let evidence: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".corrupt-"))
        .collect();
    assert_eq!(evidence.len(), 3, "evidence files: {evidence:?}");
    cleanup(&root);
}

#[test]
fn duplicate_partition_scope_is_rejected() {
    let root = test_root("duplicate-scope");
    let store = NotificationStore::with_path(store_path(&root));
    let mut file = sample_file(1);
    file.partitions.push(NotificationPartition {
        scope: NotificationScope::Global,
        items: Vec::new(),
    });

    assert!(matches!(store.save(&file), Err(AppError::Storage(_))));
    assert!(!store.path_for_test().exists());
    cleanup(&root);
}

#[test]
fn duplicate_record_id_across_partitions_is_rejected() {
    let root = test_root("duplicate-id");
    let store = NotificationStore::with_path(store_path(&root));
    let mut file = sample_file(1);
    let duplicate_id = file.partitions[0].items[0].id.clone();
    let account_scope = NotificationScope::Account {
        account_id: "account_1".into(),
    };
    file.partitions.push(NotificationPartition {
        scope: account_scope.clone(),
        items: vec![sample_record(&duplicate_id, account_scope, 200)],
    });

    assert!(matches!(store.save(&file), Err(AppError::Storage(_))));
    assert!(!store.path_for_test().exists());
    cleanup(&root);
}

#[test]
fn save_rejects_a_partition_over_one_thousand_items() {
    let root = test_root("oversized-save");
    let store = NotificationStore::with_path(store_path(&root));

    assert!(matches!(
        store.save(&oversized_file(1)),
        Err(AppError::Storage(_))
    ));
    assert!(!store.path_for_test().exists());
    cleanup(&root);
}

#[test]
fn load_treats_a_partition_over_one_thousand_items_as_corrupt() {
    let root = test_root("oversized-load");
    let path = store_path(&root);
    write_json(&path, &oversized_file(1));

    let outcome = NotificationStore::with_path(path.clone()).load().unwrap();

    assert_eq!(outcome.status, NotificationLoadStatus::ResetFromCorruption);
    assert_eq!(outcome.file, NotificationFileV1::empty());
    assert!(!path.exists());
    cleanup(&root);
}

#[test]
fn invalid_timestamps_scalars_and_enums_are_rejected() {
    let timestamp_root = test_root("invalid-timestamp");
    let timestamp_store = NotificationStore::with_path(store_path(&timestamp_root));
    let mut invalid_timestamp = sample_file(1);
    invalid_timestamp.partitions[0].items[0].updated_at_ms = MAX_JAVASCRIPT_SAFE_INTEGER + 1;
    assert!(matches!(
        timestamp_store.save(&invalid_timestamp),
        Err(AppError::Storage(_))
    ));

    let scalar_root = test_root("invalid-scalar");
    let scalar_store = NotificationStore::with_path(store_path(&scalar_root));
    let mut invalid_scalar = sample_file(1);
    invalid_scalar.partitions[0].items[0].category = NotificationCategory::RiskAccount;
    invalid_scalar.partitions[0].items[0].kind = NotificationKind::RiskOrderBlocked;
    invalid_scalar.partitions[0].items[0].content = NotificationContent::new(
        "risk.orderBlocked",
        [("limit", NotificationScalar::Number(1.0))],
        "订单被风控拦截",
        "请检查风控设置",
    )
    .unwrap();
    invalid_scalar.partitions[0].items[0]
        .content
        .params
        .insert("limit".into(), NotificationScalar::Number(f64::NAN));
    assert!(matches!(
        scalar_store.save(&invalid_scalar),
        Err(AppError::Storage(_))
    ));

    let enum_root = test_root("invalid-enum");
    let enum_path = store_path(&enum_root);
    let mut invalid_enum = serde_json::to_value(sample_file(1)).unwrap();
    invalid_enum["partitions"][0]["items"][0]["severity"] =
        serde_json::Value::String("catastrophic".into());
    write_json(&enum_path, &invalid_enum);
    let outcome = NotificationStore::with_path(enum_path).load().unwrap();
    assert_eq!(outcome.status, NotificationLoadStatus::ResetFromCorruption);

    cleanup(&timestamp_root);
    cleanup(&scalar_root);
    cleanup(&enum_root);
}

#[test]
fn record_scope_must_match_its_partition() {
    let root = test_root("scope-mismatch");
    let store = NotificationStore::with_path(store_path(&root));
    let mut file = sample_file(1);
    file.partitions[0].items[0].scope = NotificationScope::Account {
        account_id: "account_1".into(),
    };

    assert!(matches!(store.save(&file), Err(AppError::Storage(_))));
    cleanup(&root);
}

#[test]
fn successful_save_rotates_main_to_backup() {
    let root = test_root("main-rotation");
    let path = store_path(&root);
    let store = NotificationStore::with_path(path.clone());

    store.save(&sample_file(1)).unwrap();
    store.save(&sample_file(2)).unwrap();

    assert_eq!(read_file(&path).revision, 2);
    assert_eq!(
        read_file(&NotificationStore::backup_path_for_test(&path)).revision,
        1
    );
    assert!(!NotificationStore::temp_path_for_test(&path).exists());
    cleanup(&root);
}

#[test]
fn tmp_promotion_failure_restores_the_backup() {
    let root = test_root("promotion-failure");
    let path = store_path(&root);
    NotificationStore::with_path(path.clone())
        .save(&sample_file(1))
        .unwrap();
    let failing =
        NotificationStore::with_path_and_failures(path.clone(), vec![FailurePoint::PromoteTemp]);

    assert_storage_unavailable(failing.save(&sample_file(2)).unwrap_err());

    assert_eq!(read_file(&path).revision, 1);
    let outcome = NotificationStore::with_path(path).load().unwrap();
    assert_eq!(outcome.status, NotificationLoadStatus::Clean);
    assert_eq!(outcome.file.revision, 1);
    cleanup(&root);
}

#[test]
fn backup_restore_failure_keeps_recovery_candidates() {
    let root = test_root("restore-failure");
    let path = store_path(&root);
    NotificationStore::with_path(path.clone())
        .save(&sample_file(1))
        .unwrap();
    let failing = NotificationStore::with_path_and_failures(
        path.clone(),
        vec![FailurePoint::PromoteTemp, FailurePoint::RestoreBackup],
    );

    assert_storage_unavailable(failing.save(&sample_file(2)).unwrap_err());

    assert!(!path.exists());
    assert_eq!(
        read_file(&NotificationStore::backup_path_for_test(&path)).revision,
        1
    );
    assert_eq!(
        read_file(&NotificationStore::temp_path_for_test(&path)).revision,
        2
    );
    let recovered = NotificationStore::with_path(path).load().unwrap();
    assert_eq!(recovered.file.revision, 2);
    assert_eq!(
        recovered.status,
        NotificationLoadStatus::Recovered {
            source: RecoverySource::Temp
        }
    );
    cleanup(&root);
}

#[test]
fn unreadable_main_returns_unavailable_without_falling_back() {
    let root = test_root("unreadable-main");
    let path = store_path(&root);
    fs::create_dir_all(&path).unwrap();
    write_json(
        &NotificationStore::backup_path_for_test(&path),
        &sample_file(4),
    );

    assert_storage_unavailable(
        NotificationStore::with_path(path.clone())
            .load()
            .unwrap_err(),
    );

    assert!(path.is_dir());
    assert_eq!(
        read_file(&NotificationStore::backup_path_for_test(&path)).revision,
        4
    );
    cleanup(&root);
}

#[test]
fn evidence_preservation_failure_returns_unavailable_and_keeps_candidate() {
    let root = test_root("evidence-failure");
    let path = store_path(&root);
    let corrupt_bytes = b"corrupt-main-must-remain";
    fs::create_dir_all(&root).unwrap();
    fs::write(&path, corrupt_bytes).unwrap();
    let failing = NotificationStore::with_path_and_failures(
        path.clone(),
        vec![FailurePoint::PreserveCorrupt],
    );

    assert_storage_unavailable(failing.load().unwrap_err());

    assert_eq!(fs::read(&path).unwrap(), corrupt_bytes);
    assert_eq!(
        fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
            .count(),
        0
    );
    cleanup(&root);
}

#[test]
fn no_file_startup_returns_clean_empty_state_without_writing() {
    let root = test_root("no-files");
    let path = store_path(&root);

    let outcome = NotificationStore::with_path(path.clone()).load().unwrap();

    assert_eq!(outcome.status, NotificationLoadStatus::Clean);
    assert_eq!(outcome.file, NotificationFileV1::empty());
    assert!(!path.exists());
    assert!(!root.exists());
}

#[test]
fn recovered_main_is_normalized_without_changing_revision() {
    for (label, source) in [
        ("temp", RecoverySource::Temp),
        ("backup", RecoverySource::Backup),
    ] {
        let root = test_root(label);
        let path = store_path(&root);
        let candidate_path = match source {
            RecoverySource::Temp => NotificationStore::temp_path_for_test(&path),
            RecoverySource::Backup => NotificationStore::backup_path_for_test(&path),
        };
        write_json(&candidate_path, &sample_file(9));

        let outcome = NotificationStore::with_path(path.clone()).load().unwrap();

        assert_eq!(outcome.status, NotificationLoadStatus::Recovered { source });
        assert_eq!(outcome.file.revision, 9);
        assert_eq!(read_file(&path).revision, 9);
        cleanup(&root);
    }
}

#[test]
fn normalization_failure_still_returns_the_recovered_candidate() {
    let root = test_root("normalization-failure");
    let path = store_path(&root);
    write_json(
        &NotificationStore::backup_path_for_test(&path),
        &sample_file(11),
    );
    let failing =
        NotificationStore::with_path_and_failures(path.clone(), vec![FailurePoint::PromoteTemp]);

    let outcome = failing.load().unwrap();

    assert_eq!(outcome.file.revision, 11);
    assert_eq!(
        outcome.status,
        NotificationLoadStatus::Recovered {
            source: RecoverySource::Backup
        }
    );
    assert_eq!(read_file(&path).revision, 11);
    cleanup(&root);
}

#[test]
fn content_map_round_trip_is_not_reordered_or_dropped() {
    let root = test_root("content-map");
    let store = NotificationStore::with_path(store_path(&root));
    let mut file = sample_file(5);
    file.partitions[0].items[0].content.params = BTreeMap::from([(
        "orderId".into(),
        NotificationScalar::String("order-42".into()),
    )]);

    store.save(&file).unwrap();

    assert_eq!(store.load().unwrap().file, file);
    cleanup(&root);
}

#[test]
fn legacy_file_without_source_event_index_loads_with_an_empty_index() {
    let root = test_root("legacy-source-index");
    let path = store_path(&root);
    let mut value = serde_json::to_value(sample_file(3)).unwrap();
    value.as_object_mut().unwrap().remove("sourceEventIndex");
    write_json(&path, &value);

    let outcome = NotificationStore::with_path(path).load().unwrap();

    assert!(outcome.file.source_event_index.is_empty());
    cleanup(&root);
}

#[test]
fn source_event_index_rejects_duplicates_dangling_cross_scope_and_unsafe_ids() {
    let root = test_root("invalid-source-index");
    let store = NotificationStore::with_path(store_path(&root));
    let mut base = sample_file(3);
    let record_id = base.partitions[0].items[0].id.clone();
    let valid = NotificationSourceEventIndexEntry {
        scope: NotificationScope::Global,
        source_event_id: "source-1".into(),
        notification_id: record_id.clone(),
    };

    for index in [
        vec![valid.clone(), valid.clone()],
        vec![NotificationSourceEventIndexEntry {
            notification_id: "00000000-0000-4000-8000-000000000099".into(),
            ..valid.clone()
        }],
        vec![NotificationSourceEventIndexEntry {
            scope: NotificationScope::Account {
                account_id: "alpha".into(),
            },
            ..valid.clone()
        }],
        vec![NotificationSourceEventIndexEntry {
            source_event_id: "raw server body".into(),
            ..valid.clone()
        }],
        vec![NotificationSourceEventIndexEntry {
            source_event_id: String::new(),
            ..valid.clone()
        }],
    ] {
        base.source_event_index = index;
        assert!(store.save(&base).is_err());
    }
    cleanup(&root);
}

#[test]
fn records_reject_duplicate_creating_sources_and_misdirected_creation_index_entries() {
    let root = test_root("record-source-uniqueness");
    let store = NotificationStore::with_path(store_path(&root));
    let mut duplicate = sample_file(1);
    let mut second = sample_record(
        "00000000-0000-4000-8000-000000000002",
        NotificationScope::Global,
        200,
    );
    second.source_event_id = duplicate.partitions[0].items[0].source_event_id.clone();
    duplicate.partitions[0].items.push(second);
    assert!(store.save(&duplicate).is_err());

    let mut misdirected = sample_file(1);
    let second = sample_record(
        "00000000-0000-4000-8000-000000000002",
        NotificationScope::Global,
        200,
    );
    let wrong_target = second.id.clone();
    let creating_source = misdirected.partitions[0].items[0]
        .source_event_id
        .clone()
        .unwrap();
    misdirected.partitions[0].items.push(second);
    misdirected.source_event_index = vec![NotificationSourceEventIndexEntry {
        scope: NotificationScope::Global,
        source_event_id: creating_source,
        notification_id: wrong_target,
    }];
    assert!(store.save(&misdirected).is_err());
    cleanup(&root);
}

#[test]
fn unsafe_legacy_record_source_is_corruption_but_does_not_poison_the_next_save() {
    let root = test_root("unsafe-legacy-source");
    let path = store_path(&root);
    let store = NotificationStore::with_path(path.clone());
    let mut value = serde_json::to_value(sample_file(3)).unwrap();
    value.as_object_mut().unwrap().remove("sourceEventIndex");
    value["partitions"][0]["items"][0]["sourceEventId"] =
        serde_json::Value::String("event:AKIAIOSFODNN7EXAMPLE".into());
    write_json(&path, &value);

    let outcome = store.load().unwrap();
    assert_eq!(outcome.status, NotificationLoadStatus::ResetFromCorruption);
    assert_eq!(outcome.file, NotificationFileV1::empty());
    assert!(!path.exists());

    let replacement = sample_file(4);
    store.save(&replacement).unwrap();
    assert_eq!(store.load().unwrap().file, replacement);
    cleanup(&root);
}

#[test]
fn legacy_file_with_duplicate_creating_sources_is_corruption() {
    let root = test_root("duplicate-legacy-source");
    let path = store_path(&root);
    let mut file = sample_file(3);
    let mut duplicate = sample_record(
        "00000000-0000-4000-8000-000000000002",
        NotificationScope::Global,
        200,
    );
    duplicate.source_event_id = file.partitions[0].items[0].source_event_id.clone();
    file.partitions[0].items.push(duplicate);
    let mut value = serde_json::to_value(file).unwrap();
    value.as_object_mut().unwrap().remove("sourceEventIndex");
    write_json(&path, &value);

    let outcome = NotificationStore::with_path(path).load().unwrap();

    assert_eq!(outcome.status, NotificationLoadStatus::ResetFromCorruption);
    assert_eq!(outcome.file, NotificationFileV1::empty());
    cleanup(&root);
}

#[test]
fn source_index_limits_accept_the_boundary_and_reject_scope_and_total_plus_one() {
    let scope_root = test_root("source-index-scope-limit");
    let scope_store = NotificationStore::with_path(store_path(&scope_root));
    let mut scope_file = source_index_file(&[10_000], 0);
    scope_store.save(&scope_file).unwrap();
    scope_file
        .source_event_index
        .push(NotificationSourceEventIndexEntry {
            scope: scope_file.partitions[0].scope.clone(),
            source_event_id: "semantic-over-scope-limit".into(),
            notification_id: scope_file.partitions[0].items[0].id.clone(),
        });
    assert!(scope_store.save(&scope_file).is_err());
    cleanup(&scope_root);

    let total_root = test_root("source-index-total-limit");
    let total_store = NotificationStore::with_path(store_path(&total_root));
    let mut total_file = source_index_file(&[10_000, 10_000, 10_000, 10_000, 10_000, 0], 0);
    total_store.save(&total_file).unwrap();
    total_file
        .source_event_index
        .push(NotificationSourceEventIndexEntry {
            scope: total_file.partitions[5].scope.clone(),
            source_event_id: "semantic-over-total-limit".into(),
            notification_id: total_file.partitions[5].items[0].id.clone(),
        });
    assert!(total_store.save(&total_file).is_err());
    cleanup(&total_root);
}

#[test]
fn serialized_file_cap_rejects_large_saves_and_loads_as_corruption() {
    let save_root = test_root("serialized-save-cap");
    let save_store = NotificationStore::with_path(store_path(&save_root));
    let large = source_index_file(&[10_000, 10_000, 10_000, 10_000, 10_000], 230);
    assert!(save_store.save(&large).is_err());
    cleanup(&save_root);

    let load_root = test_root("serialized-load-cap");
    let path = store_path(&load_root);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut boundary = serde_json::to_vec(&NotificationFileV1::empty()).unwrap();
    boundary.resize(MAX_NOTIFICATION_FILE_BYTES, b' ');
    fs::write(&path, &boundary).unwrap();
    let store = NotificationStore::with_path(path.clone());
    let outcome = store.load().unwrap();
    assert_eq!(outcome.status, NotificationLoadStatus::Clean);
    assert_eq!(outcome.file, NotificationFileV1::empty());

    boundary.push(b' ');
    fs::write(&path, boundary).unwrap();
    let outcome = store.load().unwrap();
    assert_eq!(outcome.status, NotificationLoadStatus::ResetFromCorruption);
    assert_eq!(outcome.file, NotificationFileV1::empty());
    cleanup(&load_root);
}

#[test]
fn oversized_future_main_and_temp_are_detected_without_rename_or_overwrite() {
    for label in ["main", "temp"] {
        let root = test_root(&format!("oversized-future-{label}"));
        let path = store_path(&root);
        let candidate_path = if label == "main" {
            path.clone()
        } else {
            NotificationStore::temp_path_for_test(&path)
        };
        fs::create_dir_all(candidate_path.parent().unwrap()).unwrap();
        let mut future = br#"{"schemaVersion":2,"futureData":""#.to_vec();
        future.resize(MAX_NOTIFICATION_FILE_BYTES, b'x');
        future.extend_from_slice(br#""}"#);
        assert!(future.len() > MAX_NOTIFICATION_FILE_BYTES);
        fs::write(&candidate_path, &future).unwrap();
        let store = NotificationStore::with_path(path.clone());

        let outcome = store.load().unwrap();

        assert_eq!(
            outcome.status,
            NotificationLoadStatus::UnsupportedSchema { found: 2 }
        );
        assert_eq!(fs::read(&candidate_path).unwrap(), future);
        assert!(store.save(&sample_file(1)).is_err());
        assert_eq!(fs::read(&candidate_path).unwrap(), future);
        assert_eq!(
            fs::read_dir(&root)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
                .count(),
            0
        );
        cleanup(&root);
    }
}

#[test]
fn oversized_candidate_with_unrecognized_schema_prefix_is_preserved_and_blocks_saves() {
    let root = test_root("oversized-unknown-schema");
    let path = store_path(&root);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = vec![b' '; MAX_NOTIFICATION_FILE_BYTES];
    bytes.extend_from_slice(br#"{"schemaVersion":2}"#);
    fs::write(&path, &bytes).unwrap();
    let store = NotificationStore::with_path(path.clone());

    assert_storage_unavailable(store.load().unwrap_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(store.save(&sample_file(1)).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(
        fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
            .count(),
        0
    );
    cleanup(&root);
}

#[test]
fn candidate_growth_after_metadata_check_uses_safe_prefix_classification() {
    for (label, bytes, expected_future) in [
        (
            "future",
            {
                let mut bytes = br#"{"schemaVersion":2,"futureData":""#.to_vec();
                bytes.resize(MAX_NOTIFICATION_FILE_BYTES + 1, b'x');
                bytes
            },
            Some(2),
        ),
        ("unknown", vec![b' '; MAX_NOTIFICATION_FILE_BYTES + 1], None),
    ] {
        let root = test_root(&format!("candidate-growth-{label}"));
        let path = store_path(&root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &bytes).unwrap();
        let candidate = read_candidate_from_open_file(
            fs::File::open(&path).unwrap(),
            MAX_NOTIFICATION_FILE_BYTES as u64,
        )
        .unwrap();
        match (candidate, expected_future) {
            (Candidate::Future(found), Some(expected)) => assert_eq!(found, expected),
            (Candidate::OversizedUnknown, None) => {}
            _ => panic!("grown candidate was not classified safely"),
        }

        let store = NotificationStore::with_path(path.clone());
        if let Some(found) = expected_future {
            assert_eq!(
                store.load().unwrap().status,
                NotificationLoadStatus::UnsupportedSchema { found }
            );
        } else {
            assert_storage_unavailable(store.load().unwrap_err());
        }
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(store.save(&sample_file(1)).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        cleanup(&root);
    }
}

#[test]
fn oversized_prefix_uses_the_first_root_schema_version_even_if_a_later_key_is_future() {
    let root = test_root("oversized-duplicate-schema");
    let path = store_path(&root);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = br#"{"schemaVersion":1,"schemaVersion":2,"padding":""#.to_vec();
    bytes.resize(MAX_NOTIFICATION_FILE_BYTES + 1, b'x');
    fs::write(&path, &bytes).unwrap();

    let candidate = read_candidate_from_open_file(
        fs::File::open(&path).unwrap(),
        MAX_NOTIFICATION_FILE_BYTES as u64,
    )
    .unwrap();

    assert!(matches!(candidate, Candidate::Corrupt));
    cleanup(&root);
}
