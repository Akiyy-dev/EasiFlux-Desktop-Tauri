use super::*;
use crate::plugin::discovery::LocalDiscoveryOutcome;
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
            error_code(registry.set_enabled("com.alpha", true, "0")),
            "plugin_catalog_invalid"
        );
        assert_eq!(
            error_code(registry.set_enabled("INVALID", true, "0")),
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
        error_code(registry.set_enabled("com.alpha", true, "0")),
        "plugin_state_unavailable"
    );
    assert_eq!(
        error_code(registry.set_enabled("com.unknown", true, "0")),
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

// Catches matching only plugin ID or pruning entries outside the explicit ID/source decision.
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
    registry.set_enabled("com.alpha", true, "0").unwrap();
    let saved = serde_json::to_value(&memory.0.lock().unwrap().persisted).unwrap();
    assert_eq!(
        saved,
        json!({"schemaVersion":2,"revision":"10","entries":[
            {"id":"com.alpha","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":true},
            {"id":"com.unknown","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":true},
            {"id":"com.zeta","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":true}
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
        error_code(registry.set_enabled("INVALID", true, "0")),
        "plugin_invalid_id"
    );
    assert_eq!(
        error_code(registry.set_enabled("com.unknown", true, "0")),
        "plugin_not_found"
    );
    assert!(memory.0.lock().unwrap().saves.is_empty());
}

// Catches skipping an explicit absent-disabled decision or writing a repeated exact decision.
#[test]
fn idempotent_updates_do_not_save_or_increment_revision() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![manifest("com.alpha")]);
    assert_eq!(
        registry
            .set_enabled("com.alpha", false, "0")
            .unwrap()
            .revision,
        "1"
    );
    registry.set_enabled("com.alpha", false, "0").unwrap();
    assert_eq!(memory.0.lock().unwrap().saves.len(), 1);
    registry.set_enabled("com.alpha", true, "0").unwrap();
    assert_eq!(
        registry
            .set_enabled("com.alpha", true, "0")
            .unwrap()
            .revision,
        "2"
    );
    assert_eq!(snapshot(&registry)["revision"], "2");
    assert_eq!(memory.0.lock().unwrap().saves.len(), 2);
}

// Catches treating a v1-to-v2 rewrite as a logical decision that increments revision.
#[test]
fn same_value_decision_rewrites_loaded_v1_as_v2_without_revision_change() {
    let persistence = MemoryPersistence::loaded_from_v1(enabled_builtin_state("7"));
    let mut registry = persistence.registry_with_builtin();

    let result = registry
        .set_enabled("com.easiflux.alpha", true, "0")
        .unwrap();

    assert_eq!(result.revision, "7");
    let saved = persistence.last_save().unwrap();
    assert_eq!(saved.schema_version, 2);
    assert_eq!(saved.revision, 7);
}

// Catches serialization-only ordering changes advancing revision or failing at u64::MAX.
#[test]
fn canonical_order_rewrite_preserves_revision_and_is_copy_on_write() {
    for (revision, migrated) in [(7, false), (7, true), (u64::MAX, false), (u64::MAX, true)] {
        let original = PluginStateFileV2 {
            schema_version: 2,
            revision,
            entries: vec![
                entry("com.zeta", "com.easiflux", true),
                entry("com.alpha", "com.easiflux", true),
            ],
        };
        let persistence = MemoryPersistence::failing_loaded_from_v1(original.clone());
        persistence.0.lock().unwrap().loaded.requires_rewrite = migrated;
        let mut registry = persistence.registry(vec![manifest("com.alpha"), manifest("com.zeta")]);
        let before = registry.catalog_snapshot();
        assert_eq!(
            error_code(registry.set_enabled("com.alpha", true, "0")),
            "plugin_state_persist_failed"
        );
        assert_eq!(registry.catalog_snapshot(), before);
        assert_eq!(persistence.0.lock().unwrap().persisted, original);
        assert!(
            matches!(registry.runtime, Runtime::Available { requires_rewrite, .. } if requires_rewrite == migrated)
        );
        persistence.allow_saves();
        assert_eq!(
            registry
                .set_enabled("com.alpha", true, "0")
                .unwrap()
                .revision,
            revision.to_string()
        );
        let saved = persistence.last_save().unwrap();
        assert_eq!(saved.revision, revision);
        assert_eq!(saved.entries[0].id.as_str(), "com.alpha");
        assert_eq!(saved.entries[1].id.as_str(), "com.zeta");
        registry.set_enabled("com.alpha", true, "0").unwrap();
        assert_eq!(persistence.0.lock().unwrap().saves.len(), 2);
    }
}

// Catches clearing the rewrite marker before persistence has succeeded.
#[test]
fn failed_migration_rewrite_keeps_state_and_rewrite_marker() {
    let persistence = MemoryPersistence::failing_loaded_from_v1(enabled_builtin_state("7"));
    let mut registry = persistence.registry_with_builtin();

    assert!(registry
        .set_enabled("com.easiflux.alpha", true, "0")
        .is_err());
    assert_eq!(registry.catalog_snapshot().revision, "7");
    persistence.allow_saves();
    registry
        .set_enabled("com.easiflux.alpha", true, "0")
        .unwrap();
    assert_eq!(persistence.0.lock().unwrap().saves.len(), 2);
    assert_eq!(persistence.last_save().unwrap().schema_version, 2);
}

// Catches clearing a recovered rewrite marker after a failed retry at the
// maximum revision, where no revision increment is permitted.
#[test]
fn recovered_disabled_identity_rewrites_at_max_revision_after_save_failure() {
    let local = PluginRecord::local_declarative(manifest("com.easiflux.local")).unwrap();
    let identity = local.identity();
    let persisted = PluginStateFileV2 {
        schema_version: 2,
        revision: u64::MAX,
        entries: vec![PluginStateEntryV2 {
            id: identity.id,
            source: identity.source,
            publisher_id: identity.publisher_id,
            approval_fingerprint: identity.approval_fingerprint,
            enabled: false,
        }],
    };
    let memory = MemoryPersistence::failing_loaded_from_v1(persisted.clone());
    let mut registry = memory.registry(vec![]);

    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![local]))
        .unwrap();
    assert!(registry
        .set_enabled("com.easiflux.local", false, "1")
        .is_err());
    assert_eq!(memory.0.lock().unwrap().saves.len(), 1);
    assert_eq!(registry.catalog_snapshot().revision, u64::MAX.to_string());

    memory.allow_saves();
    registry
        .set_enabled("com.easiflux.local", false, "1")
        .unwrap();
    assert_eq!(memory.0.lock().unwrap().saves.len(), 2);
    assert_eq!(memory.last_save().unwrap(), persisted);
}

