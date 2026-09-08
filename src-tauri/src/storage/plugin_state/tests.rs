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

struct StateStoreFixture {
    fixture: Fixture,
    store: PluginStateStore,
}

impl StateStoreFixture {
    fn write_primary(&self, bytes: impl AsRef<[u8]>) {
        self.fixture.write("", bytes);
    }

    fn write_backup(&self, bytes: impl AsRef<[u8]>) {
        self.fixture.write(".bak", bytes);
    }
}

fn state_store_fixture() -> StateStoreFixture {
    let fixture = Fixture::new();
    let store = fixture.store();
    StateStoreFixture { fixture, store }
}

fn valid_v2_document(revision: &str) -> String {
    format!(r#"{{"schemaVersion":2,"revision":"{revision}","entries":[]}}"#)
}

fn assert_plugin_code<T>(result: AppResult<T>, expected: &str) {
    assert!(matches!(
        result,
        Err(AppError::Plugin { code, .. }) if code == expected
    ));
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
        r#"{{"schemaVersion":2,"revision":"{revision}","entries":[{{"id":"com.easiflux.analytics","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":false}}]}}"#
    )
}

fn state(revision: u64) -> PluginStateFileV2 {
    PluginStateFileV2 {
        schema_version: 2,
        revision,
        entries: vec![PluginStateEntryV2 {
            id: PluginId::parse("com.easiflux.analytics").unwrap(),
            source: PluginSource::BuiltIn,
            publisher_id: PluginPublisherId::parse("com.easiflux").unwrap(),
            approval_fingerprint: "v1:none".into(),
            enabled: false,
        }],
    }
}

const LOCAL_FINGERPRINT_A: &str =
    "v1:sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const LOCAL_FINGERPRINT_B: &str =
    "v1:sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

fn local_entry(id: &str, fingerprint: &str) -> PluginStateEntryV2 {
    PluginStateEntryV2 {
        id: PluginId::parse(id).unwrap(),
        source: PluginSource::LocalDeclarative,
        publisher_id: PluginPublisherId::parse("com.easiflux").unwrap(),
        approval_fingerprint: fingerprint.into(),
        enabled: true,
    }
}

// Catches secondary recovery resurrecting a local authorization while preserving
// a valid built-in decision, revision, and damaged primary evidence.
#[test]
fn secondary_recovery_disables_local_but_preserves_builtin_and_revision() {
    for suffix in [".tmp", ".bak"] {
        let fixture = Fixture::new();
        let mut saved = state(u64::MAX);
        saved.entries[0].enabled = true;
        saved
            .entries
            .push(local_entry("com.easiflux.local", LOCAL_FINGERPRINT_A));
        fixture.write("", b"broken primary");
        fixture.write(suffix, serde_json::to_vec(&saved).unwrap());

        let loaded = fixture.store().load().unwrap();

        assert!(loaded.state.entries[0].enabled);
        assert!(!loaded.state.entries[1].enabled);
        assert_eq!(loaded.state.revision, u64::MAX);
        assert!(loaded.requires_rewrite);
        assert_eq!(fs::read(fixture.path()).unwrap(), b"broken primary");
    }
}

// Catches applying fail-closed recovery semantics to a valid primary document.
#[test]
fn primary_recovery_retains_local_enabled() {
    let fixture = Fixture::new();
    let mut saved = state(7);
    saved
        .entries
        .push(local_entry("com.easiflux.local", LOCAL_FINGERPRINT_A));
    fixture.write("", serde_json::to_vec(&saved).unwrap());

    let loaded = fixture.store().load().unwrap();

    assert!(loaded.state.entries[1].enabled);
    assert!(!loaded.requires_rewrite);
}

// Catches skipping a required recovery rewrite only because all local entries
// were already disabled in the selected secondary candidate.
#[test]
fn secondary_with_already_disabled_locals_still_requires_rewrite() {
    let fixture = Fixture::new();
    let mut saved = state(7);
    let mut local = local_entry("com.easiflux.local", LOCAL_FINGERPRINT_A);
    local.enabled = false;
    saved.entries.push(local);
    fixture.write("", b"broken primary");
    fixture.write(".bak", serde_json::to_vec(&saved).unwrap());

    let loaded = fixture.store().load().unwrap();

    assert!(!loaded.state.entries[1].enabled);
    assert!(loaded.requires_rewrite);
}

