use super::*;
use crate::storage::safe_plugin_document::{
    persist_outcome_committed, PersistOutcome, SafeDocumentStep,
};
use std::fs;

#[test]
fn committed_save_failure_requires_memory_adoption() {
    let root = temp_root();
    let mut store = ManagedOwnershipStore::with_plugins_root(root.path().into());
    store.hook = Some(std::sync::Arc::new(|step| {
        if step == SafeDocumentStep::VerifyMain {
            Err(std::io::Error::other("postcommit fault"))
        } else {
            Ok(())
        }
    }));
    let next = ManagedOwnershipIndexV1::parse(&bytes(7)).unwrap();
    let failure = store.save(&next).unwrap_err();
    assert_eq!(failure.outcome, PersistOutcome::CommittedProcessCrashSafe);
    assert!(persist_outcome_committed(failure.outcome));
    assert_eq!(store.load().unwrap().index, next);
}

#[test]
fn initial_save_does_not_invent_backup_authority() {
    let root = temp_root();
    let mut store = ManagedOwnershipStore::with_plugins_root(root.path().into());
    store.hook = Some(std::sync::Arc::new(|step| {
        if step == SafeDocumentStep::ReplaceMain {
            Err(std::io::ErrorKind::Unsupported.into())
        } else {
            Ok(())
        }
    }));
    let failure = store.save(&ManagedOwnershipIndexV1::empty()).unwrap_err();
    assert_eq!(failure.outcome, PersistOutcome::NotCommitted);
    assert!(!root.path().join("managed-ownership.json.bak").exists());
    assert_eq!(
        store.load().unwrap().index,
        ManagedOwnershipIndexV1::empty()
    );
}

fn bytes(revision: u64) -> Vec<u8> {
    format!(r#"{{"schemaVersion":1,"revision":"{revision}","entries":[]}}"#).into_bytes()
}

#[test]
fn missing_index_is_available_empty() {
    let root = temp_root();
    let path = root.path().join("missing");
    let loaded = ManagedOwnershipStore::with_plugins_root(path.clone())
        .load()
        .unwrap();
    assert_eq!(loaded.index, ManagedOwnershipIndexV1::empty());
    assert!(!loaded.requires_rewrite);
    assert!(!path.exists());
}

#[test]
fn valid_main_wins() {
    let root = temp_root();
    fs::write(root.path().join("managed-ownership.json"), bytes(9)).unwrap();
    fs::write(root.path().join("managed-ownership.json.tmp"), bytes(10)).unwrap();
    let loaded = ManagedOwnershipStore::with_plugins_root(root.path().into())
        .load()
        .unwrap();
    assert_eq!(loaded.index.canonical_bytes().unwrap(), bytes(9));
    assert!(!loaded.requires_rewrite);
}

#[test]
fn secondary_candidate_requires_rewrite_before_authorization() {
    for suffix in ["tmp", "bak"] {
        let root = temp_root();
        fs::write(root.path().join("managed-ownership.json"), b"corrupt").unwrap();
        fs::write(
            root.path().join(format!("managed-ownership.json.{suffix}")),
            bytes(9),
        )
        .unwrap();
        let loaded = ManagedOwnershipStore::with_plugins_root(root.path().into())
            .load()
            .unwrap();
        assert!(loaded.requires_rewrite);
        assert_eq!(loaded.index.canonical_bytes().unwrap(), bytes(9));
    }
}

#[test]
fn future_or_262145_byte_primary_never_falls_back() {
    for primary in [
        br#"{"schemaVersion":2,"revision":"0","entries":[]}"#.to_vec(),
        vec![b' '; 262145],
    ] {
        let root = temp_root();
        fs::write(root.path().join("managed-ownership.json"), primary).unwrap();
        fs::write(root.path().join("managed-ownership.json.bak"), bytes(9)).unwrap();
        assert!(ManagedOwnershipStore::with_plugins_root(root.path().into())
            .load()
            .is_err());
    }
}

#[test]
fn all_present_candidates_invalid_is_unavailable() {
    let root = temp_root();
    for suffix in ["", ".tmp", ".bak"] {
        fs::write(
            root.path().join(format!("managed-ownership.json{suffix}")),
            b"corrupt",
        )
        .unwrap();
    }
    assert!(ManagedOwnershipStore::with_plugins_root(root.path().into())
        .load()
        .is_err());
}

#[test]
fn safe_rewrite_preserves_revision() {
    let root = temp_root();
    fs::write(
        root.path().join("managed-ownership.json.tmp"),
        bytes(u64::MAX),
    )
    .unwrap();
    let store = ManagedOwnershipStore::with_plugins_root(root.path().into());
    let recovered = store.load().unwrap();
    store.save(&recovered.index).unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(loaded.index, recovered.index);
    assert!(!loaded.requires_rewrite);
}

fn temp_root() -> tempfile::TempDir {
    let base = std::env::current_dir().unwrap().join("target");
    fs::create_dir_all(&base).unwrap();
    tempfile::tempdir_in(base).unwrap()
}
