use super::*;
use crate::plugin::manifest::{PluginPublisherId, PluginSource};
use crate::storage::plugin_state::{PluginStateEntryV2, PluginStateFileV2, PluginStateLoad};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

// Only the external storage boundary is replaced: registry derivation,
// validation, cloning and committing remain real production behavior.
#[derive(Clone)]
struct MemoryPersistence(Arc<Mutex<MemoryState>>);

struct MemoryState {
    persisted: PluginStateFileV2,
    loaded: PluginStateLoad,
    load_error: bool,
    save_error: bool,
    loads: usize,
    saves: Vec<PluginStateFileV2>,
}

impl MemoryPersistence {
    fn new(state: PluginStateFileV2) -> Self {
        Self(Arc::new(Mutex::new(MemoryState {
            persisted: state.clone(),
            loaded: PluginStateLoad {
                state,
                requires_rewrite: false,
            },
            load_error: false,
            save_error: false,
            loads: 0,
            saves: vec![],
        })))
    }

    fn registry(&self, builtins: Vec<PluginManifestV1>) -> PluginRegistry {
        PluginRegistry::initialize(builtins, Box::new(self.clone()))
    }

    fn loaded_from_v1(state: PluginStateFileV2) -> Self {
        let persistence = Self::new(state);
        persistence.0.lock().unwrap().loaded.requires_rewrite = true;
        persistence
    }

    fn failing_loaded_from_v1(state: PluginStateFileV2) -> Self {
        let persistence = Self::loaded_from_v1(state);
        persistence.0.lock().unwrap().save_error = true;
        persistence
    }

    fn registry_with_builtin(&self) -> PluginRegistry {
        self.registry(vec![manifest("com.easiflux.alpha")])
    }

    fn last_save(&self) -> Option<PluginStateFileV2> {
        self.0.lock().unwrap().saves.last().cloned()
    }

    fn allow_saves(&self) {
        self.0.lock().unwrap().save_error = false;
    }
}

fn storage_error() -> AppError {
    AppError::Plugin {
        code: "plugin_state_unavailable",
        message: "插件状态存储暂不可用",
        diagnostic: Some("private storage detail".into()),
    }
}

impl PluginStatePersistence for MemoryPersistence {
    fn load(&self) -> AppResult<PluginStateLoad> {
        let mut memory = self.0.lock().unwrap();
        memory.loads += 1;
        if memory.load_error {
            Err(storage_error())
        } else {
            Ok(memory.loaded.clone())
        }
    }

    fn save(&self, state: &PluginStateFileV2) -> AppResult<()> {
        let mut memory = self.0.lock().unwrap();
        memory.saves.push(state.clone());
        if memory.save_error {
            return Err(storage_error());
        }
        state
            .validate_for_persistence()
            .map_err(|_| storage_error())?;
        memory.persisted = state.clone();
        memory.loaded = PluginStateLoad {
            state: state.clone(),
            requires_rewrite: false,
        };
        Ok(())
    }
}

fn manifest(id: &str) -> PluginManifestV1 {
    serde_json::from_value(json!({
        "schemaVersion": 1, "id": id, "publisherId": "com.easiflux",
        "publisher": "EasiFlux", "name": "Test plugin", "description": "Metadata only",
        "version": "1.0.0", "contributions": [], "requestedCapabilities": []
    }))
    .unwrap()
}

fn entry(id: &str, publisher: &str, enabled: bool) -> PluginStateEntryV2 {
    PluginStateEntryV2 {
        id: PluginId::parse(id).unwrap(),
        source: PluginSource::BuiltIn,
        publisher_id: PluginPublisherId::parse(publisher).unwrap(),
        approval_fingerprint: "v1:none".into(),
        enabled,
    }
}

fn enabled_builtin_state(revision: &str) -> PluginStateFileV2 {
    PluginStateFileV2 {
        schema_version: 2,
        revision: revision.parse().unwrap(),
        entries: vec![entry("com.easiflux.alpha", "com.easiflux", true)],
    }
}

fn snapshot(registry: &PluginRegistry) -> Value {
    serde_json::to_value(registry.catalog_snapshot()).unwrap()
}

fn error_code(result: AppResult<PluginCatalogMutationResult>) -> Value {
    let error = serde_json::to_value(result.unwrap_err()).unwrap();
    assert_eq!(error.as_object().unwrap().len(), 2);
    assert!(!error.to_string().contains("private"));
    error["code"].clone()
}