// Catches falling back from terminal primary candidates to a valid sidecar.
#[test]
fn future_or_oversized_primary_never_falls_back() {
    for (primary, code) in [
        (
            br#"{"schemaVersion":3,"revision":"8","entries":[]}"#.to_vec(),
            "plugin_state_unsupported_schema",
        ),
        (
            vec![b' '; MAX_PLUGIN_STATE_BYTES + 1],
            "plugin_state_unavailable",
        ),
    ] {
        let fixture = Fixture::new();
        fixture.write("", &primary);
        fixture.write(".tmp", document(1));
        fixture.write(".bak", document(2));

        assert_plugin_code(fixture.store().load(), code);
    }
}

// Catches loading v1 into a v1 runtime state instead of canonical v2 while
// forgetting to request a deferred migration write.
#[test]
fn v1_load_is_lossless_and_marks_v2_rewrite() {
    let fixture = state_store_fixture();
    let primary = br#"{"schemaVersion":1,"revision":"7","entries":[{"id":"com.easiflux.alpha","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":true}]}"#;
    fixture.write_primary(primary);
    let paths = ["", ".tmp", ".bak", ".pending", ".bak.pending"];
    let before: Vec<_> = paths
        .iter()
        .map(|suffix| {
            let path = sidecar(&fixture.fixture.path(), suffix);
            (path.clone(), path.exists().then(|| fs::read(path).unwrap()))
        })
        .collect();

    let loaded = fixture.store.load().unwrap();

    assert!(loaded.requires_rewrite);
    assert_eq!(loaded.state.schema_version, 2);
    assert_eq!(loaded.state.revision, 7);
    assert!(loaded.state.entries[0].enabled);
    for (path, expected) in before {
        assert_eq!(path.exists(), expected.is_some());
        if let Some(expected) = expected {
            assert_eq!(fs::read(path).unwrap(), expected);
        }
    }
}

// Catches accidentally routing legacy documents through the permissive v2
// decoder, or accepting v1 encodings that cannot safely migrate to v2.
#[test]
fn v1_wire_decoder_rejects_noncanonical_or_non_builtin_documents() {
    let base = r#"{"schemaVersion":1,"revision":"7","entries":[{"id":"com.easiflux.alpha","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":true}]}"#;
    let duplicate_entry = r#"{"schemaVersion":1,"revision":"7","entries":[{"id":"com.easiflux.alpha","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":true},{"id":"com.easiflux.alpha","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":false}]}"#;
    for invalid in [
        base.replace(
            r#"[{"id":"com.easiflux.alpha","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":true}]"#,
            r#"[["com.easiflux.alpha","builtIn","com.easiflux","v1:none",true]]"#,
        ),
        base.replace("\"schemaVersion\":1", "\"schemaVersion\":1,\"unknown\":true"),
        base.replace("\"enabled\":true", "\"enabled\":true,\"unknown\":true"),
        base.replace("\"schemaVersion\":1", "\"schemaVersion\":1,\"schemaVersion\":1"),
        base.replace("\"enabled\":true", "\"enabled\":true,\"enabled\":true"),
        base.replace("builtIn", "localDeclarative"),
        base.replace("builtIn", "localDeclarative")
            .replace("v1:none", LOCAL_FINGERPRINT_A),
        base.replace("v1:none", LOCAL_FINGERPRINT_A),
        base.replace("\"revision\":\"7\"", "\"revision\":7"),
        base.replace("\"revision\":\"7\"", "\"revision\":\"07\""),
        base.replace("\"revision\":\"7\"", "\"revision\":\"+7\""),
        base.replace(
            "\"revision\":\"7\"",
            "\"revision\":\"18446744073709551616\"",
        ),
        duplicate_entry.into(),
    ] {
        assert!(serde_json::from_str::<PluginStateFileV1Wire>(&invalid).is_err());
        let fixture = Fixture::new();
        fixture.write("", &invalid);
        assert_sanitized(fixture.store().load().unwrap_err());
    }
}

