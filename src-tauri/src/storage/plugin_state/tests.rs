use super::*;
use crate::error::AppError;
use crate::storage::atomic_file::WriteStep;
use std::fs;
use std::path::Path;
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("plugin-state-{}", uuid::Uuid::new_v4())))
    }

    fn path(&self) -> PathBuf {
        self.0.join("private-raw-secret/plugins/state.json")
    }

    fn store(&self) -> PluginStateStore {
        PluginStateStore::with_path(self.path())
    }

    fn write(&self, suffix: &str, bytes: impl AsRef<[u8]>) {
        let path = sidecar(&self.path(), suffix);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut path = path.as_os_str().to_os_string();
    path.push(suffix);
    path.into()
}

fn document(revision: u64) -> String {
    format!(
        r#"{{"schemaVersion":1,"revision":"{revision}","entries":[{{"id":"com.easiflux.analytics","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":false}}]}}"#
    )
}

fn state(revision: u64) -> PluginStateFileV1 {
    PluginStateFileV1 {
        schema_version: 1,
        revision,
        entries: vec![PluginStateEntryV1 {
            id: PluginId::parse("com.easiflux.analytics").unwrap(),
            source: PluginSource::BuiltIn,
            publisher_id: PluginPublisherId::parse("com.easiflux").unwrap(),
            approval_fingerprint: "v1:none".into(),
            enabled: false,
        }],
    }
}

fn assert_sanitized(error: AppError) {
    assert!(matches!(error, AppError::Plugin { .. }));
    for value in [
        error.to_string(),
        error.user_message(),
        serde_json::to_string(&error).unwrap(),
    ] {
        assert!(!value.contains("raw-secret"), "{value}");
        assert!(!value.contains("state.json"), "{value}");
    }
}

// Catches defaulting to a non-v1 schema or manufacturing filesystem state on read.
#[test]
fn missing_candidates_load_empty_v1_without_creating_directories() {
    let fixture = Fixture::new();
    let loaded = fixture.store().load().unwrap();
    assert_eq!(loaded.schema_version, 1);
    assert_eq!(loaded.revision, 0);
    assert!(loaded.entries.is_empty());
    assert!(!fixture.0.exists());
}

// Catches omitted trust identity, numeric revision serialization, and nonpersistent saves.
#[test]
fn save_creates_parents_and_round_trips_a_bound_identity_and_string_revision() {
    let fixture = Fixture::new();
    fixture.store().save(&state(u64::MAX)).unwrap();
    let raw: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.path()).unwrap()).unwrap();
    assert_eq!(
        raw,
        serde_json::from_str::<serde_json::Value>(&document(u64::MAX)).unwrap()
    );
    assert_eq!(fixture.store().load().unwrap(), state(u64::MAX));
}

// Catches selecting newer sidecars ahead of main or skipping the temp recovery candidate.
#[test]
fn recovery_prefers_main_then_temp_then_backup() {
    for (main, temp, expected) in [
        (Some(document(1)), Some(document(2)), 1),
        (None, Some(document(2)), 2),
        (Some("malformed".into()), Some(document(2)), 2),
        (None, Some("malformed".into()), 3),
        (Some("malformed".into()), None, 3),
    ] {
        let fixture = Fixture::new();
        if let Some(main) = main {
            fixture.write("", main);
        }
        if let Some(temp) = temp {
            fixture.write(".tmp", temp);
        }
        fixture.write(".bak", document(3));
        assert_eq!(fixture.store().load().unwrap().revision, expected);
    }
}

// Catches destructive downgrade even if future state has unrelated/unknown v1 fields.
#[test]
fn future_main_is_terminal_and_save_never_overwrites_it_or_sidecars() {
    for future in [
        document(8).replace("\"schemaVersion\":1", "\"schemaVersion\":2"),
        r#"{"schemaVersion":4294967296,"newLayout":{"future":true}}"#.into(),
    ] {
        let fixture = Fixture::new();
        fixture.write("", &future);
        fixture.write(".tmp", document(2));
        fixture.write(".bak", document(3));
        assert_sanitized(fixture.store().load().unwrap_err());
        assert_sanitized(fixture.store().save(&state(9)).unwrap_err());
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), future);
        assert_eq!(
            fs::read_to_string(sidecar(&fixture.path(), ".tmp")).unwrap(),
            document(2)
        );
        assert_eq!(
            fs::read_to_string(sidecar(&fixture.path(), ".bak")).unwrap(),
            document(3)
        );
    }
}

