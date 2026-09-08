use super::*;

#[test]
fn prepare_allocates_one_absent_remove_slot() {
    let (_temp, root, record, bytes) = fixture();
    let import = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
    let promoted = import
        .prepare_stage(&record, &bytes)
        .unwrap()
        .promote()
        .unwrap();
    let storage = SystemLocalPluginPackageStorage::with_plugins_root(root.clone());
    let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
    assert!(!root
        .join("removal-staging")
        .join(removal.removal_slot().as_str())
        .exists());
    let QuarantineRenameOutcome::Committed(evidence) = removal.quarantine_once() else {
        panic!("quarantine must be verified");
    };
    assert_eq!(&evidence.removal_slot, removal.removal_slot());
    assert!(matches!(removal.cleanup_once(), CleanupOutcome::Removed));
    assert!(children(&root.join("local")).is_empty());
    assert!(children(&root.join("removal-staging")).is_empty());
}

fn removal_fixture() -> (
    tempfile::TempDir,
    PathBuf,
    PromotedManagedPackage,
    SystemLocalPluginPackageStorage,
) {
    let (temp, root, record, bytes) = fixture();
    let promoted = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
        .prepare_stage(&record, &bytes)
        .unwrap()
        .promote()
        .unwrap();
    let storage = SystemLocalPluginPackageStorage::with_plugins_root(root.clone());
    (temp, root, promoted, storage)
}

#[test]
fn prepare_requires_exact_receipt_manifest_and_three_entry_identities() {
    for drift in 0..6 {
        let (_temp, root, promoted, storage) = removal_fixture();
        let mut entry = promoted.entry.clone();
        match drift {
            0 => entry.directory_identity.object += 1,
            1 => entry.manifest_identity.object += 1,
            2 => entry.receipt_identity.object += 1,
            3 => fs::write(
                root.join("local")
                    .join(entry.package_slot().as_str())
                    .join("manifest.json"),
                b"{}",
            )
            .unwrap(),
            4 => fs::write(
                root.join("local")
                    .join(entry.package_slot().as_str())
                    .join("ownership-receipt.json"),
                b"{}",
            )
            .unwrap(),
            _ => fs::write(
                root.join("local")
                    .join(entry.package_slot().as_str())
                    .join("extra"),
                b"retain",
            )
            .unwrap(),
        }
        assert!(matches!(
            storage.prepare(&promoted.locator, &entry),
            Err(RemovalStorageFailure::IdentityChanged)
        ));
    }
}

#[test]
fn prepare_counts_all_17_removal_entries_and_initial_collision_is_unavailable() {
    for count in [16, 17] {
        let (_temp, root, promoted, storage) = removal_fixture();
        for n in 0..count {
            fs::write(
                root.join("removal-staging").join(format!("unknown-{n}")),
                b"retain",
            )
            .unwrap();
        }
        assert!(matches!(
            storage.prepare(&promoted.locator, &promoted.entry),
            Err(RemovalStorageFailure::StagingCapacityExceeded)
        ));
    }
    let (_temp, root, promoted, mut storage) = removal_fixture();
    let uuid = uuid::Uuid::new_v4();
    storage.hooks.controls.uuid = Some(Arc::new(move || uuid));
    fs::write(
        root.join("removal-staging")
            .join(format!("remove-{}", uuid.simple())),
        b"retain",
    )
    .unwrap();
    assert!(matches!(
        storage.prepare(&promoted.locator, &promoted.entry),
        Err(RemovalStorageFailure::Unavailable)
    ));
}

#[test]
fn native_success_then_every_post_step_failure_is_unconfirmed_and_cannot_cleanup() {
    for point in [
        RemovalFsStep::AfterRename,
        RemovalFsStep::SyncSource,
        RemovalFsStep::SyncTarget,
        RemovalFsStep::ReopenTarget,
        RemovalFsStep::ReadManifest,
        RemovalFsStep::ReadReceipt,
        RemovalFsStep::VerifyIdentities,
    ] {
        let (_temp, root, promoted, mut storage) = removal_fixture();
        storage.hooks.controls.removal_hook = Some(Arc::new(move |step| {
            if step == point {
                Err(io::ErrorKind::PermissionDenied.into())
            } else {
                Ok(())
            }
        }));
        let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
        assert!(
            matches!(
                removal.quarantine_once(),
                QuarantineRenameOutcome::CommitUnconfirmed
            ),
            "{point:?}"
        );
        assert!(matches!(
            removal.quarantine_once(),
            QuarantineRenameOutcome::CommitUnconfirmed
        ));
        assert_eq!(removal.cleanup_once(), CleanupOutcome::Conflict);
        assert_eq!(removal.cleanup_once(), CleanupOutcome::Conflict);
        assert!(children(&root.join("local")).is_empty());
        assert_eq!(children(&only(&root.join("removal-staging"))).len(), 2);
    }
}