// Catches failing to persist the exact next revision/identity before publishing it.
#[test]
fn successful_update_persists_next_state_and_returns_updated_item() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![manifest("com.alpha")]);
    let result =
        serde_json::to_value(registry.set_enabled("com.alpha", true, "0").unwrap()).unwrap();
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
        error_code(registry.set_enabled("com.alpha", false, "0")),
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
        registry
            .set_enabled("com.alpha", false, "0")
            .unwrap()
            .revision,
        "5"
    );
    assert_eq!(snapshot(&registry)["plugins"][0]["status"], "disabled");
}

// Catches cloning from stale state and replacing another plugin's committed change.
#[test]
fn sequential_changes_to_two_ids_retain_both_values() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![manifest("com.alpha"), manifest("com.zeta")]);
    registry.set_enabled("com.alpha", true, "0").unwrap();
    assert_eq!(
        registry
            .set_enabled("com.zeta", true, "0")
            .unwrap()
            .revision,
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

// Catches flattening identity capacity into persistence failure or committing on rejection.
#[test]
fn capacity_exceeded_rejects_513th_identity_without_save_or_commit() {
    assert_capacity_transaction_is_atomic(7);
}

#[test]
fn capacity_exceeded_takes_precedence_over_revision_exhaustion() {
    assert_capacity_transaction_is_atomic(u64::MAX);
}

fn assert_capacity_transaction_is_atomic(revision: u64) {
    for requires_rewrite in [false, true] {
        let original = PluginStateFileV2 {
            schema_version: 2,
            revision,
            entries: (0..512)
                .map(|n| entry(&format!("com.example.orphan{n}"), "com.easiflux", false))
                .collect(),
        };
        let memory = MemoryPersistence::new(original.clone());
        memory.0.lock().unwrap().loaded.requires_rewrite = requires_rewrite;
        let mut registry = memory.registry_with_builtin();
        let before = registry.catalog_snapshot();
        assert_eq!(
            error_code(registry.set_enabled("com.easiflux.alpha", true, "0")),
            "plugin_state_capacity_exceeded"
        );
        assert_eq!(registry.catalog_snapshot(), before);
        let Runtime::Available {
            state,
            requires_rewrite: marker,
            ..
        } = &registry.runtime
        else {
            panic!("capacity rejection must not disable the registry");
        };
        assert_eq!(state, &original);
        assert_eq!(*marker, requires_rewrite);
        let persisted = memory.0.lock().unwrap();
        assert!(persisted.saves.is_empty());
        assert_eq!(persisted.persisted, original);
    }
}

// Catches overflowing a persisted u64 revision or saving the wrapped revision.
#[test]
fn exhausted_revision_refuses_changes_but_allows_idempotence() {
    let memory = MemoryPersistence::new(PluginStateFileV2 {
        schema_version: 2,
        revision: u64::MAX,
        entries: vec![entry("com.alpha", "com.easiflux", false)],
    });
    let mut registry = memory.registry(vec![manifest("com.alpha")]);
    assert_eq!(
        registry
            .set_enabled("com.alpha", false, "0")
            .unwrap()
            .revision,
        "18446744073709551615"
    );
    assert_eq!(
        error_code(registry.set_enabled("com.alpha", true, "0")),
        "plugin_revision_exhausted"
    );
    assert_eq!(snapshot(&registry)["revision"], "18446744073709551615");
    assert!(memory.0.lock().unwrap().saves.is_empty());
}

fn local(id: &str, name: &str) -> PluginRecord {
    let mut manifest = manifest(id);
    manifest.name = name.into();
    PluginRecord::local_declarative(manifest).unwrap()
}

fn local_entry(record: &PluginRecord, enabled: bool) -> PluginStateEntryV2 {
    let identity = record.identity();
    PluginStateEntryV2 {
        id: identity.id,
        source: identity.source,
        publisher_id: identity.publisher_id,
        approval_fingerprint: identity.approval_fingerprint,
        enabled,
    }
}

// Catches treating empty initial publication as a no-op, or ordering as semantic change.
#[test]
fn first_publish_is_generation_one_and_identical_reload_is_stable() {
    let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = persistence.registry(vec![]);
    let empty = LocalDiscoveryOutcome::available(vec![]);
    assert!(registry.apply_local_discovery(empty.clone()).unwrap());
    assert_eq!(registry.catalog_generation(), 1);
    assert!(!registry.apply_local_discovery(empty).unwrap());
    let a = local("com.alpha", "Alpha");
    let z = local("com.zeta", "Zeta");
    assert!(registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![z.clone(), a.clone()]))
        .unwrap());
    assert!(!registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![a, z]))
        .unwrap());
    assert_eq!(registry.catalog_generation(), 2);
    assert_eq!(snapshot(&registry)["revision"], "0");
    assert!(persistence.0.lock().unwrap().saves.is_empty());
}