// Catches marking an empty trusted catalog unavailable or inventing a revision.
#[test]
fn empty_catalog_is_available_at_revision_zero() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    assert_eq!(
        snapshot(&memory.registry(vec![])),
        json!({
            "schemaVersion": 2, "revision": "0", "catalogGeneration": "0",
            "availability": "available", "availabilityReasonCode": null,
            "localDiscovery": {"status": "available", "rejectedPackageCount": 0}, "plugins": []
        })
    );
}

// Catches preserving insertion order, enabling by default, or leaking grants.
#[test]
fn catalog_is_sorted_and_disabled_by_default() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let value = snapshot(&memory.registry(vec![manifest("com.zeta"), manifest("com.alpha")]));
    assert_eq!(value["plugins"].as_array().unwrap().len(), 2);
    assert_eq!(value["plugins"][0]["manifest"]["id"], "com.alpha");
    assert_eq!(value["plugins"][1]["manifest"]["id"], "com.zeta");
    for item in value["plugins"].as_array().unwrap() {
        assert_eq!(item["status"], "disabled");
        assert_eq!(item["source"], "builtIn");
        assert_eq!(item["canToggle"], true);
        assert_eq!(item["statusReasonCode"], Value::Null);
        assert_eq!(item["grantedCapabilities"], json!([]));
    }
}

// Catches partial catalog acceptance and retry accidentally reviving invalid builtins.
#[test]
fn invalid_or_duplicate_builtin_rejects_the_entire_catalog() {
    let mut invalid = manifest("com.invalid");
    invalid.requested_capabilities.push("network".into());
    for builtins in [
        vec![manifest("com.alpha"), invalid],
        vec![manifest("com.alpha"), manifest("com.alpha")],
    ] {
        let memory = MemoryPersistence::new(PluginStateFileV2::empty());
        let mut registry = memory.registry(builtins);
        registry.retry_state_load();
        let value = snapshot(&registry);
        assert_eq!(value["availability"], "unavailable");
        assert_eq!(value["availabilityReasonCode"], "catalogInvalid");
        assert_eq!(value["plugins"], json!([]));
        assert_eq!(
            error_code(registry.set_enabled("com.alpha", true)),
            "plugin_catalog_invalid"
        );
        assert_eq!(
            error_code(registry.set_enabled("INVALID", true)),
            "plugin_invalid_id"
        );
        assert_eq!(memory.0.lock().unwrap().loads, 0);
    }
}

// Catches dropping metadata or allowing toggles while loading storage has failed.
#[test]
fn unavailable_storage_preserves_metadata_as_blocked() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    memory.0.lock().unwrap().load_error = true;
    let mut registry = memory.registry(vec![manifest("com.alpha")]);
    let value = snapshot(&registry);
    assert_eq!(value["availabilityReasonCode"], "stateUnavailable");
    assert_eq!(value["plugins"][0]["manifest"]["id"], "com.alpha");
    assert_eq!(value["plugins"][0]["status"], "blocked");
    assert_eq!(value["plugins"][0]["statusReasonCode"], "stateUnavailable");
    assert_eq!(value["plugins"][0]["canToggle"], false);
    assert_eq!(
        error_code(registry.set_enabled("com.alpha", true)),
        "plugin_state_unavailable"
    );
    assert_eq!(
        error_code(registry.set_enabled("com.unknown", true)),
        "plugin_not_found"
    );
    assert!(memory.0.lock().unwrap().saves.is_empty());
}

// Catches retry being a no-op, or unnecessarily reloading an already available registry.
#[test]
fn successful_retry_restores_persisted_revision_and_enabled_state() {
    let memory = MemoryPersistence::new(PluginStateFileV2 {
        schema_version: 2,
        revision: 7,
        entries: vec![entry("com.alpha", "com.easiflux", true)],
    });
    memory.0.lock().unwrap().load_error = true;
    let mut registry = memory.registry(vec![manifest("com.alpha")]);
    registry.retry_state_load();
    assert_eq!(snapshot(&registry)["availability"], "unavailable");
    memory.0.lock().unwrap().load_error = false;
    registry.retry_state_load();
    let value = snapshot(&registry);
    assert_eq!(value["availability"], "available");
    assert_eq!(value["revision"], "7");
    assert_eq!(value["plugins"][0]["status"], "enabled");
    registry.retry_state_load();
    assert_eq!(memory.0.lock().unwrap().loads, 3);
}