#[test]
fn cleanup_interruptions_are_monotonic_and_second_calls_do_not_mutate() {
    for (point, shape) in [
        (RemovalFsStep::BeforeCleanup, KnownCleanupShape::Full),
        (
            RemovalFsStep::BeforeManifestRemoval,
            KnownCleanupShape::Full,
        ),
        (
            RemovalFsStep::AfterManifestRemoval,
            KnownCleanupShape::ReceiptOnly,
        ),
        (
            RemovalFsStep::BeforeReceiptRemoval,
            KnownCleanupShape::ReceiptOnly,
        ),
        (
            RemovalFsStep::AfterReceiptRemoval,
            KnownCleanupShape::EmptyDirectory,
        ),
        (
            RemovalFsStep::BeforeDirectoryRemoval,
            KnownCleanupShape::EmptyDirectory,
        ),
        (
            RemovalFsStep::AfterDirectoryRemoval,
            KnownCleanupShape::BothAbsent,
        ),
        (RemovalFsStep::SyncCleanup, KnownCleanupShape::BothAbsent),
    ] {
        let (_temp, root, promoted, mut storage) = removal_fixture();
        storage.hooks.controls.removal_hook = Some(Arc::new(move |step| {
            if step == point {
                Err(io::ErrorKind::PermissionDenied.into())
            } else {
                Ok(())
            }
        }));
        let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
        assert!(matches!(
            removal.quarantine_once(),
            QuarantineRenameOutcome::Committed(_)
        ));
        assert_eq!(removal.cleanup_once(), CleanupOutcome::Pending(shape));
        assert_eq!(removal.cleanup_once(), CleanupOutcome::Conflict);
        assert!(matches!(
            removal.quarantine_once(),
            QuarantineRenameOutcome::CommitUnconfirmed
        ));
        let target = root
            .join("removal-staging")
            .join(removal.removal_slot().as_str());
        match shape {
            KnownCleanupShape::Full => assert_eq!(children(&target).len(), 2),
            KnownCleanupShape::ReceiptOnly => assert_eq!(
                children(&target),
                vec![target.join("ownership-receipt.json")]
            ),
            KnownCleanupShape::EmptyDirectory => assert!(children(&target).is_empty()),
            KnownCleanupShape::BothAbsent => assert!(!target.exists()),
        }
    }
}

#[test]
fn adjacent_collision_stops_precommit_native_collision_requires_exact_source() {
    for native in [false, true] {
        let (_temp, root, promoted, mut storage) = removal_fixture();
        let injected = root.clone();
        let uuid = uuid::Uuid::new_v4();
        storage.hooks.controls.uuid = Some(Arc::new(move || uuid));
        storage.hooks.controls.removal_hook = Some(Arc::new(move |step| {
            if step
                == if native {
                    RemovalFsStep::BeforeNativeRename
                } else {
                    RemovalFsStep::BeforeReverify
                }
            {
                fs::create_dir(
                    injected
                        .join("removal-staging")
                        .join(format!("remove-{}", uuid.simple())),
                )?;
            }
            Ok(())
        }));
        let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
        let expected = if native {
            RemovalStorageFailure::ProvenWriteFailure
        } else {
            RemovalStorageFailure::IdentityChanged
        };
        assert!(
            matches!(removal.quarantine_once(), QuarantineRenameOutcome::ProvenNotCommitted(value) if value == expected)
        );
        assert!(matches!(
            removal.quarantine_once(),
            QuarantineRenameOutcome::CommitUnconfirmed
        ));
        assert_eq!(removal.cleanup_once(), CleanupOutcome::Conflict);
        assert_eq!(children(&root.join("local")).len(), 1);
    }
}

#[test]
fn every_non_collision_native_error_is_uncertain() {
    for error in [
        io::ErrorKind::AlreadyExists,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::NotFound,
        io::ErrorKind::Unsupported,
        io::ErrorKind::Other,
    ] {
        let (_temp, root, promoted, mut storage) = removal_fixture();
        storage.hooks.controls.rename_error = Some(error);
        let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
        assert!(matches!(
            removal.quarantine_once(),
            QuarantineRenameOutcome::CommitUnconfirmed
        ));
        assert_eq!(removal.cleanup_once(), CleanupOutcome::Conflict);
        assert_eq!(children(&root.join("local")).len(), 1);
    }
}

#[test]
fn manifest_only_extra_or_replaced_child_is_cleanup_conflict() {
    for drift in 0..3 {
        let (_temp, root, promoted, storage) = removal_fixture();
        let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
        assert!(matches!(
            removal.quarantine_once(),
            QuarantineRenameOutcome::Committed(_)
        ));
        let target = root
            .join("removal-staging")
            .join(removal.removal_slot().as_str());
        match drift {
            0 => fs::remove_file(target.join("ownership-receipt.json")).unwrap(),
            1 => fs::write(target.join("extra"), b"retain").unwrap(),
            _ => {
                fs::rename(target.join("manifest.json"), root.join("old-manifest")).unwrap();
                fs::write(target.join("manifest.json"), b"replacement").unwrap();
            }
        }
        assert_eq!(removal.cleanup_once(), CleanupOutcome::Conflict);
        assert!(target.join("manifest.json").exists());
    }
}

