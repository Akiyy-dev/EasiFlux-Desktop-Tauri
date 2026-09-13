use super::*;
use std::{
    fs,
    sync::{Arc, Mutex},
};

fn fixture(names: SafeDocumentNames) -> (tempfile::TempDir, SafePluginDocument) {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let root = temp_root();
    let doc = SafePluginDocument::new(root.path().to_owned(), names, 262144);
    (root, doc)
}

#[test]
fn both_documents_use_exactly_five_reserved_names() {
    for names in [
        SafeDocumentNames::plugin_state(),
        SafeDocumentNames::managed_ownership(),
    ] {
        let (root, doc) = fixture(names);
        fs::write(root.path().join(names.tmp), b"legacy").unwrap();
        doc.persist(Some(b"old"), b"new").unwrap();
        assert_eq!(fs::read(root.path().join(names.main)).unwrap(), b"new");
        assert_eq!(fs::read(root.path().join(names.bak)).unwrap(), b"old");
        let mut actual: Vec<_> = fs::read_dir(root.path())
            .unwrap()
            .map(|v| v.unwrap().file_name())
            .collect();
        actual.sort();
        let mut expected = vec![
            std::ffi::OsString::from(names.main),
            std::ffi::OsString::from(names.bak),
        ];
        expected.sort();
        assert_eq!(actual, expected);
    }
}

#[test]
fn pending_and_backup_pending_are_never_authority_candidates() {
    let (root, doc) = fixture(SafeDocumentNames::plugin_state());
    fs::write(root.path().join("state.json.pending"), b"pending").unwrap();
    fs::write(
        root.path().join("state.json.bak.pending"),
        b"backup pending",
    )
    .unwrap();
    assert!(doc
        .load_candidates()
        .unwrap()
        .iter()
        .all(|c| matches!(c, CandidateBytes::Missing)));
}

#[test]
fn safe_leftovers_are_cleaned_before_exclusive_create() {
    for names in [
        SafeDocumentNames::plugin_state(),
        SafeDocumentNames::managed_ownership(),
    ] {
        let (root, doc) = fixture(names);
        for name in [names.pending, names.bak_pending] {
            fs::write(root.path().join(name), b"stale").unwrap();
        }
        doc.persist(Some(b"old"), b"new").unwrap();
        assert_eq!(fs::read(root.path().join(names.main)).unwrap(), b"new");
    }
}

#[cfg(windows)]
#[test]
fn windows_delete_sharing_work_handle_blocks_main_until_absence_is_proven() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    for names in [
        SafeDocumentNames::plugin_state(),
        SafeDocumentNames::managed_ownership(),
    ] {
        for work in [names.tmp, names.pending, names.bak_pending] {
            let (root, doc) = fixture(names);
            fs::write(root.path().join(names.main), b"old").unwrap();
            fs::write(root.path().join(work), b"stale").unwrap();
            let extra = fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .open(root.path().join(work))
                .unwrap();
            assert_eq!(
                doc.persist(Some(b"old"), b"new").unwrap_err().outcome,
                PersistOutcome::NotCommitted,
                "{work}"
            );
            assert_eq!(fs::read(root.path().join(names.main)).unwrap(), b"old");
            drop(extra);
            doc.persist(Some(b"old"), b"new").unwrap();
            assert_eq!(fs::read(root.path().join(names.main)).unwrap(), b"new");
        }
    }
}

#[test]
fn unsafe_pending_symlink_reparse_hardlink_or_case_variant_fails_closed() {
    for names in [
        SafeDocumentNames::plugin_state(),
        SafeDocumentNames::managed_ownership(),
    ] {
        for name in names.all() {
            for shape in ["hardlink", "directory", "case", "symlink"] {
                let (root, doc) = fixture(names);
                let outside =
                    tempfile::NamedTempFile::new_in(root.path().parent().unwrap()).unwrap();
                fs::write(outside.path(), b"untouched").unwrap();
                let target = root.path().join(name);
                match shape {
                    "hardlink" => fs::hard_link(outside.path(), &target).unwrap(),
                    "directory" => fs::create_dir(&target).unwrap(),
                    "case" => {
                        fs::write(root.path().join(name.to_uppercase()), b"untouched").unwrap()
                    }
                    _ => {
                        #[cfg(unix)]
                        std::os::unix::fs::symlink(outside.path(), &target).unwrap();
                        #[cfg(windows)]
                        if let Err(e) = std::os::windows::fs::symlink_file(outside.path(), &target)
                        {
                            if e.raw_os_error() == Some(1314) {
                                continue;
                            } else {
                                panic!("{e}");
                            }
                        }
                    }
                }
                assert_eq!(
                    doc.persist(Some(b"old"), b"new").unwrap_err().outcome,
                    PersistOutcome::NotCommitted,
                    "{name} {shape}"
                );
                assert_eq!(fs::read(outside.path()).unwrap(), b"untouched");
            }
        }
    }
}