// Catches matching only plugin ID, or pruning unknown/mismatched identities on save.
#[test]
fn only_exact_identity_enables_and_unmatched_entries_survive_mutation() {
    let memory = MemoryPersistence::new(PluginStateFileV2 {
        schema_version: 2,
        revision: 9,
        entries: vec![
            entry("com.alpha", "com.other", true),
            entry("com.zeta", "com.easiflux", true),
            entry("com.unknown", "com.easiflux", true),
        ],
    });
    let mut registry = memory.registry(vec![manifest("com.alpha"), manifest("com.zeta")]);
    assert_eq!(snapshot(&registry)["plugins"][0]["status"], "disabled");
    assert_eq!(snapshot(&registry)["plugins"][1]["status"], "enabled");
    registry.set_enabled("com.alpha", true).unwrap();
    let saved = serde_json::to_value(&memory.0.lock().unwrap().persisted).unwrap();
    assert_eq!(
        saved,
        json!({"schemaVersion":2,"revision":"10","entries":[
            {"id":"com.alpha","source":"builtIn","publisherId":"com.other","approvalFingerprint":"v1:none","enabled":true},
            {"id":"com.zeta","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":true},
            {"id":"com.unknown","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":true},
            {"id":"com.alpha","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":true}
        ]})
    );
}

// Catches accepting invalid typed state from a persistence implementation.
#[test]
fn invalid_loaded_fingerprint_is_unavailable_and_never_activates() {
    let mut invalid = entry("com.alpha", "com.easiflux", true);
    invalid.approval_fingerprint = "v1:other".into();
    let memory = MemoryPersistence::new(PluginStateFileV2 {
        schema_version: 2,
        revision: 3,
        entries: vec![invalid],
    });
    let value = snapshot(&memory.registry(vec![manifest("com.alpha")]));
    assert_eq!(value["availabilityReasonCode"], "stateUnavailable");
    assert_eq!(value["plugins"][0]["status"], "blocked");
}

// Catches skipping ID parsing/lookup or attempting persistence for rejected requests.
#[test]
fn malformed_and_unknown_ids_return_structured_errors_without_saving() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![manifest("com.alpha")]);
    assert_eq!(
        error_code(registry.set_enabled("INVALID", true)),
        "plugin_invalid_id"
    );
    assert_eq!(
        error_code(registry.set_enabled("com.unknown", true)),
        "plugin_not_found"
    );
    assert!(memory.0.lock().unwrap().saves.is_empty());
}

// Catches treating a no-op as a write, both absent disabled and persisted enabled.
#[test]
fn idempotent_updates_do_not_save_or_increment_revision() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![manifest("com.alpha")]);
    assert_eq!(
        registry.set_enabled("com.alpha", false).unwrap().revision,
        "0"
    );
    assert!(memory.0.lock().unwrap().saves.is_empty());
    registry.set_enabled("com.alpha", true).unwrap();
    assert_eq!(
        registry.set_enabled("com.alpha", true).unwrap().revision,
        "1"
    );
    assert_eq!(snapshot(&registry)["revision"], "1");
    assert_eq!(memory.0.lock().unwrap().saves.len(), 1);
}

// Catches treating a v1-to-v2 rewrite as a logical decision that increments revision.
#[test]
fn same_value_decision_rewrites_loaded_v1_as_v2_without_revision_change() {
    let persistence = MemoryPersistence::loaded_from_v1(enabled_builtin_state("7"));
    let mut registry = persistence.registry_with_builtin();

    let result = registry.set_enabled("com.easiflux.alpha", true).unwrap();

    assert_eq!(result.revision, "7");
    let saved = persistence.last_save().unwrap();
    assert_eq!(saved.schema_version, 2);
    assert_eq!(saved.revision, 7);
}