#[test]
fn fresh_read_only_verification_cannot_authorize_uncertain_cleanup() {
    let (_temp, root, promoted, mut storage) = removal_fixture();
    storage.hooks.controls.removal_hook = Some(Arc::new(|step| {
        if step == RemovalFsStep::AfterRename {
            Err(io::ErrorKind::Other.into())
        } else {
            Ok(())
        }
    }));
    let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
    assert_eq!(removal.reverify_before_rename(), Ok(()));
    assert!(matches!(
        removal.quarantine_once(),
        QuarantineRenameOutcome::CommitUnconfirmed
    ));
    let evidence = removal.verify_quarantine().unwrap();
    assert_eq!(&evidence.removal_slot, removal.removal_slot());
    assert!(evidence.entry.removal_slot().is_some());
    assert_eq!(removal.cleanup_once(), CleanupOutcome::Conflict);
    assert_eq!(children(&only(&root.join("removal-staging"))).len(), 2);
}

#[test]
fn successful_calls_and_failed_precheck_consume_mutation_budget_before_io() {
    for stop in [false, true] {
        let (_temp, root, promoted, mut storage) = removal_fixture();
        let events = Arc::new(Mutex::new(vec![]));
        let observed = events.clone();
        let uuid_calls = Arc::new(AtomicUsize::new(0));
        let generated = uuid_calls.clone();
        storage.hooks.controls.uuid = Some(Arc::new(move || {
            generated.fetch_add(1, Ordering::SeqCst);
            uuid::Uuid::new_v4()
        }));
        storage.hooks.controls.removal_hook = Some(Arc::new(move |step| {
            observed.lock().unwrap().push(step);
            if stop && step == RemovalFsStep::BeforeReverify {
                Err(io::ErrorKind::Other.into())
            } else {
                Ok(())
            }
        }));
        let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
        if stop {
            assert!(matches!(
                removal.quarantine_once(),
                QuarantineRenameOutcome::ProvenNotCommitted(_)
            ));
        } else {
            assert!(matches!(
                removal.quarantine_once(),
                QuarantineRenameOutcome::Committed(_)
            ));
            assert_eq!(removal.cleanup_once(), CleanupOutcome::Removed);
        }
        let original_events = events.lock().unwrap().clone();
        assert!(matches!(
            removal.quarantine_once(),
            QuarantineRenameOutcome::CommitUnconfirmed
        ));
        assert_eq!(removal.cleanup_once(), CleanupOutcome::Conflict);
        assert_eq!(*events.lock().unwrap(), original_events);
        assert_eq!(uuid_calls.load(Ordering::SeqCst), 1);
        drop(removal);
        assert_eq!(*events.lock().unwrap(), original_events);
        assert_eq!(children(&root.join("local")).len(), usize::from(stop));
    }
}

#[test]
fn postrename_reopen_reparses_both_files_and_three_identities() {
    for drift in 0..5 {
        let (_temp, root, promoted, mut storage) = removal_fixture();
        let injected = root.clone();
        storage.hooks.controls.removal_hook = Some(Arc::new(move |step| {
            if step == RemovalFsStep::AfterRename {
                let target = only(&injected.join("removal-staging"));
                if drift < 2 {
                    fs::write(
                        target.join(if drift == 0 {
                            "manifest.json"
                        } else {
                            "ownership-receipt.json"
                        }),
                        b"{}",
                    )?;
                } else if drift < 4 {
                    let path = target.join(if drift == 2 {
                        "manifest.json"
                    } else {
                        "ownership-receipt.json"
                    });
                    let bytes = fs::read(&path)?;
                    fs::rename(&path, injected.join("original"))?;
                    fs::write(path, bytes)?;
                } else {
                    fs::rename(&target, injected.join("original"))?;
                    fs::create_dir(&target)?;
                    for name in ["manifest.json", "ownership-receipt.json"] {
                        fs::copy(injected.join("original").join(name), target.join(name))?;
                    }
                }
            }
            Ok(())
        }));
        let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
        assert!(matches!(
            removal.quarantine_once(),
            QuarantineRenameOutcome::CommitUnconfirmed
        ));
        assert!(removal.verify_quarantine().is_err());
        assert_eq!(removal.cleanup_once(), CleanupOutcome::Conflict);
        assert_eq!(children(&only(&root.join("removal-staging"))).len(), 2);
    }
}

#[test]
fn child_change_at_final_reverify_stops_before_native_rename() {
    let (_temp, root, promoted, mut storage) = removal_fixture();
    let source = root
        .join("local")
        .join(promoted.entry.package_slot().as_str());
    let injected = source.clone();
    storage.hooks.controls.removal_hook = Some(Arc::new(move |step| {
        if step == RemovalFsStep::BeforeNativeRename {
            fs::write(injected.join("manifest.json"), b"{}")?;
        }
        Ok(())
    }));
    let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
    assert!(matches!(
        removal.quarantine_once(),
        QuarantineRenameOutcome::ProvenNotCommitted(RemovalStorageFailure::IdentityChanged)
    ));
    assert!(source.exists());
    assert!(children(&root.join("removal-staging")).is_empty());
}