// Catches downgrading an unknown primary schema by accepting an older backup.
#[test]
fn future_primary_schema_never_falls_back_to_older_backup() {
    let fixture = state_store_fixture();
    fixture.write_primary(br#"{"schemaVersion":3,"revision":"8","entries":[]}"#);
    fixture.write_backup(valid_v2_document("7"));

    assert_plugin_code(fixture.store.load(), "plugin_state_unsupported_schema");
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
    assert_eq!(loaded.state.schema_version, 2);
    assert_eq!(loaded.state.revision, 0);
    assert!(loaded.state.entries.is_empty());
    assert!(!fixture.0.exists());
}

// Catches accepting a second wire spelling for the same revision identity.
#[test]
fn revisions_require_canonical_decimal_strings() {
    for revision in ["00", "01"] {
        let fixture = Fixture::new();
        let invalid = document(1).replace(
            "\"revision\":\"1\"",
            &format!("\"revision\":\"{revision}\""),
        );
        fixture.write("", &invalid);
        assert_sanitized(fixture.store().load().unwrap_err());
        assert!(serde_json::from_str::<PluginStateFileV2>(&invalid).is_err());
    }
    for revision in [0, u64::MAX] {
        let fixture = Fixture::new();
        fixture.write("", document(revision));
        assert_eq!(fixture.store().load().unwrap().state.revision, revision);
    }
}

// Catches future-schema sniffing that needs to materialize/recursively parse
// unknown layouts and consequently downgrades a valid future main to an old sidecar.
#[test]
fn deeply_nested_future_main_is_terminal_without_mutating_any_candidate() {
    for depth in [129, 4096] {
        let nested = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        for future in [
            format!(r#"{{"schemaVersion":3,"newLayout":{nested}}}"#),
            format!(r#"{{"newLayout":{nested},"schemaVersion":18446744073709551615}}"#),
        ] {
            let fixture = Fixture::new();
            for (suffix, contents) in [
                ("", future.as_str()),
                (".tmp", &document(1)),
                (".bak", &document(2)),
                (".pending", "uncommitted"),
                (".bak.pending", "staged-backup"),
            ] {
                fixture.write(suffix, contents);
            }
            let paths = ["", ".tmp", ".bak", ".pending", ".bak.pending"];
            let before: Vec<_> = paths
                .iter()
                .map(|suffix| fs::read(sidecar(&fixture.path(), suffix)).unwrap())
                .collect();
            let error = fixture.store().load().unwrap_err();
            assert!(matches!(
                &error,
                AppError::Plugin {
                    code: "plugin_state_unsupported_schema",
                    ..
                }
            ));
            assert_sanitized(error);
            assert_sanitized(fixture.store().save(&state(3)).unwrap_err());
            for (suffix, expected) in paths.iter().zip(before) {
                assert_eq!(
                    fs::read(sidecar(&fixture.path(), suffix)).unwrap(),
                    expected
                );
            }
        }
    }
}

// Catches accepting positional arrays as schema headers or treating an ambiguous
// duplicate header / malformed unknown layout as a valid future schema.
#[test]
fn schema_inspection_rejects_non_object_roots_and_ambiguous_or_malformed_headers() {
    for invalid in [
        r#"[1,"0",[]]"#,
        r#"{"schemaVersion":1,"schemaVersion":2,"revision":"0","entries":[]}"#,
        r#"{"schemaVersion":2,"schemaVersion":1,"revision":"0","entries":[]}"#,
        r#"{"schemaVersion":2,"schemaVersion":2}"#,
        r#"{"schemaVersion":2,"newLayout":[1,]}"#,
        r#"{"schemaVersion":2,"newLayout":{"nested":true,}}"#,
        r#"{"schemaVersion":2} trailing"#,
        r#"{"newLayout":{"schemaVersion":2},"revision":"0","entries":[]}"#,
    ] {
        let fixture = Fixture::new();
        fixture.write("", invalid);
        let error = fixture.store().load().unwrap_err();
        assert!(
            matches!(
                &error,
                AppError::Plugin {
                    code: "plugin_state_unavailable",
                    ..
                }
            ),
            "{invalid}"
        );
        fixture.write(".bak", document(4));
        assert_eq!(
            fixture.store().load().unwrap().state.revision,
            4,
            "{invalid}"
        );
    }
}

// Catches derived struct visitors accepting non-object entries and restoring
// enabled state from a malformed main instead of the validated backup.
#[test]
fn positional_state_entry_rejects_main_and_recovers_backup_without_rewriting_evidence() {
    let fixture = Fixture::new();
    let invalid = r#"{"schemaVersion":2,"revision":"9","entries":[["com.easiflux.analytics","builtIn","com.easiflux","v1:none",true]]}"#;
    let backup = document(3);
    fixture.write("", invalid);
    fixture.write(".bak", &backup);

    let loaded = fixture.store().load().unwrap();
    assert_eq!(loaded.state.revision, 3);
    assert_eq!(loaded.state.entries.len(), 1);
    assert!(!loaded.state.entries[0].enabled);
    assert!(serde_json::from_str::<PluginStateFileV2>(invalid).is_err());
    assert_eq!(fs::read_to_string(fixture.path()).unwrap(), invalid);
    assert_eq!(
        fs::read_to_string(sidecar(&fixture.path(), ".bak")).unwrap(),
        backup
    );
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
    assert_eq!(fixture.store().load().unwrap().state, state(u64::MAX));
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
        assert_eq!(fixture.store().load().unwrap().state.revision, expected);
    }
}

// Catches destructive downgrade even if future state has unrelated/unknown v1 fields.
#[test]
fn future_main_is_terminal_and_save_never_overwrites_it_or_sidecars() {
    for future in [
        document(8).replace("\"schemaVersion\":2", "\"schemaVersion\":3"),
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
        base.replace("\"schemaVersion\":2", "\"schemaVersion\":0"),
        base.replace(
            "\"schemaVersion\":2",
            "\"schemaVersion\":2,\"unknown\":true",
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
            "\"schemaVersion\":2",
            "\"schemaVersion\":2,\"schemaVersion\":2",
        ),
        "{raw-secret".into(),
    ];
    for invalid in cases {
        let fixture = Fixture::new();
        fixture.write("", &invalid);
        assert_sanitized(fixture.store().load().unwrap_err());
        fixture.write(".bak", document(3));
        assert_eq!(
            fixture.store().load().unwrap().state.revision,
            3,
            "{invalid}"
        );
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
    assert_eq!(fixture.store().load().unwrap().state.entries.len(), 2);
}

// Catches losing local-declarative decisions, rewriting their schema, or
// accepting an invalid local fingerprint during a normal persistence cycle.
#[test]
fn local_declarative_v2_state_round_trips_through_load_and_save() {
    let fixture = Fixture::new();
    let expected = PluginStateFileV2 {
        schema_version: 2,
        revision: 7,
        entries: vec![local_entry("com.easiflux.local", LOCAL_FINGERPRINT_A)],
    };
    fixture.write("", serde_json::to_vec(&expected).unwrap());

    let loaded = fixture.store().load().unwrap();

    assert!(!loaded.requires_rewrite);
    assert_eq!(loaded.state, expected);
    fixture.store().save(&loaded.state).unwrap();
    assert_eq!(fixture.store().load().unwrap().state, expected);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(fixture.path()).unwrap()).unwrap()
            ["schemaVersion"],
        2
    );
}

// Catches treating a built-in fingerprint as a valid local-declarative trust
// record, including nearly valid but noncanonical SHA-256 encodings.
#[test]
fn local_declarative_v2_rejects_noncanonical_fingerprints() {
    let uppercase = LOCAL_FINGERPRINT_A.replace("abcdef", "Abcdef");
    let nonhex = LOCAL_FINGERPRINT_A.replace("abcdef", "gbcdef");
    let short = &LOCAL_FINGERPRINT_A[..LOCAL_FINGERPRINT_A.len() - 1];
    for fingerprint in ["v1:none", uppercase.as_str(), nonhex.as_str(), short] {
        let invalid = PluginStateFileV2 {
            schema_version: 2,
            revision: 7,
            entries: vec![local_entry("com.easiflux.local", fingerprint)],
        };
        let fixture = Fixture::new();
        fixture.write("", serde_json::to_vec(&invalid).unwrap());
        assert_sanitized(fixture.store().load().unwrap_err());
        assert_sanitized(fixture.store().save(&invalid).unwrap_err());
    }
}

// Catches collapsing distinct source/fingerprint identities, while ensuring an
// exact composite duplicate is still rejected instead of last-wins merging.
#[test]
fn v2_composite_identity_distinguishes_source_and_fingerprint() {
    let built_in = PluginStateEntryV2 {
        id: PluginId::parse("com.easiflux.shared").unwrap(),
        source: PluginSource::BuiltIn,
        publisher_id: PluginPublisherId::parse("com.easiflux").unwrap(),
        approval_fingerprint: APPROVAL_FINGERPRINT_NONE.into(),
        enabled: false,
    };
    let local_a = local_entry("com.easiflux.shared", LOCAL_FINGERPRINT_A);
    let local_b = local_entry("com.easiflux.shared", LOCAL_FINGERPRINT_B);
    let valid = PluginStateFileV2 {
        schema_version: 2,
        revision: 7,
        entries: vec![built_in, local_a.clone(), local_b],
    };
    let fixture = Fixture::new();
    fixture.write("", serde_json::to_vec(&valid).unwrap());
    assert_eq!(fixture.store().load().unwrap().state, valid);

    let duplicate = PluginStateFileV2 {
        schema_version: 2,
        revision: 7,
        entries: vec![local_a.clone(), local_a],
    };
    let fixture = Fixture::new();
    fixture.write("", serde_json::to_vec(&duplicate).unwrap());
    assert_sanitized(fixture.store().load().unwrap_err());
    assert_sanitized(fixture.store().save(&duplicate).unwrap_err());
}

// Catches off-by-one limits and JSON/UTF8 parsing before the byte bound.
#[test]
fn exact_byte_and_entry_limits_are_accepted_and_excess_is_rejected() {
    let fixture = Fixture::new();
    let mut bytes = document(1).into_bytes();
    bytes.resize(256 * 1024, b' ');
    fixture.write("", &bytes);
    assert_eq!(fixture.store().load().unwrap().state.revision, 1);
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
    assert_eq!(fixture.store().load().unwrap().state.entries.len(), 512);
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
    invalid.schema_version = 3;
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
        assert_eq!(fixture.store().load().unwrap().state.revision, 5);
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
    assert_eq!(fixture.store().load().unwrap().state.revision, 0);
    fixture.store().save(&state(1)).unwrap();
    assert_eq!(fixture.store().load().unwrap().state.revision, 1);
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
            let requested = failures.clone();
            let failures = failures.clone();
            let observed = Arc::new(Mutex::new(Vec::new()));
            let recorded = Arc::clone(&observed);
            store.file.hook = Some(Arc::new(move |step| {
                recorded.lock().unwrap().push(step);
                if failures.contains(&step) {
                    Err(std::io::Error::other("raw-secret state.json injected"))
                } else {
                    Ok(())
                }
            }));
            assert_sanitized(store.save(&state(2)).unwrap_err());
            for fault in requested {
                if fault != WriteStep::Restore || prior.is_some() {
                    assert!(
                        observed.lock().unwrap().contains(&fault),
                        "requested fault {fault:?} did not run"
                    );
                }
            }
            assert_eq!(
                store.load().unwrap().state.revision,
                if prior.is_some() { 1 } else { 0 }
            );
            assert_eq!(
                fixture.store().load().unwrap().state.revision,
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
    assert_eq!(fixture.store().load().unwrap().state.revision, 1);
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
    assert_eq!(fixture.store().load().unwrap().state.revision, 2);
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
    assert!([2, 3].contains(&loaded.state.revision));
    assert_eq!(store.load().unwrap().state.revision, 3);
}