// Catches any missing strict DTO/semantic validation and accidental empty-on-corruption behavior.
#[test]
fn malformed_current_documents_are_rejected_but_can_recover_from_backup() {
    let base = document(1);
    let cases = [
        base.replace("\"schemaVersion\":1", "\"schemaVersion\":0"),
        base.replace(
            "\"schemaVersion\":1",
            "\"schemaVersion\":1,\"unknown\":true",
        ),
        base.replace("\"enabled\":false", "\"enabled\":false,\"unknown\":true"),
        base.replace("com.easiflux.analytics", "Invalid"),
        base.replace(
            "\"publisherId\":\"com.easiflux\"",
            "\"publisherId\":\"invalid\"",
        ),
        base.replace("builtIn", "external"),
        base.replace("v1:none", "v1:other"),
        base.replace("\"revision\":\"1\"", "\"revision\":1"),
        base.replace(
            "\"revision\":\"1\"",
            "\"revision\":\"18446744073709551616\"",
        ),
        base.replace("\"revision\":\"1\"", "\"revision\":\"+1\""),
        base.replace("\"revision\":\"1\"", "\"revision\":\"\""),
        base.replace("\"enabled\":false", "\"enabled\":0"),
        base.replace(
            "\"schemaVersion\":1",
            "\"schemaVersion\":1,\"schemaVersion\":1",
        ),
        "{raw-secret".into(),
    ];
    for invalid in cases {
        let fixture = Fixture::new();
        fixture.write("", &invalid);
        assert_sanitized(fixture.store().load().unwrap_err());
        fixture.write(".bak", document(3));
        assert_eq!(fixture.store().load().unwrap().revision, 3, "{invalid}");
    }
}

// Catches last-wins maps; a distinct publisher is a distinct composite identity.
#[test]
fn duplicate_composite_identities_are_rejected_without_collapsing_other_publishers() {
    let fixture = Fixture::new();
    let mut value: serde_json::Value = serde_json::from_str(&document(1)).unwrap();
    let entry = value["entries"][0].clone();
    value["entries"].as_array_mut().unwrap().push(entry);
    fixture.write("", serde_json::to_vec(&value).unwrap());
    assert_sanitized(fixture.store().load().unwrap_err());
    value["entries"][1]["publisherId"] = "com.other".into();
    fixture.write("", serde_json::to_vec(&value).unwrap());
    assert_eq!(fixture.store().load().unwrap().entries.len(), 2);
}

// Catches off-by-one limits and JSON/UTF8 parsing before the byte bound.
#[test]
fn exact_byte_and_entry_limits_are_accepted_and_excess_is_rejected() {
    let fixture = Fixture::new();
    let mut bytes = document(1).into_bytes();
    bytes.resize(256 * 1024, b' ');
    fixture.write("", &bytes);
    assert_eq!(fixture.store().load().unwrap().revision, 1);
    bytes.push(0xff);
    fixture.write("", &bytes);
    assert_sanitized(fixture.store().load().unwrap_err());
    assert_sanitized(fixture.store().save(&state(2)).unwrap_err());
    assert_eq!(fs::read(fixture.path()).unwrap(), bytes);

    let mut value: serde_json::Value = serde_json::from_str(&document(1)).unwrap();
    let entry = value["entries"][0].clone();
    value["entries"] = (0..512)
        .map(|i| {
            let mut entry = entry.clone();
            entry["id"] = format!("com.plugin.p{i}").into();
            entry
        })
        .collect::<Vec<_>>()
        .into();
    fixture.write("", serde_json::to_vec(&value).unwrap());
    assert_eq!(fixture.store().load().unwrap().entries.len(), 512);
    let mut extra = entry;
    extra["id"] = "com.plugin.extra".into();
    value["entries"].as_array_mut().unwrap().push(extra);
    fixture.write("", serde_json::to_vec(&value).unwrap());
    assert_sanitized(fixture.store().load().unwrap_err());
}

// Catches trusting an in-memory DTO without revalidating before any file mutation.
#[test]
fn invalid_in_memory_states_cannot_replace_committed_bytes() {
    let fixture = Fixture::new();
    fixture.write("", document(1));
    let mut cases = Vec::new();
    let mut invalid = state(2);
    invalid.schema_version = 2;
    cases.push(invalid);
    let mut invalid = state(2);
    invalid.entries[0].approval_fingerprint = "raw-secret".into();
    cases.push(invalid);
    let mut invalid = state(2);
    invalid.entries.push(invalid.entries[0].clone());
    cases.push(invalid);
    let mut invalid = state(2);
    invalid.entries = (0..513)
        .map(|i| {
            let mut entry = state(2).entries.remove(0);
            entry.id = PluginId::parse(format!("com.plugin.p{i}")).unwrap();
            entry
        })
        .collect();
    cases.push(invalid);
    for invalid in cases {
        assert_sanitized(fixture.store().save(&invalid).unwrap_err());
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), document(1));
    }
}

// Catches losing the previous commit and persisting stale recovered temp data ahead of backup.
#[test]
fn saves_preserve_previous_commit_and_recovered_state_in_backup() {
    for suffix in ["", ".tmp", ".bak"] {
        let fixture = Fixture::new();
        fixture.write(suffix, document(4));
        fixture.store().save(&state(5)).unwrap();
        assert_eq!(fixture.store().load().unwrap().revision, 5);
        let backup: serde_json::Value =
            serde_json::from_slice(&fs::read(sidecar(&fixture.path(), ".bak")).unwrap()).unwrap();
        assert_eq!(backup["revision"], "4");
        assert!(!sidecar(&fixture.path(), ".pending").exists());
    }
}