#[cfg(windows)]
#[test]
fn windows_removal_source_and_child_name_swaps_do_not_redirect_held_mutations() {
    let (_temp, root, promoted, mut storage) = removal_fixture();
    let injected = root.clone();
    storage.hooks.controls.removal_hook = Some(Arc::new(move |step| {
        let original = injected.join("swapped");
        match step {
            RemovalFsStep::BeforeNativeRename => {
                assert!(fs::rename(only(&injected.join("local")), &original).is_err());
            }
            RemovalFsStep::BeforeManifestRemoval => {
                assert!(fs::rename(
                    only(&injected.join("removal-staging")).join("manifest.json"),
                    &original
                )
                .is_err());
            }
            RemovalFsStep::BeforeReceiptRemoval => {
                assert!(fs::rename(
                    only(&injected.join("removal-staging")).join("ownership-receipt.json"),
                    &original
                )
                .is_err());
            }
            _ => (),
        }
        Ok(())
    }));
    let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
    assert!(matches!(
        removal.quarantine_once(),
        QuarantineRenameOutcome::Committed(_)
    ));
    assert_eq!(removal.cleanup_once(), CleanupOutcome::Removed);
    assert!(!root.join("swapped").exists());
}

#[cfg(unix)]
#[test]
fn best_effort_adjacent_cleanup_checks_all_survivors_and_parent() {
    for parent_swap in [false, true] {
        let (_temp, root, promoted, mut storage) = removal_fixture();
        let injected = root.clone();
        storage.hooks.controls.removal_hook = Some(Arc::new(move |step| {
            if step == RemovalFsStep::BeforeManifestRemoval {
                let target = only(&injected.join("removal-staging"));
                if parent_swap {
                    fs::rename(&target, injected.join("original"))?;
                    fs::create_dir(&target)?;
                } else {
                    fs::rename(
                        target.join("ownership-receipt.json"),
                        injected.join("original"),
                    )?;
                    fs::write(target.join("ownership-receipt.json"), b"replacement")?;
                }
            }
            Ok(())
        }));
        let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
        assert!(matches!(
            removal.quarantine_once(),
            QuarantineRenameOutcome::Committed(_)
        ));
        assert_eq!(removal.cleanup_once(), CleanupOutcome::Conflict);
        let survivor = if parent_swap {
            root.join("original")
        } else {
            only(&root.join("removal-staging"))
        };
        assert!(survivor.join("manifest.json").exists());
    }
}

#[test]
fn cleanup_observes_known_prior_subsets_without_recreating_files() {
    for shape in [
        KnownCleanupShape::ReceiptOnly,
        KnownCleanupShape::EmptyDirectory,
        KnownCleanupShape::BothAbsent,
    ] {
        let (_temp, root, promoted, storage) = removal_fixture();
        let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
        assert!(matches!(
            removal.quarantine_once(),
            QuarantineRenameOutcome::Committed(_)
        ));
        let target = root
            .join("removal-staging")
            .join(removal.removal_slot().as_str());
        fs::remove_file(target.join("manifest.json")).unwrap();
        if shape != KnownCleanupShape::ReceiptOnly {
            fs::remove_file(target.join("ownership-receipt.json")).unwrap();
        }
        if shape == KnownCleanupShape::BothAbsent {
            fs::remove_dir(&target).unwrap();
        }
        assert_eq!(removal.cleanup_once(), CleanupOutcome::Removed);
        assert_eq!(removal.cleanup_once(), CleanupOutcome::Conflict);
        assert!(!target.exists());
    }
}