#[test]
fn every_precommit_fault_reports_not_committed_and_preserves_old_main() {
    for step in [
        SafeDocumentStep::CleanPending,
        SafeDocumentStep::CleanBackupPending,
        SafeDocumentStep::CreatePending,
        SafeDocumentStep::WritePending,
        SafeDocumentStep::FlushPending,
        SafeDocumentStep::CreateBackupPending,
        SafeDocumentStep::WriteBackupPending,
        SafeDocumentStep::FlushBackupPending,
        SafeDocumentStep::ReplaceBackup,
        SafeDocumentStep::DeleteLegacyTmp,
        SafeDocumentStep::SyncAfterLegacyTmp,
        SafeDocumentStep::ReplaceMain,
    ] {
        let (root, mut doc) = fixture(SafeDocumentNames::plugin_state());
        fs::write(root.path().join("state.json"), b"old").unwrap();
        doc.hook = Some(Arc::new(move |s| {
            if s == step {
                Err(std::io::Error::other("fault"))
            } else {
                Ok(())
            }
        }));
        assert_eq!(
            doc.persist(Some(b"old"), b"new").unwrap_err().outcome,
            PersistOutcome::NotCommitted,
            "{step:?}"
        );
        assert_eq!(fs::read(root.path().join("state.json")).unwrap(), b"old");
    }
}

#[test]
fn backup_replace_precedes_legacy_tmp_delete_and_main_replace() {
    let (_root, mut doc) = fixture(SafeDocumentNames::plugin_state());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    doc.hook = Some(Arc::new(move |s| {
        recorded.lock().unwrap().push(s);
        Ok(())
    }));
    doc.persist(Some(b"old"), b"new").unwrap();
    let seen = seen.lock().unwrap();
    let at = |step| seen.iter().position(|s| *s == step).unwrap();
    assert!(at(SafeDocumentStep::ReplaceBackup) < at(SafeDocumentStep::DeleteLegacyTmp));
    assert!(at(SafeDocumentStep::SyncAfterLegacyTmp) < at(SafeDocumentStep::ReplaceMain));
}

#[test]
fn postcommit_fault_reports_committed_outcome_and_adopts_new_bytes() {
    for step in [SafeDocumentStep::VerifyMain, SafeDocumentStep::SyncParent] {
        let (root, mut doc) = fixture(SafeDocumentNames::plugin_state());
        doc.hook = Some(Arc::new(move |s| {
            if s == step {
                Err(std::io::Error::other("fault"))
            } else {
                Ok(())
            }
        }));
        let error = doc.persist(Some(b"old"), b"new").unwrap_err();
        assert_eq!(error.outcome, PersistOutcome::CommittedProcessCrashSafe);
        assert!(persist_outcome_committed(error.outcome));
        assert_eq!(fs::read(root.path().join("state.json")).unwrap(), b"new");
        #[cfg(unix)]
        assert!(!outcome_satisfies_destructive_barrier(error.outcome));
    }
}

#[test]
fn replacement_reopens_to_the_same_source_identity() {
    let (root, doc) = fixture(SafeDocumentNames::plugin_state());
    doc.persist(None, b"first").unwrap();
    doc.persist(Some(b"first"), b"second").unwrap();
    assert_eq!(fs::read(root.path().join("state.json")).unwrap(), b"second");
}

#[cfg(windows)]
#[test]
fn windows_power_loss_boundary_never_reports_durable() {
    let (_root, doc) = fixture(SafeDocumentNames::plugin_state());
    assert_eq!(
        doc.persist(None, b"new").unwrap(),
        PersistOutcome::CommittedProcessCrashSafe
    );
}

#[test]
fn no_candidates_means_empty_without_creating_the_root() {
    let root = temp_root();
    let missing = root.path().join("missing");
    let doc = SafePluginDocument::new(missing.clone(), SafeDocumentNames::plugin_state(), 262144);
    assert!(doc
        .load_candidates()
        .unwrap()
        .iter()
        .all(|c| matches!(c, CandidateBytes::Missing)));
    assert!(!missing.exists());
}

fn temp_root() -> tempfile::TempDir {
    let base = std::env::current_dir().unwrap().join("target");
    fs::create_dir_all(&base).unwrap();
    tempfile::tempdir_in(base).unwrap()
}

#[test]
fn controlled_identity_swap_of_each_reserved_object_fails_closed() {
    for names in [
        SafeDocumentNames::plugin_state(),
        SafeDocumentNames::managed_ownership(),
    ] {
        for name in names.all() {
            let (root, mut doc) = fixture(names);
            for reserved in names.all() {
                fs::write(root.path().join(reserved), b"old").unwrap();
            }
            let path = root.path().join(name);
            let changed = path.clone();
            let parked = root.path().join("parked");
            doc.hook = Some(Arc::new(move |step| {
                if step == SafeDocumentStep::CleanPending {
                    fs::rename(&changed, &parked)?;
                    fs::write(&changed, b"replacement")?;
                }
                Ok(())
            }));
            let failure = doc.persist(Some(b"old"), b"new").unwrap_err();
            assert_eq!(failure.outcome, PersistOutcome::NotCommitted, "{name}");
            assert_eq!(fs::read(path).unwrap(), b"replacement", "{name}");
        }
    }
}