// Catches treating a fully written but uncommitted stage as crash-recovery input.
#[test]
fn stale_pending_is_never_read_as_current_and_does_not_block_save() {
    let fixture = Fixture::new();
    fixture.write(".pending", document(99));
    assert_eq!(fixture.store().load().unwrap().revision, 0);
    fixture.store().save(&state(1)).unwrap();
    assert_eq!(fixture.store().load().unwrap().revision, 1);
}

// Catches returning an error while leaving new staged data recoverable, including double faults.
#[test]
fn injected_write_rotation_promotion_and_restore_failures_never_publish_uncommitted_state() {
    for failures in [
        vec![WriteStep::TempWrite],
        vec![WriteStep::BackupRotation],
        vec![WriteStep::Promotion],
        vec![WriteStep::Promotion, WriteStep::Restore],
    ] {
        for prior in [None, Some(""), Some(".tmp"), Some(".bak")] {
            let fixture = Fixture::new();
            if let Some(suffix) = prior {
                fixture.write(suffix, document(1));
            }
            // Rotation and restoration are meaningful only if a previous commit exists.
            if prior.is_none() && failures.contains(&WriteStep::BackupRotation) {
                continue;
            }
            let mut store = fixture.store();
            let failures = failures.clone();
            store.file.hook = Some(Arc::new(move |step| {
                if failures.contains(&step) {
                    Err(std::io::Error::other("raw-secret state.json injected"))
                } else {
                    Ok(())
                }
            }));
            assert_sanitized(store.save(&state(2)).unwrap_err());
            assert_eq!(
                store.load().unwrap().revision,
                if prior.is_some() { 1 } else { 0 }
            );
            assert_eq!(
                fixture.store().load().unwrap().revision,
                if prior.is_some() { 1 } else { 0 }
            );
        }
    }
}

// Catches ignoring real filesystem failures and leaking private paths in error contracts.
#[test]
fn real_io_failure_is_sanitized_and_keeps_previous_main() {
    let fixture = Fixture::new();
    fixture.write("", document(1));
    fs::create_dir(sidecar(&fixture.path(), ".pending")).unwrap();
    assert_sanitized(fixture.store().save(&state(2)).unwrap_err());
    assert_eq!(fixture.store().load().unwrap().revision, 1);
}

// Catches a false rejected-save result after the rename commit point; the flag
// ensures the intended post-commit fault actually ran, not just the happy path.
#[test]
fn post_commit_directory_sync_failure_keeps_the_save_successful() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let fixture = Fixture::new();
    fixture.write("", document(1));
    let injected = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&injected);
    let mut store = fixture.store();
    store.file.hook = Some(Arc::new(move |step| {
        if step == WriteStep::PostCommitSync {
            observed.store(true, Ordering::SeqCst);
            return Err(std::io::Error::other("raw-secret post-commit sync failure"));
        }
        Ok(())
    }));
    store.save(&state(2)).unwrap();
    assert!(injected.load(Ordering::SeqCst));
    assert_eq!(fixture.store().load().unwrap().revision, 2);
}

// Catches moving load outside the transaction lock or allowing overlapping save transactions.
#[test]
fn store_is_send_sync_and_load_and_save_share_the_full_transaction_lock() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PluginStateStore>();
    let fixture = Fixture::new();
    fixture.write("", document(1));
    let (entered_tx, entered_rx) = mpsc::channel();
    let release = Arc::new(Barrier::new(2));
    let pause = Arc::clone(&release);
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut store = fixture.store();
    store.file.hook = Some(Arc::new(move |step| {
        if step == WriteStep::Promotion
            && calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0
        {
            entered_tx.send(()).unwrap();
            pause.wait();
        }
        Ok(())
    }));
    let store = Arc::new(store);
    let first = Arc::clone(&store);
    let first = std::thread::spawn(move || first.save(&state(2)));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let loader = Arc::clone(&store);
    let load_done = done_tx.clone();
    let loader = std::thread::spawn(move || {
        let loaded = loader.load();
        load_done.send(()).unwrap();
        loaded
    });
    let saver = Arc::clone(&store);
    let saver = std::thread::spawn(move || {
        let saved = saver.save(&state(3));
        done_tx.send(()).unwrap();
        saved
    });
    let blocked = done_rx.recv_timeout(Duration::from_millis(100)).is_err();
    release.wait();
    first.join().unwrap().unwrap();
    let loaded = loader.join().unwrap().unwrap();
    saver.join().unwrap().unwrap();
    assert!(blocked, "a load or save escaped the in-flight transaction");
    assert!([2, 3].contains(&loaded.revision));
    assert_eq!(store.load().unwrap().revision, 3);
}