#[test]
fn premature_cleanup_consumes_request_without_later_mutation() {
    let (_temp, root, promoted, storage) = removal_fixture();
    let mut removal = storage.prepare(&promoted.locator, &promoted.entry).unwrap();
    assert_eq!(removal.cleanup_once(), CleanupOutcome::Conflict);
    assert!(matches!(
        removal.quarantine_once(),
        QuarantineRenameOutcome::CommitUnconfirmed
    ));
    assert_eq!(removal.cleanup_once(), CleanupOutcome::Conflict);
    assert_eq!(children(&root.join("local")).len(), 1);
    assert!(children(&root.join("removal-staging")).is_empty());
}
use crate::storage::local_plugin_import::ImportPromotionState;
use crate::{
    plugin::{import::test_support::VALID, record::PluginRecord},
    storage::local_plugin_import::{LocalManifestImportStorage, SystemLocalManifestImportStorage},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

fn fixture() -> (tempfile::TempDir, PathBuf, PluginRecord, Vec<u8>) {
    let base = std::env::current_dir().unwrap().join("target");
    let temp = tempfile::tempdir_in(base).unwrap();
    let root = temp.path().canonicalize().unwrap().join("plugins");
    let record = PluginRecord::local_declarative(serde_json::from_slice(VALID).unwrap()).unwrap();
    let bytes = record.canonical_manifest_bytes().unwrap();
    (temp, root, record, bytes)
}
fn children(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect()
}
fn only(root: &Path) -> PathBuf {
    let paths = children(root);
    assert_eq!(paths.len(), 1);
    paths[0].clone()
}
fn failure(step: ImportFsStep) -> TestControls {
    TestControls {
        hook: Some(Arc::new(move |actual| {
            if actual == step {
                Err(io::ErrorKind::PermissionDenied.into())
            } else {
                Ok(())
            }
        })),
        ..Default::default()
    }
}

#[test]
fn stage_writes_and_syncs_manifest_then_receipt_then_directories() {
    let (_temp, root, record, bytes) = fixture();
    let events = Arc::new(Mutex::new(Vec::new()));
    let collected = events.clone();
    let storage =
        SystemLocalManifestImportStorage::with_plugins_root(root).with_controls(TestControls {
            hook: Some(Arc::new(move |step| {
                collected.lock().unwrap().push(step);
                Ok(())
            })),
            ..Default::default()
        });
    let mut stage = storage.prepare_stage(&record, &bytes).unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            ImportFsStep::CreateStage,
            ImportFsStep::WriteManifest,
            ImportFsStep::SyncManifest,
            ImportFsStep::WriteReceipt,
            ImportFsStep::SyncReceipt,
            #[cfg(unix)]
            ImportFsStep::SyncStageDirectory,
            #[cfg(unix)]
            ImportFsStep::SyncStagingParent
        ]
    );
    stage.cleanup().unwrap();
}

#[test]
fn receipt_and_target_slot_are_host_generated() {
    let (_temp, root, record, bytes) = fixture();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root);
    let mut a = storage.prepare_stage(&record, &bytes).unwrap();
    let mut b = storage.prepare_stage(&record, &bytes).unwrap();
    let a = a.promote().unwrap();
    let b = b.promote().unwrap();
    let ar = &a.locator.receipt.as_ref().unwrap().model;
    let br = &b.locator.receipt.as_ref().unwrap().model;
    assert_ne!(ar.receipt_id(), br.receipt_id());
    assert_ne!(ar.package_slot(), br.package_slot());
    assert_eq!(
        uuid::Uuid::parse_str(ar.receipt_id().as_str())
            .unwrap()
            .get_version(),
        Some(uuid::Version::Random)
    );
    assert!(ar.matches_record(&record));
}

fn collision_attempts(collisions: usize) {
    let (_temp, root, record, bytes) = fixture();
    let snapshots = Arc::new(Mutex::new(Vec::new()));
    let observed = snapshots.clone();
    let injected = root.clone();
    let uuid_count = Arc::new(AtomicUsize::new(0));
    let count = uuid_count.clone();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone()).with_controls(
        TestControls {
            uuid: Some(Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                uuid::Uuid::new_v4()
            })),
            hook: Some(Arc::new(move |step| {
                if step == ImportFsStep::BeforePromotion {
                    let stage = only(&injected.join("import-staging"));
                    let receipt_bytes = fs::read(stage.join("ownership-receipt.json"))?;
                    let receipt =
                        crate::plugin::ownership::OwnershipReceiptV1::parse(&receipt_bytes)
                            .unwrap();
                    let mut observed = observed.lock().unwrap();
                    if observed.len() < collisions {
                        fs::create_dir(
                            injected.join("local").join(receipt.package_slot().as_str()),
                        )?;
                    }
                    observed.push((stage, receipt_bytes, receipt));
                }
                Ok(())
            })),
            ..Default::default()
        },
    );
    let mut stage = storage.prepare_stage(&record, &bytes).unwrap();
    assert_eq!(stage.promote().is_ok(), collisions < 4);
    let snapshots = snapshots.lock().unwrap();
    let total = (collisions + 1).min(4);
    assert_eq!(snapshots.len(), total);
    assert_eq!(uuid_count.load(Ordering::SeqCst), total * 3);
    for (i, (path, data, receipt)) in snapshots.iter().enumerate() {
        assert!(!path.exists());
        assert_eq!(*data, receipt.canonical_bytes().unwrap());
        for (other_path, _, other) in snapshots.iter().take(i) {
            assert_ne!(path, other_path);
            assert_ne!(receipt.receipt_id(), other.receipt_id());
            assert_ne!(receipt.package_slot(), other.package_slot());
        }
        if i < collisions {
            assert!(children(&root.join("local").join(receipt.package_slot().as_str())).is_empty());
        }
    }
    assert!(children(&root.join("import-staging")).is_empty());
}

#[test]
fn target_collision_rebuilds_entire_stage_and_receipt() {
    collision_attempts(1);
}
#[test]
fn four_complete_attempts_stop_without_reusing_or_mutating_receipt() {
    collision_attempts(4);
}