// Catches lower-trust collisions replacing built-ins or not contributing to bounded health.
#[test]
fn builtins_win_collisions_and_local_failure_clears_only_local_slice() {
    let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = persistence.registry(vec![manifest("com.middle")]);
    registry
        .apply_local_discovery(
            LocalDiscoveryOutcome::degraded(
                vec![
                    local("com.zeta", "Zeta"),
                    local("com.middle", "Collision"),
                    local("com.alpha", "Alpha"),
                ],
                2,
            )
            .unwrap(),
        )
        .unwrap();
    let value = snapshot(&registry);
    assert_eq!(
        value["localDiscovery"],
        json!({"status":"degraded","rejectedPackageCount":3})
    );
    assert_eq!(
        value["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["manifest"]["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["com.alpha", "com.middle", "com.zeta"]
    );
    assert_eq!(value["plugins"][1]["source"], "builtIn");
    assert_eq!(value["plugins"][1]["manifest"]["name"], "Test plugin");
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::unavailable())
        .unwrap();
    let value = snapshot(&registry);
    assert_eq!(value["plugins"].as_array().unwrap().len(), 1);
    assert_eq!(value["plugins"][0]["manifest"]["id"], "com.middle");
    assert_eq!(value["availability"], "available");
    assert_eq!(value["localDiscovery"]["status"], "unavailable");
    assert_eq!(value["catalogGeneration"], "2");
}

