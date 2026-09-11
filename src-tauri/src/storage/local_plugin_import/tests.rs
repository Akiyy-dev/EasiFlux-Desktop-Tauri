use super::*;
use crate::plugin::import::test_support::VALID;
use crate::plugin::import::ImportCommitFailure;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::{fs, io, path::PathBuf};

// RED: the old importer writes only a manifest and cannot bind a host receipt.
#[test]
fn promoted_import_contains_exact_receipt_and_manifest_bound_to_entry() {
    let (_temp, root) = root();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
    let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
    stage.promote().unwrap();
    let target = children(&root.join("local")).pop().unwrap();
    assert_eq!(children(&target).len(), 2);
    let receipt = crate::plugin::ownership::OwnershipReceiptV1::parse(
        &fs::read(target.join("ownership-receipt.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        receipt.package_slot().as_str(),
        target.file_name().unwrap().to_str().unwrap()
    );
}

fn root() -> (tempfile::TempDir, PathBuf) {
    // Like discovery's fixtures, avoid the sandbox-restricted user-home chain.
    let base = std::env::current_dir().unwrap().join("target");
    fs::create_dir_all(&base).unwrap();
    let temp = tempfile::tempdir_in(base).unwrap();
    let root = temp.path().canonicalize().unwrap().join("plugins");
    (temp, root)
}

fn children(path: &std::path::Path) -> Vec<PathBuf> {
    fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect()
}

fn failing(step: ImportFsStep) -> TestControls {
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

fn supplied(ids: Vec<u128>) -> (TestControls, Arc<AtomicUsize>) {
    let attempts = Arc::new(AtomicUsize::new(0));
    let used = attempts.clone();
    (
        TestControls {
            uuid: Some(Arc::new(move || {
                fixture_uuid(ids[used.fetch_add(1, Ordering::SeqCst)])
            })),
            ..Default::default()
        },
        attempts,
    )
}

#[test]
fn storage_and_owned_stage_cross_worker_boundaries() {
    fn shared<T: Send + Sync>() {}
    fn movable<T: Send>() {}
    shared::<SystemLocalManifestImportStorage>();
    shared::<Box<dyn LocalManifestImportStorage>>();
    movable::<Box<dyn OwnedImportStage>>();
}

#[test]
fn fixed_parent_adapter_opens_its_created_roots() {
    let (_temp, root) = root();
    let parents = platform::ImportDirectories::open_or_create(&root).unwrap();
    assert_eq!(parents.staging_count(17).unwrap(), 0);
    assert!(root.join("local").is_dir());
}

#[test]
fn stage_is_invisible_until_exclusive_promotion() {
    let (_temp, root) = root();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
    assert!(!root.exists());
    let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
    assert!(children(&root.join("local")).is_empty());
    let pending = children(&root.join("import-staging"));
    assert_eq!(pending.len(), 1);
    assert_eq!(children(&pending[0]).len(), 2);
    assert_eq!(
        fs::read(pending[0].join("manifest.json")).unwrap(),
        canonical()
    );
    assert!(stage.promote().is_ok());
    let target = children(&root.join("local")).pop().unwrap();
    assert_eq!(fs::read(target.join("manifest.json")).unwrap(), canonical());
    assert!(children(&root.join("import-staging")).is_empty());
    assert!(stage.cleanup().is_err());
    assert!(stage.promote().is_err());
    assert!(target.exists());
}

#[test]
fn stage_cap_counts_unknown_entries() {
    let (_temp, root) = root();
    fs::create_dir_all(root.join("import-staging")).unwrap();
    for n in 0..16 {
        fs::write(root.join("import-staging").join(n.to_string()), b"unknown").unwrap();
    }
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
    assert_eq!(
        storage.prepare_stage(&record(), &canonical()).err(),
        Some(ImportCommitFailure::StagingCapacityExceeded)
    );
    let entries = children(&root.join("import-staging"));
    assert_eq!(entries.len(), 16);
    for entry in entries {
        assert_eq!(fs::read(entry).unwrap(), b"unknown");
    }
}

#[test]
fn four_target_collisions_never_replace() {
    let (_temp, root) = root();
    fs::create_dir_all(root.join("local")).unwrap();
    for n in 1..=4 {
        let target = root
            .join("local")
            .join(format!("pkg-{}", fixture_uuid(n).simple()));
        // Include an empty directory: ordinary Unix rename would replace it.
        if n == 1 {
            fs::create_dir(&target).unwrap();
        } else if n == 2 {
            fs::write(&target, b"occupied file").unwrap();
        } else {
            fs::create_dir(&target).unwrap();
            fs::write(target.join("unknown"), b"occupied").unwrap();
        }
    }
    let (controls, attempts) = supplied(vec![100, 200, 1, 101, 201, 2, 102, 202, 3, 103, 203, 4]);
    let storage =
        SystemLocalManifestImportStorage::with_plugins_root(root.clone()).with_controls(controls);
    let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
    assert_eq!(
        stage.promote().err(),
        Some(ImportCommitFailure::WriteFailed)
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 12); // four complete stage/receipt/slot attempts
    assert!(children(
        &root
            .join("local")
            .join(format!("pkg-{}", fixture_uuid(1).simple()))
    )
    .is_empty());
    assert_eq!(
        fs::read(
            root.join("local")
                .join(format!("pkg-{}", fixture_uuid(2).simple()))
        )
        .unwrap(),
        b"occupied file"
    );
    for n in 3..=4 {
        assert_eq!(
            fs::read(
                root.join("local")
                    .join(format!("pkg-{}", fixture_uuid(n).simple()))
                    .join("unknown")
            )
            .unwrap(),
            b"occupied"
        );
    }
    assert_eq!(children(&root.join("local")).len(), 4);
    stage.cleanup().unwrap();
}

#[test]
fn four_stage_collisions_are_bounded() {
    let (_temp, root) = root();
    fs::create_dir_all(root.join("import-staging")).unwrap();
    for n in 1..=4 {
        fs::write(
            root.join("import-staging")
                .join(format!("stage-{}", fixture_uuid(n).simple())),
            b"occupied",
        )
        .unwrap();
    }
    let (controls, attempts) = supplied(vec![1, 101, 201, 2, 102, 202, 3, 103, 203, 4, 104, 204]);
    let storage =
        SystemLocalManifestImportStorage::with_plugins_root(root.clone()).with_controls(controls);
    assert_eq!(
        storage.prepare_stage(&record(), &canonical()).err(),
        Some(ImportCommitFailure::WriteFailed)
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 12);
    for entry in children(&root.join("import-staging")) {
        assert_eq!(fs::read(entry).unwrap(), b"occupied");
    }
}

#[test]
fn already_exists_after_stage_creation_is_not_a_stage_name_collision() {
    let (_temp, root) = root();
    let attempts = Arc::new(AtomicUsize::new(0));
    let used = attempts.clone();
    let controls = TestControls {
        uuid: Some(Arc::new(move || {
            fixture_uuid(used.fetch_add(1, Ordering::SeqCst) as u128 + 1)
        })),
        hook: Some(Arc::new(|step| {
            if step == ImportFsStep::WriteManifest {
                Err(io::ErrorKind::AlreadyExists.into())
            } else {
                Ok(())
            }
        })),
        ..Default::default()
    };
    let storage =
        SystemLocalManifestImportStorage::with_plugins_root(root.clone()).with_controls(controls);
    assert_eq!(
        storage.prepare_stage(&record(), &canonical()).err(),
        Some(ImportCommitFailure::WriteFailed)
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    assert!(children(&root.join("import-staging")).is_empty());
}

#[test]
fn staging_capacity_allows_sixteenth_and_never_reads_past_seventeenth() {
    let (_temp, root) = root();
    fs::create_dir_all(root.join("import-staging")).unwrap();
    for n in 0..15 {
        fs::write(root.join("import-staging").join(n.to_string()), b"unknown").unwrap();
    }
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
    let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
    assert_eq!(children(&root.join("import-staging")).len(), 16);
    assert_eq!(
        storage.prepare_stage(&record(), &canonical()).err(),
        Some(ImportCommitFailure::StagingCapacityExceeded)
    );
    stage.cleanup().unwrap();
    for n in 15..20 {
        fs::write(root.join("import-staging").join(n.to_string()), b"unknown").unwrap();
    }
    let parents = platform::ImportDirectories::open_or_create(&root).unwrap();
    assert_eq!(parents.staging_count(17).unwrap(), 17);
    assert_eq!(children(&root.join("import-staging")).len(), 20);
}

#[test]
fn target_collision_retries_and_then_promotes_to_a_new_independent_name() {
    let (_temp, root) = root();
    fs::create_dir_all(
        root.join("local")
            .join(format!("pkg-{}", fixture_uuid(1).simple())),
    )
    .unwrap();
    let (controls, attempts) = supplied(vec![100, 200, 1, 101, 201, 2]);
    let storage =
        SystemLocalManifestImportStorage::with_plugins_root(root.clone()).with_controls(controls);
    let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
    assert!(stage.promote().is_ok());
    assert_eq!(attempts.load(Ordering::SeqCst), 6);
    assert!(children(
        &root
            .join("local")
            .join(format!("pkg-{}", fixture_uuid(1).simple()))
    )
    .is_empty());
    assert_eq!(
        fs::read(
            root.join("local")
                .join(format!("pkg-{}", fixture_uuid(2).simple()))
                .join("manifest.json")
        )
        .unwrap(),
        canonical()
    );
}

#[test]
fn prepromotion_failure_preserves_stage_and_no_target() {
    let (_temp, root) = root();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
        .with_controls(failing(ImportFsStep::BeforePromotion));
    let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
    assert_eq!(
        stage.promote().err(),
        Some(ImportCommitFailure::WriteFailed)
    );
    assert!(children(&root.join("local")).is_empty());
    assert_eq!(children(&root.join("import-staging")).len(), 1);
    stage.cleanup().unwrap();
    assert!(children(&root.join("import-staging")).is_empty());
    stage.cleanup().unwrap();
    assert!(stage.promote().is_err());
}

#[test]
fn every_filesystem_checkpoint_respects_the_directory_commit_boundary() {
    let prepare_steps = [
        ImportFsStep::CreateStage,
        ImportFsStep::WriteManifest,
        ImportFsStep::SyncManifest,
        #[cfg(unix)]
        ImportFsStep::SyncStageDirectory,
        #[cfg(unix)]
        ImportFsStep::SyncStagingParent,
    ];

    for step in prepare_steps {
        let (_temp, root) = root();
        let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = Arc::clone(&observed);
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
            .with_controls(TestControls {
                hook: Some(Arc::new(move |actual| {
                    recorded.lock().unwrap().push(actual);
                    if actual == step {
                        Err(io::ErrorKind::PermissionDenied.into())
                    } else {
                        Ok(())
                    }
                })),
                ..Default::default()
            });
        assert_eq!(
            storage.prepare_stage(&record(), &canonical()).err(),
            Some(ImportCommitFailure::WriteFailed)
        );
        assert!(observed.lock().unwrap().contains(&step));
        assert!(children(&root.join("local")).is_empty());
        assert!(children(&root.join("import-staging")).is_empty());
    }

    let (_temp, before_root) = root();
    let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = Arc::clone(&observed);
    let storage = SystemLocalManifestImportStorage::with_plugins_root(before_root.clone())
        .with_controls(TestControls {
            hook: Some(Arc::new(move |actual| {
                recorded.lock().unwrap().push(actual);
                if actual == ImportFsStep::BeforePromotion {
                    Err(io::ErrorKind::PermissionDenied.into())
                } else {
                    Ok(())
                }
            })),
            ..Default::default()
        });
    let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
    assert_eq!(
        stage.promote().err(),
        Some(ImportCommitFailure::WriteFailed)
    );
    assert!(observed
        .lock()
        .unwrap()
        .contains(&ImportFsStep::BeforePromotion));
    assert!(children(&before_root.join("local")).is_empty());
    assert_eq!(children(&before_root.join("import-staging")).len(), 1);
    stage.cleanup().unwrap();

    for step in [
        ImportFsStep::AfterPromotion,
        ImportFsStep::SyncDestinationDirectory,
    ] {
        let (_temp, root) = root();
        let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = Arc::clone(&observed);
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
            .with_controls(TestControls {
                hook: Some(Arc::new(move |actual| {
                    recorded.lock().unwrap().push(actual);
                    if actual == step {
                        Err(io::ErrorKind::PermissionDenied.into())
                    } else {
                        Ok(())
                    }
                })),
                ..Default::default()
            });
        let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
        let promotion = stage.promote();
        assert!(observed.lock().unwrap().contains(&step));
        assert!(promotion.is_err());
        assert_eq!(stage.promotion_state(), ImportPromotionState::Committed);
        let target = children(&root.join("local")).pop().unwrap();
        assert_eq!(fs::read(target.join("manifest.json")).unwrap(), canonical());
        assert!(children(&root.join("import-staging")).is_empty());
        assert!(stage.cleanup().is_err());
    }
}

#[test]
fn prepare_failures_clean_only_created_objects() {
    for step in [
        ImportFsStep::CreateStage,
        ImportFsStep::WriteManifest,
        ImportFsStep::SyncManifest,
    ] {
        let (_temp, root) = root();
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
            .with_controls(failing(step));
        assert_eq!(
            storage.prepare_stage(&record(), &canonical()).err(),
            Some(ImportCommitFailure::WriteFailed)
        );
        assert!(children(&root.join("local")).is_empty());
        assert!(children(&root.join("import-staging")).is_empty());
    }
}

#[test]
fn partial_prepare_failure_preserves_unknown_content() {
    let (_temp, root) = root();
    let injected = root.clone();
    let controls = TestControls {
        hook: Some(Arc::new(move |step| {
            if step == ImportFsStep::WriteManifest {
                let stage = children(&injected.join("import-staging")).pop().unwrap();
                fs::write(stage.join("unknown"), b"preserve")?;
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            Ok(())
        })),
        ..Default::default()
    };
    let storage =
        SystemLocalManifestImportStorage::with_plugins_root(root.clone()).with_controls(controls);
    assert_eq!(
        storage.prepare_stage(&record(), &canonical()).err(),
        Some(ImportCommitFailure::WriteFailed)
    );
    let stage = children(&root.join("import-staging")).pop().unwrap();
    assert_eq!(fs::read(stage.join("unknown")).unwrap(), b"preserve");
    assert!(stage.join("manifest.json").exists());
}

#[test]
fn prepare_refuses_an_unknown_entry_added_during_write() {
    let (_temp, root) = root();
    let injected = root.clone();
    let controls = TestControls {
        hook: Some(Arc::new(move |step| {
            if step == ImportFsStep::SyncManifest {
                let stage = children(&injected.join("import-staging")).pop().unwrap();
                fs::write(stage.join("unknown"), b"preserve")?;
            }
            Ok(())
        })),
        ..Default::default()
    };
    let storage =
        SystemLocalManifestImportStorage::with_plugins_root(root.clone()).with_controls(controls);
    assert_eq!(
        storage.prepare_stage(&record(), &canonical()).err(),
        Some(ImportCommitFailure::WriteFailed)
    );
    let stage = children(&root.join("import-staging")).pop().unwrap();
    assert_eq!(fs::read(stage.join("unknown")).unwrap(), b"preserve");
    assert_eq!(fs::read(stage.join("manifest.json")).unwrap(), canonical());
}

#[cfg(unix)]
#[test]
fn unix_staging_parent_sync_failure_is_prepared_error() {
    for step in [
        ImportFsStep::SyncStageDirectory,
        ImportFsStep::SyncStagingParent,
    ] {
        let (_temp, root) = root();
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
            .with_controls(failing(step));
        assert_eq!(
            storage.prepare_stage(&record(), &canonical()).err(),
            Some(ImportCommitFailure::WriteFailed)
        );
        assert!(children(&root.join("local")).is_empty());
        assert!(children(&root.join("import-staging")).is_empty());
    }
}

#[test]
fn postpromotion_sync_error_is_committed() {
    let (_temp, root) = root();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
        .with_controls(failing(ImportFsStep::SyncDestinationDirectory));
    let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
    assert!(stage.promote().is_err());
    assert_eq!(stage.promotion_state(), ImportPromotionState::Committed);
    let target = children(&root.join("local")).pop().unwrap();
    assert_eq!(fs::read(target.join("manifest.json")).unwrap(), canonical());
    assert!(stage.cleanup().is_err());
}

#[test]
fn postpromotion_identity_error_is_committed_but_unverified() {
    let (_temp, root) = root();
    let injected = root.clone();
    let controls = TestControls {
        hook: Some(Arc::new(move |step| {
            if step == ImportFsStep::AfterPromotion {
                let target = children(&injected.join("local")).pop().unwrap();
                fs::write(target.join("unknown"), b"preserve")?;
            }
            Ok(())
        })),
        ..Default::default()
    };
    let storage =
        SystemLocalManifestImportStorage::with_plugins_root(root.clone()).with_controls(controls);
    let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
    assert!(stage.promote().is_err());
    assert_eq!(stage.promotion_state(), ImportPromotionState::Committed);
    assert!(stage.cleanup().is_err());
    let target = children(&root.join("local")).pop().unwrap();
    assert_eq!(fs::read(target.join("unknown")).unwrap(), b"preserve");
    assert_eq!(fs::read(target.join("manifest.json")).unwrap(), canonical());
}

#[test]
fn postpromotion_hook_error_never_reports_precommit_failure() {
    let (_temp, root) = root();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
        .with_controls(failing(ImportFsStep::AfterPromotion));
    let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
    assert!(stage.promote().is_err());
    assert_eq!(stage.promotion_state(), ImportPromotionState::Committed);
    assert_eq!(children(&root.join("local")).len(), 1);
    assert!(stage.cleanup().is_err());
}

#[test]
fn cleanup_refuses_unknown_file_or_replaced_identity() {
    for replace in [0, 1, 2] {
        let (_temp, root) = root();
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
        let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
        let path = children(&root.join("import-staging")).pop().unwrap();
        if replace == 0 {
            fs::write(path.join("unknown"), b"preserve").unwrap();
        } else if replace == 1 {
            fs::rename(&path, root.join("original-stage")).unwrap();
            fs::create_dir(&path).unwrap();
            fs::write(path.join("manifest.json"), b"replacement directory").unwrap();
        } else {
            fs::rename(path.join("manifest.json"), root.join("original-manifest")).unwrap();
            fs::write(path.join("manifest.json"), b"replacement file").unwrap();
        }
        assert_eq!(stage.cleanup(), Err(ImportCommitFailure::WriteFailed));
        assert_eq!(
            stage.promote().err(),
            Some(ImportCommitFailure::WriteFailed)
        );
        assert!(path.join("manifest.json").exists());
        if replace == 0 {
            assert_eq!(fs::read(path.join("unknown")).unwrap(), b"preserve");
        } else if replace == 1 {
            assert_eq!(
                fs::read(path.join("manifest.json")).unwrap(),
                b"replacement directory"
            );
        } else {
            assert_eq!(
                fs::read(path.join("manifest.json")).unwrap(),
                b"replacement file"
            );
        }
        assert!(children(&root.join("local")).is_empty());
    }
}

#[test]
fn cleanup_refuses_hardlinked_manifest() {
    let (_temp, root) = root();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
    let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
    let path = children(&root.join("import-staging")).pop().unwrap();
    fs::hard_link(path.join("manifest.json"), root.join("outside-link")).unwrap();
    assert!(stage.cleanup().is_err());
    assert!(stage.promote().is_err());
    assert_eq!(fs::read(root.join("outside-link")).unwrap(), canonical());
    assert_eq!(fs::read(path.join("manifest.json")).unwrap(), canonical());
}

#[test]
fn restart_never_cleans_old_stages() {
    let (_temp, root) = root();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
    drop(storage.prepare_stage(&record(), &canonical()).unwrap());
    drop(storage);
    let old = children(&root.join("import-staging")).pop().unwrap();
    for n in 0..15 {
        fs::write(root.join("import-staging").join(n.to_string()), b"unknown").unwrap();
    }
    let restarted = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
    assert_eq!(fs::read(old.join("manifest.json")).unwrap(), canonical());
    assert_eq!(
        restarted.prepare_stage(&record(), &canonical()).err(),
        Some(ImportCommitFailure::StagingCapacityExceeded)
    );
    assert_eq!(fs::read(old.join("manifest.json")).unwrap(), canonical());
}

#[test]
fn cross_volume_or_unsupported_exclusive_rename_has_no_copy_fallback() {
    for kind in [io::ErrorKind::CrossesDevices, io::ErrorKind::Unsupported] {
        let (_temp, root) = root();
        let (mut controls, attempts) = supplied(vec![100, 200, 1]);
        controls.rename_error = Some(kind);
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone())
            .with_controls(controls);
        let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
        assert_eq!(
            stage.promote().err(),
            Some(ImportCommitFailure::WriteFailed)
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert!(children(&root.join("local")).is_empty());
        let pending = children(&root.join("import-staging")).pop().unwrap();
        assert_eq!(
            fs::read(pending.join("manifest.json")).unwrap(),
            canonical()
        );
        stage.cleanup().unwrap();
    }
}

#[cfg(unix)]
#[test]
fn fixed_roots_refuse_symlink_ancestors_and_children() {
    use std::os::unix::fs::symlink;
    for component in ["plugins", "local", "import-staging", "removal-staging"] {
        let (_temp, root) = root();
        let outside = root.parent().unwrap().join("outside");
        fs::create_dir(&outside).unwrap();
        let link = if component == "plugins" {
            root.clone()
        } else {
            fs::create_dir(&root).unwrap();
            root.join(component)
        };
        symlink(&outside, link).unwrap();
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root);
        assert_eq!(
            storage.prepare_stage(&record(), &canonical()).err(),
            Some(ImportCommitFailure::WriteFailed)
        );
        assert!(children(&outside).is_empty());
    }
}

#[test]
fn invalid_relative_or_parent_traversal_root_is_rejected_without_writes() {
    let (_temp, root) = root();
    // PathBuf::push normalizes .. in verbatim Windows paths, so construct the
    // actual rejected spelling without invoking that normalization.
    let mut traversal = root.as_os_str().to_os_string();
    traversal.push(std::path::MAIN_SEPARATOR_STR);
    traversal.push("..");
    traversal.push(std::path::MAIN_SEPARATOR_STR);
    traversal.push("unexpected");
    for invalid in [
        PathBuf::from("relative-plugin-import-root"),
        PathBuf::from(traversal),
    ] {
        let storage = SystemLocalManifestImportStorage::with_plugins_root(invalid);
        assert_eq!(
            storage.prepare_stage(&record(), &canonical()).err(),
            Some(ImportCommitFailure::WriteFailed)
        );
    }
    assert!(!root.exists());
}

#[cfg(windows)]
mod windows {
    use super::*;

    fn junction(link: &std::path::Path, target: &std::path::Path) {
        let result = std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "junction creation failed: {result:?}"
        );
    }

    #[test]
    fn fixed_roots_refuse_junction_ancestors_and_children() {
        for component in ["plugins", "local", "import-staging", "removal-staging"] {
            let (_temp, root) = root();
            let outside = root.parent().unwrap().join("outside");
            fs::create_dir(&outside).unwrap();
            let link = if component == "plugins" {
                root.clone()
            } else {
                fs::create_dir(&root).unwrap();
                root.join(component)
            };
            junction(&link, &outside);
            let storage = SystemLocalManifestImportStorage::with_plugins_root(root);
            assert_eq!(
                storage.prepare_stage(&record(), &canonical()).err(),
                Some(ImportCommitFailure::WriteFailed)
            );
            assert!(children(&outside).is_empty());
            fs::remove_dir(link).unwrap();
        }
    }

    #[test]
    fn cleanup_refuses_replaced_stage_junction() {
        let (_temp, root) = root();
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
        let mut stage = storage.prepare_stage(&record(), &canonical()).unwrap();
        let path = children(&root.join("import-staging")).pop().unwrap();
        let original = root.join("original");
        fs::rename(&path, &original).unwrap();
        let outside = root.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("manifest.json"), b"unknown").unwrap();
        junction(&path, &outside);
        assert!(stage.cleanup().is_err());
        assert!(stage.promote().is_err());
        assert_eq!(fs::read(outside.join("manifest.json")).unwrap(), b"unknown");
        assert_eq!(
            fs::read(original.join("manifest.json")).unwrap(),
            canonical()
        );
        fs::remove_dir(path).unwrap();
    }

    #[test]
    fn fixed_parent_handles_pin_rename_until_stage_is_dropped() {
        let (_temp, root) = root();
        let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
        let stage = storage.prepare_stage(&record(), &canonical()).unwrap();
        for path in [
            root.clone(),
            root.join("import-staging"),
            root.join("local"),
        ] {
            let moved = path.with_extension("moved");
            assert!(fs::rename(&path, &moved).is_err());
            assert!(path.is_dir());
        }
        drop(stage);
        let moved = root.with_extension("moved");
        fs::rename(&root, &moved).unwrap();
        assert!(moved.join("import-staging").is_dir());
    }

    #[test]
    fn nul_and_ads_roots_fail_without_creating_plugin_directories() {
        let (_temp, root) = root();
        for suffix in ["\0suffix", ":stream"] {
            let mut raw = root.as_os_str().to_os_string();
            raw.push(suffix);
            let storage = SystemLocalManifestImportStorage::with_plugins_root(PathBuf::from(raw));
            assert_eq!(
                storage.prepare_stage(&record(), &canonical()).err(),
                Some(ImportCommitFailure::WriteFailed)
            );
        }
        assert!(!root.exists());
    }
}

fn record() -> PluginRecord {
    PluginRecord::local_declarative(serde_json::from_slice(VALID).unwrap()).unwrap()
}
fn canonical() -> Vec<u8> {
    record().canonical_manifest_bytes().unwrap()
}
fn fixture_uuid(n: u128) -> uuid::Uuid {
    uuid::Builder::from_u128(n)
        .with_variant(uuid::Variant::RFC4122)
        .with_version(uuid::Version::Random)
        .into_uuid()
}