#[test]
fn stage_name_collision_is_bounded_by_the_same_four_attempts() {
    let (_temp, root, record, bytes) = fixture();
    fs::create_dir_all(root.join("import-staging")).unwrap();
    let ids: Vec<_> = (0..12).map(|_| uuid::Uuid::new_v4()).collect();
    for index in [0, 3, 6, 9] {
        fs::write(
            root.join("import-staging")
                .join(format!("stage-{}", ids[index].simple())),
            b"unknown",
        )
        .unwrap();
    }
    let count = Arc::new(AtomicUsize::new(0));
    let used = count.clone();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone()).with_controls(
        TestControls {
            uuid: Some(Arc::new(move || ids[used.fetch_add(1, Ordering::SeqCst)])),
            ..Default::default()
        },
    );
    assert!(storage.prepare_stage(&record, &bytes).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 12);
    for child in children(&root.join("import-staging")) {
        assert_eq!(fs::read(child).unwrap(), b"unknown");
    }
}

#[test]
fn partial_stage_cleanup_requires_every_surviving_recorded_identity() {
    for replace in [false, true] {
        let (_temp, root, record, bytes) = fixture();
        let injected = root.clone();
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
            .with_controls(TestControls {
                hook: Some(Arc::new(move |step| {
                    if step == ImportFsStep::WriteReceipt {
                        let stage = only(&injected.join("import-staging"));
                        fs::rename(
                            stage.join("manifest.json"),
                            injected.join("original-manifest"),
                        )?;
                        if replace {
                            fs::write(stage.join("manifest.json"), b"replacement")?;
                        }
                        return Err(io::ErrorKind::PermissionDenied.into());
                    }
                    Ok(())
                })),
                ..Default::default()
            });
        assert!(storage.prepare_stage(&record, &bytes).is_err());
        let stage = only(&root.join("import-staging"));
        assert!(stage.join("ownership-receipt.json").exists());
        assert_eq!(fs::read(root.join("original-manifest")).unwrap(), bytes);
        if replace {
            assert_eq!(
                fs::read(stage.join("manifest.json")).unwrap(),
                b"replacement"
            );
        }
    }
}

#[test]
fn receipt_write_or_sync_failure_cleans_only_exact_partial_stage() {
    for step in [ImportFsStep::WriteReceipt, ImportFsStep::SyncReceipt] {
        let (_temp, root, record, bytes) = fixture();
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
            .with_controls(failure(step));
        assert!(storage.prepare_stage(&record, &bytes).is_err());
        assert!(children(&root.join("import-staging")).is_empty());
    }
}

#[test]
fn extra_file_or_replaced_object_blocks_cleanup() {
    for changed in [
        "extra",
        "manifest.json",
        "ownership-receipt.json",
        "directory",
    ] {
        let (_temp, root, record, bytes) = fixture();
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
        let mut stage = storage.prepare_stage(&record, &bytes).unwrap();
        let path = only(&root.join("import-staging"));
        match changed {
            "directory" => {
                fs::rename(&path, root.join("original")).unwrap();
                fs::create_dir(&path).unwrap();
            }
            "extra" => {
                fs::write(path.join("extra"), b"preserve").unwrap();
            }
            file => {
                fs::rename(path.join(file), root.join("original")).unwrap();
                fs::write(path.join(file), b"preserve").unwrap();
            }
        }
        assert!(stage.cleanup().is_err());
        assert!(stage.promote().is_err());
        assert!(path.exists());
        assert!(children(&root.join("local")).is_empty());
    }
}

#[test]
fn same_volume_is_required() {
    let a = FileIdentity {
        volume: 1,
        object: 1,
    };
    let b = FileIdentity {
        volume: 2,
        object: 2,
    };
    for index in 0..4 {
        let mut ids = [a; 4];
        ids[index] = b;
        assert_eq!(
            platform::require_same_volume(&ids).unwrap_err().kind(),
            io::ErrorKind::CrossesDevices
        );
    }
    let (_temp, root, _, _) = fixture();
    let parents = SystemLocalPluginPackageStorage::with_plugins_root(root)
        .open()
        .unwrap();
    assert_eq!(
        parents.local.identity().unwrap().volume,
        parents.removal.identity().unwrap().volume
    );
    assert_eq!(
        parents.local.identity().unwrap().volume,
        parents.staging.identity().unwrap().volume
    );
}