// Catches health-only changes failing to advance generation and collision-count overflow.
#[test]
fn summary_changes_advance_generation_and_collision_count_is_bounded() {
    let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = persistence.registry(vec![manifest("com.alpha")]);
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![]))
        .unwrap();
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::degraded(vec![], 1).unwrap())
        .unwrap();
    assert_eq!(registry.catalog_generation(), 2);
    registry
        .apply_local_discovery(
            LocalDiscoveryOutcome::degraded(vec![local("com.alpha", "Collision")], 256).unwrap(),
        )
        .unwrap();
    assert_eq!(
        snapshot(&registry)["localDiscovery"]["rejectedPackageCount"],
        256
    );
    assert_eq!(registry.catalog_generation(), 3);
}

// Catches partial publication when checked generation addition fails.
#[test]
fn generation_overflow_is_atomic_but_identical_reload_still_succeeds() {
    let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = persistence.registry(vec![]);
    let outcome = LocalDiscoveryOutcome::available(vec![local("com.alpha", "Alpha")]);
    registry.apply_local_discovery(outcome.clone()).unwrap();
    registry.catalog_generation = u64::MAX;
    let before = registry.catalog_snapshot();
    assert!(!registry.apply_local_discovery(outcome).unwrap());
    let error = registry
        .apply_local_discovery(LocalDiscoveryOutcome::unavailable())
        .unwrap_err();
    assert_eq!(
        serde_json::to_value(error).unwrap()["code"],
        "plugin_catalog_generation_exhausted"
    );
    assert_eq!(registry.catalog_snapshot(), before);
    assert!(persistence.0.lock().unwrap().saves.is_empty());
}

// Catches stale/noncanonical tokens reaching lookup, persistence or retry paths.
#[test]
fn stale_and_noncanonical_generation_reject_before_lookup_or_save() {
    let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = persistence.registry(vec![manifest("com.alpha")]);
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![]))
        .unwrap();
    for generation in [
        "0",
        "2",
        "01",
        "",
        "+1",
        " 1",
        "1 ",
        "-1",
        "1.0",
        "18446744073709551616",
        "١",
    ] {
        assert_eq!(
            error_code(registry.set_enabled("INVALID", true, generation)),
            "plugin_catalog_stale",
            "{generation}"
        );
    }
    assert_eq!(persistence.0.lock().unwrap().loads, 1);
    assert!(persistence.0.lock().unwrap().saves.is_empty());
}