// Catches clearing the rewrite marker before persistence has succeeded.
#[test]
fn failed_migration_rewrite_keeps_state_and_rewrite_marker() {
    let persistence = MemoryPersistence::failing_loaded_from_v1(enabled_builtin_state("7"));
    let mut registry = persistence.registry_with_builtin();

    assert!(registry.set_enabled("com.easiflux.alpha", true).is_err());
    assert_eq!(registry.catalog_snapshot().revision, "7");
    persistence.allow_saves();
    registry.set_enabled("com.easiflux.alpha", true).unwrap();
    assert_eq!(persistence.0.lock().unwrap().saves.len(), 2);
    assert_eq!(persistence.last_save().unwrap().schema_version, 2);
}

// Catches failing to persist the exact next revision/identity before publishing it.
#[test]
fn successful_update_persists_next_state_and_returns_updated_item() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![manifest("com.alpha")]);
    let result = serde_json::to_value(registry.set_enabled("com.alpha", true).unwrap()).unwrap();
    assert_eq!(result["revision"], "1");
    assert_eq!(result["plugin"]["status"], "enabled");
    assert_eq!(result["plugin"]["manifest"]["id"], "com.alpha");
    assert_eq!(snapshot(&registry)["plugins"][0], result["plugin"]);
    let memory = memory.0.lock().unwrap();
    assert_eq!(memory.saves.len(), 1);
    assert_eq!(
        serde_json::to_value(&memory.saves[0]).unwrap(),
        json!({
            "schemaVersion":2,"revision":"1","entries":[{"id":"com.alpha","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":true}]
        })
    );
    assert_eq!(memory.persisted, memory.saves[0]);
}

// Catches committing in memory before save, revision drift, or losing the prior state.
#[test]
fn failed_save_preserves_memory_and_persisted_value_then_can_retry() {
    let memory = MemoryPersistence::new(PluginStateFileV2 {
        schema_version: 2,
        revision: 4,
        entries: vec![entry("com.alpha", "com.easiflux", true)],
    });
    let mut registry = memory.registry(vec![manifest("com.alpha")]);
    let before = snapshot(&registry);
    memory.0.lock().unwrap().save_error = true;
    assert_eq!(
        error_code(registry.set_enabled("com.alpha", false)),
        "plugin_state_persist_failed"
    );
    assert_eq!(snapshot(&registry), before);
    {
        let memory = memory.0.lock().unwrap();
        assert_eq!(memory.persisted.revision, 4);
        assert!(memory.persisted.entries[0].enabled);
        assert_eq!(memory.saves.len(), 1);
        assert_eq!(memory.saves[0].revision, 5);
        assert!(!memory.saves[0].entries[0].enabled);
    }
    memory.0.lock().unwrap().save_error = false;
    assert_eq!(
        registry.set_enabled("com.alpha", false).unwrap().revision,
        "5"
    );
    assert_eq!(snapshot(&registry)["plugins"][0]["status"], "disabled");
}

// Catches cloning from stale state and replacing another plugin's committed change.
#[test]
fn sequential_changes_to_two_ids_retain_both_values() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![manifest("com.alpha"), manifest("com.zeta")]);
    registry.set_enabled("com.alpha", true).unwrap();
    assert_eq!(
        registry.set_enabled("com.zeta", true).unwrap().revision,
        "2"
    );
    let persisted = &memory.0.lock().unwrap().persisted;
    assert_eq!(persisted.revision, 2);
    assert_eq!(persisted.entries.len(), 2);
    assert_eq!(persisted.entries[0].id.as_str(), "com.alpha");
    assert_eq!(persisted.entries[1].id.as_str(), "com.zeta");
    assert!(persisted.entries.iter().all(|entry| entry.enabled));
    for item in snapshot(&registry)["plugins"].as_array().unwrap() {
        assert_eq!(item["status"], "enabled");
    }
}

// Catches overflowing a persisted u64 revision or saving the wrapped revision.
#[test]
fn exhausted_revision_refuses_changes_but_allows_idempotence() {
    let memory = MemoryPersistence::new(PluginStateFileV2 {
        schema_version: 2,
        revision: u64::MAX,
        entries: vec![],
    });
    let mut registry = memory.registry(vec![manifest("com.alpha")]);
    assert_eq!(
        registry.set_enabled("com.alpha", false).unwrap().revision,
        "18446744073709551615"
    );
    assert_eq!(
        error_code(registry.set_enabled("com.alpha", true)),
        "plugin_revision_exhausted"
    );
    assert_eq!(snapshot(&registry)["revision"], "18446744073709551615");
    assert!(memory.0.lock().unwrap().saves.is_empty());
}