#[test]
fn postpromotion_reopen_compares_all_three_identities() {
    for changed in ["directory", "manifest.json", "ownership-receipt.json"] {
        let (_temp, root, record, bytes) = fixture();
        let injected = root.clone();
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
            .with_controls(TestControls {
                hook: Some(Arc::new(move |step| {
                    if step == ImportFsStep::AfterPromotion {
                        let path = only(&injected.join("local"));
                        if changed == "directory" {
                            fs::rename(&path, injected.join("original"))?;
                            fs::create_dir(&path)?;
                            for name in ["manifest.json", "ownership-receipt.json"] {
                                fs::copy(injected.join("original").join(name), path.join(name))?;
                            }
                        } else {
                            fs::rename(path.join(changed), injected.join("original"))?;
                            fs::copy(injected.join("original"), path.join(changed))?;
                        }
                    }
                    Ok(())
                })),
                ..Default::default()
            });
        let mut stage = storage.prepare_stage(&record, &bytes).unwrap();
        assert!(stage.promote().is_err());
        assert_eq!(stage.promotion_state(), ImportPromotionState::Committed);
        assert!(stage.cleanup().is_err());
        assert_eq!(children(&only(&root.join("local"))).len(), 2);
    }
}

#[test]
fn postpromotion_reparses_both_canonical_files_even_when_identity_is_unchanged() {
    for changed in ["manifest.json", "ownership-receipt.json"] {
        let (_temp, root, record, bytes) = fixture();
        let injected = root.clone();
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
            .with_controls(TestControls {
                hook: Some(Arc::new(move |step| {
                    if step == ImportFsStep::AfterPromotion {
                        let path = only(&injected.join("local")).join(changed);
                        let mut data = fs::read(&path)?;
                        data.push(b' '); // Semantically valid JSON, but no longer exact canonical bytes.
                        fs::write(path, data)?;
                    }
                    Ok(())
                })),
                ..Default::default()
            });
        let mut stage = storage.prepare_stage(&record, &bytes).unwrap();
        assert!(stage.promote().is_err());
        assert_eq!(stage.promotion_state(), ImportPromotionState::Committed);
        assert!(stage.cleanup().is_err());
        assert_eq!(children(&only(&root.join("local"))).len(), 2);
    }
}

#[test]
fn source_name_swap_at_held_rename_boundary_is_fail_closed() {
    let (_temp, root, record, bytes) = fixture();
    let injected = root.clone();
    let reached = Arc::new(AtomicUsize::new(0));
    let observed = reached.clone();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone()).with_controls(
        TestControls {
            hook: Some(Arc::new(move |step| {
                if step == ImportFsStep::BeforePromotion {
                    observed.fetch_add(1, Ordering::SeqCst);
                    let stage = only(&injected.join("import-staging"));
                    let moved = fs::rename(&stage, injected.join("original"));
                    #[cfg(windows)]
                    assert!(moved.is_err());
                    #[cfg(unix)]
                    {
                        moved?;
                        fs::create_dir(&stage)?;
                        fs::write(stage.join("manifest.json"), b"replacement")?;
                    }
                }
                Ok(())
            })),
            ..Default::default()
        },
    );
    let mut stage = storage.prepare_stage(&record, &bytes).unwrap();
    #[cfg(windows)]
    assert!(stage.promote().is_ok());
    #[cfg(unix)]
    {
        assert!(stage.promote().is_err());
        assert_eq!(stage.promotion_state(), ImportPromotionState::NotCommitted);
        assert!(stage.cleanup().is_err());
    }
    assert_eq!(reached.load(Ordering::SeqCst), 1);
}

#[test]
fn unsupported_and_cross_volume_rename_fail_closed_without_copy() {
    for kind in [io::ErrorKind::Unsupported, io::ErrorKind::CrossesDevices] {
        let (_temp, root, record, bytes) = fixture();
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
            .with_controls(TestControls {
                rename_error: Some(kind),
                ..Default::default()
            });
        let mut stage = storage.prepare_stage(&record, &bytes).unwrap();
        assert!(stage.promote().is_err());
        assert_eq!(stage.promotion_state(), ImportPromotionState::NotCommitted);
        assert!(children(&root.join("local")).is_empty());
        assert_eq!(children(&only(&root.join("import-staging"))).len(), 2);
        stage.cleanup().unwrap();
    }
}

#[test]
fn before_promotion_child_swap_stops_before_native_rename() {
    let (_temp, root, record, bytes) = fixture();
    let injected = root.clone();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone()).with_controls(
        TestControls {
            hook: Some(Arc::new(move |step| {
                if step == ImportFsStep::BeforePromotion {
                    let stage = only(&injected.join("import-staging"));
                    fs::rename(
                        stage.join("ownership-receipt.json"),
                        injected.join("original-receipt"),
                    )?;
                    fs::copy(
                        injected.join("original-receipt"),
                        stage.join("ownership-receipt.json"),
                    )?;
                }
                Ok(())
            })),
            ..Default::default()
        },
    );
    let mut stage = storage.prepare_stage(&record, &bytes).unwrap();
    assert!(stage.promote().is_err());
    assert_eq!(stage.promotion_state(), ImportPromotionState::NotCommitted);
    assert!(children(&root.join("local")).is_empty());
    assert!(stage.cleanup().is_err());
}

#[test]
fn built_in_record_is_rejected_before_creating_roots() {
    let (_temp, root, record, bytes) = fixture();
    let built_in = PluginRecord::built_in(record.manifest().clone()).unwrap();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
    assert!(storage.prepare_stage(&built_in, &bytes).is_err());
    assert!(!root.exists());
}