// Catches approving changed content by ID and conflating revision with generation.
#[test]
fn content_replacement_defaults_disabled_and_exact_identity_recovers() {
    let original = local("com.alpha", "Original");
    let persistence = MemoryPersistence::new(PluginStateFileV2 {
        schema_version: 2,
        revision: 7,
        entries: vec![local_entry(&original, true)],
    });
    let mut registry = persistence.registry(vec![]);
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![original.clone()]))
        .unwrap();
    assert_eq!(snapshot(&registry)["plugins"][0]["status"], "enabled");
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![local(
            "com.alpha",
            "Replacement",
        )]))
        .unwrap();
    assert_eq!(snapshot(&registry)["plugins"][0]["status"], "disabled");
    assert_eq!(snapshot(&registry)["revision"], "7");
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![original]))
        .unwrap();
    assert_eq!(snapshot(&registry)["plugins"][0]["status"], "enabled");
    let result = registry.set_enabled("com.alpha", false, "3").unwrap();
    assert_eq!(result.revision, "8");
    assert_eq!(result.catalog_generation, "3");
}

// Catches skipping explicit disabled cleanup, retaining old publisher approvals, or pruning other sources.
#[test]
fn explicit_disabled_prunes_all_old_same_source_identities_and_prevents_rollback() {
    let old = local("com.alpha", "Old");
    let mut older_manifest = manifest("com.alpha");
    older_manifest.publisher_id = PluginPublisherId::parse("com.other").unwrap();
    let older = PluginRecord::local_declarative(older_manifest).unwrap();
    let new = local("com.alpha", "New");
    let persistence = MemoryPersistence::new(PluginStateFileV2 {
        schema_version: 2,
        revision: 9,
        entries: vec![
            local_entry(&old, true),
            local_entry(&older, true),
            entry("com.alpha", "com.easiflux", true),
            entry("com.orphan", "com.other", true),
        ],
    });
    let mut registry = persistence.registry(vec![]);
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![new.clone()]))
        .unwrap();
    let result = registry.set_enabled("com.alpha", false, "1").unwrap();
    assert_eq!(result.revision, "10");
    let saved = persistence.last_save().unwrap();
    assert_eq!(saved.entries.len(), 3);
    assert_eq!(saved.entries[0].source, PluginSource::BuiltIn);
    assert_eq!(saved.entries[1], local_entry(&new, false));
    assert_eq!(saved.entries[2].id.as_str(), "com.orphan");
    registry.set_enabled("com.alpha", false, "1").unwrap();
    assert_eq!(persistence.0.lock().unwrap().saves.len(), 1);
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![old]))
        .unwrap();
    assert_eq!(snapshot(&registry)["plugins"][0]["status"], "disabled");
}

// Catches cleanup/migration marker committing before successful durable persistence.
#[test]
fn failed_identity_cleanup_preserves_memory_revision_and_rewrite_marker() {
    let old = local("com.alpha", "Old");
    let new = local("com.alpha", "New");
    let persistence = MemoryPersistence::failing_loaded_from_v1(PluginStateFileV2 {
        schema_version: 2,
        revision: 7,
        entries: vec![local_entry(&old, true)],
    });
    let mut registry = persistence.registry(vec![]);
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![new.clone()]))
        .unwrap();
    let before = registry.catalog_snapshot();
    assert_eq!(
        error_code(registry.set_enabled("com.alpha", false, "1")),
        "plugin_state_persist_failed"
    );
    assert_eq!(registry.catalog_snapshot(), before);
    assert!(matches!(
        registry.runtime,
        Runtime::Available {
            requires_rewrite: true,
            ..
        }
    ));
    assert_eq!(
        persistence.0.lock().unwrap().persisted.entries,
        vec![local_entry(&old, true)]
    );
    persistence.allow_saves();
    assert_eq!(
        registry
            .set_enabled("com.alpha", false, "1")
            .unwrap()
            .revision,
        "8"
    );
    assert_eq!(
        persistence.last_save().unwrap().entries,
        vec![local_entry(&new, false)]
    );
    assert!(matches!(
        registry.runtime,
        Runtime::Available {
            requires_rewrite: false,
            ..
        }
    ));
}