#[test]
fn unsupported_replacement_has_no_fallback_and_keeps_old_main() {
    let (root, mut doc) = fixture(SafeDocumentNames::plugin_state());
    fs::write(root.path().join("state.json"), b"old").unwrap();
    doc.hook = Some(Arc::new(|step| {
        if step == SafeDocumentStep::ReplaceMain {
            Err(io::ErrorKind::Unsupported.into())
        } else {
            Ok(())
        }
    }));
    assert_eq!(
        doc.persist(Some(b"old"), b"new").unwrap_err().outcome,
        PersistOutcome::NotCommitted
    );
    assert_eq!(fs::read(root.path().join("state.json")).unwrap(), b"old");
    assert_eq!(
        fs::read(root.path().join("state.json.pending")).unwrap(),
        b"new"
    );
}

#[test]
fn oversized_objects_at_all_five_names_cannot_be_mutated() {
    for names in [
        SafeDocumentNames::plugin_state(),
        SafeDocumentNames::managed_ownership(),
    ] {
        for name in names.all() {
            let (root, doc) = fixture(names);
            let path = root.path().join(name);
            fs::write(&path, vec![b' '; 262145]).unwrap();
            assert_eq!(
                doc.persist(None, b"next").unwrap_err().outcome,
                PersistOutcome::NotCommitted
            );
            assert_eq!(fs::metadata(path).unwrap().len(), 262145);
        }
    }
}

#[test]
fn reserved_objects_that_grow_after_open_fail_closed_before_mutation() {
    for names in [
        SafeDocumentNames::plugin_state(),
        SafeDocumentNames::managed_ownership(),
    ] {
        for name in names.all() {
            let (root, mut doc) = fixture(names);
            for reserved in names.all() {
                fs::write(root.path().join(reserved), b"old").unwrap();
            }
            let path = root.path().join(name);
            let changed = path.clone();
            doc.hook = Some(Arc::new(move |step| {
                if step == SafeDocumentStep::CleanPending {
                    fs::OpenOptions::new()
                        .write(true)
                        .open(&changed)?
                        .set_len(262145)?;
                }
                Ok(())
            }));
            assert_eq!(
                doc.persist(Some(b"old"), b"new").unwrap_err().outcome,
                PersistOutcome::NotCommitted,
                "{name}"
            );
            assert_eq!(fs::metadata(path).unwrap().len(), 262145, "{name}");
        }
    }
}

#[cfg(windows)]
#[test]
fn windows_junction_reparse_at_all_five_names_fails_closed() {
    for names in [
        SafeDocumentNames::plugin_state(),
        SafeDocumentNames::managed_ownership(),
    ] {
        for name in names.all() {
            let (root, doc) = fixture(names);
            let outside = temp_root();
            fs::write(outside.path().join("keep"), b"untouched").unwrap();
            let link = root.path().join(name);
            let result = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(outside.path())
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "junction fixture failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                doc.persist(None, b"next").unwrap_err().outcome,
                PersistOutcome::NotCommitted
            );
            assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"untouched");
            fs::remove_dir(link).unwrap();
        }
    }
}

#[cfg(unix)]
#[test]
fn main_commit_without_parent_sync_is_committed_but_not_destructive_barrier_safe() {
    let (root, mut doc) = fixture(SafeDocumentNames::plugin_state());
    doc.hook = Some(Arc::new(|step| {
        if step == SafeDocumentStep::SyncParent {
            Err(io::Error::other("parent sync"))
        } else {
            Ok(())
        }
    }));
    let failure = doc.persist(Some(b"old"), b"new").unwrap_err();
    assert_eq!(failure.outcome, PersistOutcome::CommittedProcessCrashSafe);
    assert!(!outcome_satisfies_destructive_barrier(failure.outcome));
    assert_eq!(fs::read(root.path().join("state.json")).unwrap(), b"new");
}

#[cfg(unix)]
#[test]
fn successful_parent_sync_is_required_for_durable_outcome() {
    let (_root, doc) = fixture(SafeDocumentNames::plugin_state());
    assert_eq!(
        doc.persist(None, b"new").unwrap(),
        PersistOutcome::CommittedDurable
    );
}

#[cfg(unix)]
#[test]
fn fifo_reserved_objects_are_rejected_without_blocking() {
    use std::os::unix::fs::FileTypeExt;

    for names in [
        SafeDocumentNames::plugin_state(),
        SafeDocumentNames::managed_ownership(),
    ] {
        for name in names.all() {
            let (root, doc) = fixture(names);
            let path = root.path().join(name);
            assert!(path.is_absolute());
            let output = std::process::Command::new("mkfifo")
                .args(["-m", "600"])
                .arg(&path)
                .output()
                .expect("POSIX mkfifo must be available for Unix security tests");
            assert!(output.status.success(), "mkfifo failed: {output:?}");
            assert!(fs::symlink_metadata(&path).unwrap().file_type().is_fifo());
            assert_eq!(
                doc.persist(None, b"new").unwrap_err().outcome,
                PersistOutcome::NotCommitted
            );
        }
    }
}