#[test]
fn failed_cleanup_is_terminal_even_if_shape_is_restored() {
    let (_temp, root, record, bytes) = fixture();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
    let mut stage = storage.prepare_stage(&record, &bytes).unwrap();
    let path = only(&root.join("import-staging"));
    fs::write(path.join("extra"), b"unknown").unwrap();
    assert!(stage.cleanup().is_err());
    fs::remove_file(path.join("extra")).unwrap();
    assert!(stage.cleanup().is_err());
    assert!(stage.promote().is_err());
    assert_eq!(children(&path).len(), 2);
}

#[test]
fn collision_cleanup_must_prove_stage_name_absent_before_retry() {
    let (_temp, root, record, bytes) = fixture();
    let injected = root.clone();
    let source_name = Arc::new(Mutex::new(None::<PathBuf>));
    let name = source_name.clone();
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone()).with_controls(
        TestControls {
            hook: Some(Arc::new(move |step| {
                if step == ImportFsStep::BeforePromotion {
                    count.fetch_add(1, Ordering::SeqCst);
                    let path = only(&injected.join("import-staging"));
                    let receipt = crate::plugin::ownership::OwnershipReceiptV1::parse(&fs::read(
                        path.join("ownership-receipt.json"),
                    )?)
                    .unwrap();
                    fs::create_dir(injected.join("local").join(receipt.package_slot().as_str()))?;
                    *name.lock().unwrap() = Some(path);
                }
                if step == ImportFsStep::AfterStageRemoval {
                    let path = name.lock().unwrap().clone().unwrap();
                    fs::create_dir(&path)?;
                    fs::write(path.join("unknown"), b"preserved")?;
                }
                Ok(())
            })),
            ..Default::default()
        },
    );
    let mut stage = storage.prepare_stage(&record, &bytes).unwrap();
    assert!(stage.promote().is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(stage.cleanup().is_err());
    let path = source_name.lock().unwrap().clone().unwrap();
    assert_eq!(fs::read(path.join("unknown")).unwrap(), b"preserved");
}

#[cfg(windows)]
#[test]
fn windows_native_exclusive_rename_never_replaces_target() {
    collision_attempts(4);
}
#[cfg(target_os = "linux")]
#[test]
fn linux_native_exclusive_rename_never_replaces_target() {
    collision_attempts(4);
}
#[cfg(target_os = "macos")]
#[test]
fn macos_native_exclusive_rename_never_replaces_target() {
    collision_attempts(4);
}

#[cfg(windows)]
#[test]
fn uncertain_native_collision_never_retries_or_cleans_source() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_temp, root, record, bytes) = fixture();
    let injected = root.clone();
    let held = Arc::new(Mutex::new(None));
    let pinned = held.clone();
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone()).with_controls(
        TestControls {
            hook: Some(Arc::new(move |step| {
                if step == ImportFsStep::BeforePromotion {
                    count.fetch_add(1, Ordering::SeqCst);
                    let source = only(&injected.join("import-staging"));
                    let receipt = crate::plugin::ownership::OwnershipReceiptV1::parse(&fs::read(
                        source.join("ownership-receipt.json"),
                    )?)
                    .unwrap();
                    let target = injected.join("local").join(receipt.package_slot().as_str());
                    fs::write(&target, b"occupied")?;
                    *pinned.lock().unwrap() = Some(
                        fs::OpenOptions::new()
                            .read(true)
                            .share_mode(0)
                            .open(target)?,
                    );
                }
                Ok(())
            })),
            ..Default::default()
        },
    );
    let mut stage = storage.prepare_stage(&record, &bytes).unwrap();
    assert!(stage.promote().is_err());
    assert_eq!(
        stage.promotion_state(),
        ImportPromotionState::CommitUnconfirmed
    );
    assert!(stage.cleanup().is_err());
    assert!(stage.promote().is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(children(&only(&root.join("import-staging"))).len(), 2);
    drop(held.lock().unwrap().take());
    assert_eq!(fs::read(only(&root.join("local"))).unwrap(), b"occupied");
}

#[test]
fn promoted_import_contains_exact_receipt_and_manifest_bound_to_entry() {
    let base = std::env::current_dir().unwrap().join("target");
    let temp = tempfile::tempdir_in(base).unwrap();
    let root = temp.path().canonicalize().unwrap().join("plugins");
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
    let record = PluginRecord::local_declarative(serde_json::from_slice(VALID).unwrap()).unwrap();
    let bytes = record.canonical_manifest_bytes().unwrap();
    let mut stage = storage.prepare_stage(&record, &bytes).unwrap();
    let promoted = stage.promote().unwrap();
    assert!(promoted.entry.matches_locator(&promoted.locator));
    let target = root
        .join("local")
        .join(promoted.locator.package_slot.as_str());
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 2);
    assert_eq!(std::fs::read(target.join("manifest.json")).unwrap(), bytes);
    assert!(root.join("removal-staging").is_dir());
}
