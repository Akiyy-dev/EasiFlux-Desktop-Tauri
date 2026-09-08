use super::*;
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
