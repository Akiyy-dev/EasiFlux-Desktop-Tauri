use super::*;
use crate::plugin::discovery::{LocalDiscoveryOutcome, ScanUsage};
use crate::plugin::import::ImportCommitFailure;
use crate::plugin::manifest::{PluginPublisherId, PluginSource};
use crate::storage::plugin_state::{PluginStateEntryV2, PluginStateFileV2, PluginStateLoad};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct OwnershipMemory(
    Arc<
        Mutex<(
            crate::storage::managed_plugin_ownership::ManagedOwnershipLoad,
            bool,
            bool,
        )>,
    >,
);
impl OwnershipMemory {
    fn new(index: crate::plugin::ownership::ManagedOwnershipIndexV1) -> Self {
        Self(Arc::new(Mutex::new((
            crate::storage::managed_plugin_ownership::ManagedOwnershipLoad {
                index,
                requires_rewrite: false,
            },
            false,
            false,
        ))))
    }
}
impl crate::storage::managed_plugin_ownership::ManagedOwnershipPersistence for OwnershipMemory {
    fn load(&self) -> AppResult<crate::storage::managed_plugin_ownership::ManagedOwnershipLoad> {
        let state = self.0.lock().unwrap();
        if state.1 {
            Err(plugin_error("plugin_ownership_unavailable", "unavailable"))
        } else {
            Ok(state.0.clone())
        }
    }
    fn save(
        &self,
        index: &crate::plugin::ownership::ManagedOwnershipIndexV1,
    ) -> crate::storage::safe_plugin_document::PersistResult {
        use crate::storage::safe_plugin_document::{PersistFailure, PersistOutcome};
        let mut state = self.0.lock().unwrap();
        if state.2 {
            return Err(PersistFailure {
                outcome: PersistOutcome::NotCommitted,
            });
        }
        state.0.index = index.clone();
        state.0.requires_rewrite = false;
        Ok(PersistOutcome::CommittedProcessCrashSafe)
    }
}

struct OwnershipFixture(std::path::PathBuf);
impl OwnershipFixture {
    fn new() -> Self {
        let root = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("ownership-discovery-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("local")).unwrap();
        std::fs::create_dir(root.join("removal-staging")).unwrap();
        Self(root.canonicalize().unwrap())
    }
    fn package(
        &self,
    ) -> (
        LocalDiscoveryOutcome,
        crate::plugin::ownership::ManagedOwnershipIndexV1,
    ) {
        use crate::plugin::ownership::*;
        let record = local("com.example.managed", "Managed");
        let slot = PackageSlot::parse("pkg-00000000000000000000000000000001").unwrap();
        let package = self.0.join("local").join(slot.as_str());
        std::fs::create_dir(&package).unwrap();
        std::fs::write(
            package.join("manifest.json"),
            record.canonical_manifest_bytes().unwrap(),
        )
        .unwrap();
        let receipt = OwnershipReceiptV1::new(
            ReceiptId::parse("550e8400e29b41d4a716446655440000").unwrap(),
            slot,
            &record,
        )
        .unwrap();
        std::fs::write(
            package.join("ownership-receipt.json"),
            receipt.canonical_bytes().unwrap(),
        )
        .unwrap();
        let outcome = self.scan();
        let locator = &outcome.plugins[0].locator;
        let entry = ManagedOwnershipEntryV1::managed(
            locator.receipt.clone().unwrap(),
            &record,
            locator.directory_identity,
            locator.manifest_identity,
        )
        .unwrap();
        (
            outcome,
            ManagedOwnershipIndexV1::empty().register(entry).unwrap(),
        )
    }
    fn scan(&self) -> LocalDiscoveryOutcome {
        crate::plugin::discovery::discover_from_plugins_root(&self.0)
    }
}
impl Drop for OwnershipFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn exact_receipt_and_index_and_three_objects_is_managed() {
    let fixture = OwnershipFixture::new();
    let (outcome, index) = fixture.package();
    let mut registry = PluginRegistry::initialize(
        vec![],
        Box::new(MemoryPersistence::new(PluginStateFileV2::empty())),
        Box::new(OwnershipMemory::new(index)),
    );
    registry.apply_local_discovery(outcome).unwrap();
    assert_eq!(snapshot(&registry)["plugins"][0]["management"], "managed");
    assert_eq!(snapshot(&registry)["plugins"][0]["canRemove"], true);
    let before = snapshot(&registry);
    let mutation = serde_json::to_value(
        registry
            .set_enabled("com.example.managed", true, "1")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(mutation["plugin"]["canRemove"], false);
    assert_eq!(mutation["plugin"]["management"], "managed");
    assert_eq!(mutation["catalogGeneration"], before["catalogGeneration"]);
}

// Catches deduplication hiding an invalid sibling or the canonical orphan's conflict.
#[cfg(unix)]
#[test]
fn unix_coexisting_aliases_preserve_managed_local_and_each_quarantine_conflict() {
    let fixture = OwnershipFixture::new();
    let (_, index) = fixture.package();
    match std::fs::create_dir(fixture.0.join("local/PKG-00000000000000000000000000000001")) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return,
        Err(error) => panic!("create distinct case-sensitive sibling: {error}"),
    }
    let staging = fixture.0.join("removal-staging");
    std::fs::create_dir(staging.join("remove-00000000000000000000000000000001")).unwrap();
    std::fs::create_dir(staging.join("REMOVE-00000000000000000000000000000001")).unwrap();
    let outcome = fixture.scan();
    assert_eq!(outcome.plugins.len(), 1);
    assert_eq!(outcome.removals.observations.len(), 1);
    assert_eq!(outcome.occupied_slots.len(), 1);
    assert_eq!(outcome.removals.occupied_slots.len(), 1);
    assert_eq!(outcome.removals.unknown_entry_count, 1);
    let mut registry = PluginRegistry::initialize(
        vec![],
        Box::new(MemoryPersistence::new(PluginStateFileV2::empty())),
        Box::new(OwnershipMemory::new(index)),
    );
    registry.apply_local_discovery(outcome).unwrap();
    let value = snapshot(&registry);
    assert_eq!(value["plugins"][0]["management"], "managed");
    assert_eq!(
        value["localDiscovery"],
        json!({"status": "degraded", "rejectedPackageCount": 1})
    );
    assert_eq!(value["managedOwnership"]["status"], "degraded");
    assert_eq!(value["managedOwnership"]["conflictingEntryCount"], 2);
}

// Catches local ownership leaking into the selected built-in's mutation or gate.
fn assert_builtin_collision_isolated(kind: &str) {
    use crate::plugin::ownership::{ManagedOwnershipIndexV1, RemovalSlot};
    let fixture = OwnershipFixture::new();
    let (_, mut index) = fixture.package();
    if kind == "legacy" {
        std::fs::remove_file(
            fixture
                .0
                .join("local/pkg-00000000000000000000000000000001/ownership-receipt.json"),
        )
        .unwrap();
        index = ManagedOwnershipIndexV1::empty();
    } else if kind == "pending" {
        index = index
            .begin_removal(
                index.entries()[0].receipt_id(),
                RemovalSlot::parse("remove-00000000000000000000000000000001").unwrap(),
            )
            .unwrap();
    }
    let memory = OwnershipMemory::new(index);
    memory.0.lock().unwrap().1 = kind == "unavailable";
    memory.0.lock().unwrap().2 = kind == "pending";
    let mut registry = PluginRegistry::initialize(
        vec![manifest("com.example.managed")],
        Box::new(MemoryPersistence::new(PluginStateFileV2::empty())),
        Box::new(memory),
    );
    registry.apply_local_discovery(fixture.scan()).unwrap();
    for enabled in [false, true] {
        let generation = registry.catalog_snapshot().catalog_generation;
        let mutation = serde_json::to_value(
            registry
                .set_enabled("com.example.managed", enabled, &generation)
                .unwrap(),
        )
        .unwrap();
        let catalog = snapshot(&registry);
        assert_eq!(mutation["plugin"]["management"], "builtIn");
        assert_eq!(mutation["plugin"]["source"], "builtIn");
        assert_eq!(mutation["plugin"]["canToggle"], true);
        assert_eq!(mutation["plugin"]["canRemove"], false);
        assert_eq!(
            mutation["plugin"]["status"],
            if enabled { "enabled" } else { "disabled" }
        );
        assert_eq!(mutation["plugin"], catalog["plugins"][0]);
        assert_eq!(mutation["revision"], catalog["revision"]);
        assert_eq!(mutation["catalogGeneration"], catalog["catalogGeneration"]);
        serde_json::from_value::<PluginCatalogMutationResult>(mutation).unwrap();
    }
    assert!(registry.publication.locals.is_empty());
    assert!(registry.publication.locators.is_empty());
    assert!(registry.publication.management.is_empty());
}

#[test]
fn legacy_collision_cannot_taint_builtin_toggle() {
    assert_builtin_collision_isolated("legacy");
}
#[test]
fn managed_collision_cannot_taint_builtin_toggle() {
    assert_builtin_collision_isolated("managed");
}
#[test]
fn pending_collision_cannot_block_builtin_toggle() {
    assert_builtin_collision_isolated("pending");
}
#[test]
fn unavailable_collision_cannot_taint_builtin_toggle() {
    assert_builtin_collision_isolated("unavailable");
}

#[test]
fn receipt_without_index_is_external() {
    let fixture = OwnershipFixture::new();
    let (outcome, _) = fixture.package();
    let mut registry = MemoryPersistence::new(PluginStateFileV2::empty()).registry(vec![]);
    registry.apply_local_discovery(outcome).unwrap();
    assert_eq!(snapshot(&registry)["plugins"][0]["management"], "external");
    assert_eq!(snapshot(&registry)["plugins"][0]["canRemove"], false);
}

#[test]
fn reload_reloads_previously_available_ownership_instead_of_reusing_stale_authority() {
    let fixture = OwnershipFixture::new();
    let (outcome, index) = fixture.package();
    let memory = OwnershipMemory::new(index);
    let mut registry = PluginRegistry::initialize(
        vec![],
        Box::new(MemoryPersistence::new(PluginStateFileV2::empty())),
        Box::new(memory.clone()),
    );
    registry.apply_local_discovery(outcome.clone()).unwrap();
    assert_eq!(snapshot(&registry)["plugins"][0]["management"], "managed");
    memory.0.lock().unwrap().0.index = crate::plugin::ownership::ManagedOwnershipIndexV1::empty();
    registry.apply_local_discovery(outcome.clone()).unwrap();
    assert_eq!(snapshot(&registry)["plugins"][0]["management"], "external");
    assert_eq!(snapshot(&registry)["catalogGeneration"], "2");
    memory.0.lock().unwrap().1 = true;
    registry.apply_local_discovery(outcome).unwrap();
    assert_eq!(
        snapshot(&registry)["plugins"][0]["management"],
        "ownershipUnavailable"
    );
    assert_eq!(snapshot(&registry)["catalogGeneration"], "3");
}

#[test]
fn two_distinct_packages_claiming_one_receipt_are_one_conflict_not_managed() {
    let fixture = OwnershipFixture::new();
    let (_, index) = fixture.package();
    let second = fixture.0.join("local/pkg-00000000000000000000000000000002");
    std::fs::create_dir(&second).unwrap();
    std::fs::write(
        second.join("manifest.json"),
        local("com.example.copy", "Copy")
            .canonical_manifest_bytes()
            .unwrap(),
    )
    .unwrap();
    std::fs::copy(
        fixture
            .0
            .join("local/pkg-00000000000000000000000000000001/ownership-receipt.json"),
        second.join("ownership-receipt.json"),
    )
    .unwrap();
    let mut registry = PluginRegistry::initialize(
        vec![],
        Box::new(MemoryPersistence::new(PluginStateFileV2::empty())),
        Box::new(OwnershipMemory::new(index)),
    );
    registry.apply_local_discovery(fixture.scan()).unwrap();
    let value = snapshot(&registry);
    assert_eq!(value["managedOwnership"]["conflictingEntryCount"], 1);
    for item in value["plugins"].as_array().unwrap() {
        assert_eq!(item["management"], "ownershipConflict");
        assert_eq!(item["canRemove"], false);
    }
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn wrong_case_occupied_source_or_target_never_means_both_absent() {
    for target in [false, true] {
        let (fixture, memory, mut registry) = pending_fixture(false);
        let source = fixture.0.join("local/pkg-00000000000000000000000000000001");
        let destination = if target {
            fixture
                .0
                .join("removal-staging/REMOVE-00000000000000000000000000000001")
        } else {
            fixture.0.join("local/PKG-00000000000000000000000000000001")
        };
        std::fs::rename(source, &destination).unwrap();
        #[cfg(target_os = "macos")]
        if !destination
            .with_file_name(
                destination
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_ascii_lowercase(),
            )
            .exists()
        {
            // This fixture volume is case-sensitive; the Unix test below covers absence.
            continue;
        }
        registry.apply_local_discovery(fixture.scan()).unwrap();
        assert_eq!(memory.0.lock().unwrap().0.index.entries().len(), 1);
        assert_eq!(
            snapshot(&registry)["managedOwnership"]["conflictingEntryCount"],
            1
        );
        assert!(destination.is_dir());
    }
}

// Catches alias-only target presence permitting an exact-source rollback on macOS.
#[cfg(target_os = "macos")]
#[test]
fn macos_target_alias_prevents_removing_to_managed_rollback() {
    let (fixture, memory, mut registry) = pending_fixture(false);
    let staging = fixture.0.join("removal-staging");
    std::fs::create_dir(staging.join("REMOVE-00000000000000000000000000000001")).unwrap();
    if !staging
        .join("remove-00000000000000000000000000000001")
        .exists()
    {
        return; // Requires a case-insensitive fixture volume.
    }
    let before = memory.0.lock().unwrap().0.index.clone();
    registry.apply_local_discovery(fixture.scan()).unwrap();
    assert_eq!(memory.0.lock().unwrap().0.index, before);
    assert_eq!(
        snapshot(&registry)["managedOwnership"]["conflictingEntryCount"],
        1
    );
}

// Catches conservatively folding unrelated names even when Unix proves canonical absence.
#[cfg(unix)]
#[test]
fn case_sensitive_unix_alias_does_not_prevent_confirmed_absent_cleanup() {
    for target in [false, true] {
        let (fixture, memory, mut registry) = pending_fixture(false);
        let source = fixture.0.join("local/pkg-00000000000000000000000000000001");
        let destination = if target {
            fixture
                .0
                .join("removal-staging/REMOVE-00000000000000000000000000000001")
        } else {
            fixture.0.join("local/PKG-00000000000000000000000000000001")
        };
        std::fs::rename(source, &destination).unwrap();
        let canonical = destination.with_file_name(
            destination
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_ascii_lowercase(),
        );
        if canonical.exists() {
            continue; // This volume is case-insensitive, so absence is not confirmed.
        }
        registry.apply_local_discovery(fixture.scan()).unwrap();
        assert!(memory.0.lock().unwrap().0.index.entries().is_empty());
        assert!(destination.is_dir());
    }
}

#[test]
fn recovered_index_does_not_authorize_until_safe_rewrite() {
    let fixture = OwnershipFixture::new();
    let (outcome, index) = fixture.package();
    let memory = OwnershipMemory::new(index);
    {
        let mut state = memory.0.lock().unwrap();
        state.0.requires_rewrite = true;
        state.2 = true;
    }
    let mut registry = PluginRegistry::initialize(
        vec![],
        Box::new(MemoryPersistence::new(PluginStateFileV2::empty())),
        Box::new(memory.clone()),
    );
    registry.apply_local_discovery(outcome.clone()).unwrap();
    assert_eq!(
        snapshot(&registry)["plugins"][0]["management"],
        "ownershipUnavailable"
    );
    memory.0.lock().unwrap().2 = false;
    registry.apply_local_discovery(outcome).unwrap();
    assert_eq!(snapshot(&registry)["plugins"][0]["management"], "managed");
    assert_eq!(snapshot(&registry)["catalogGeneration"], "2");
}

#[test]
fn removal_shapes_are_complete_and_mutually_exclusive() {
    use crate::plugin::ownership::{ManagedOwnershipIndexV1, RemovalSlot};
    for (shape, failed_save, want) in [
        ("full", false, [0, 0, 1]),
        ("receipt", false, [0, 0, 1]),
        ("empty", false, [0, 0, 1]),
        ("absent", true, [0, 0, 1]),
        ("absent", false, [0, 0, 0]),
        ("source", true, [0, 1, 0]),
        ("source", false, [0, 0, 0]),
        ("manifest", false, [1, 0, 0]),
        ("extra", false, [1, 0, 0]),
        ("both", false, [1, 0, 0]),
        ("identity", false, [1, 0, 0]),
        ("orphan", false, [1, 0, 0]),
        ("unconfirmed", false, [1, 0, 0]),
    ] {
        let fixture = OwnershipFixture::new();
        let (_, mut index) = fixture.package();
        let receipt = index.entries()[0].receipt_id().clone();
        index = index
            .begin_removal(
                &receipt,
                RemovalSlot::parse("remove-00000000000000000000000000000001").unwrap(),
            )
            .unwrap();
        let source = fixture.0.join("local/pkg-00000000000000000000000000000001");
        let target = fixture
            .0
            .join("removal-staging/remove-00000000000000000000000000000001");
        if shape != "source" {
            std::fs::rename(&source, &target).unwrap();
        }
        match shape {
            "receipt" => std::fs::remove_file(target.join("manifest.json")).unwrap(),
            "manifest" => std::fs::remove_file(target.join("ownership-receipt.json")).unwrap(),
            "empty" | "absent" => {
                std::fs::remove_file(target.join("manifest.json")).unwrap();
                std::fs::remove_file(target.join("ownership-receipt.json")).unwrap();
                if shape == "absent" {
                    std::fs::remove_dir(&target).unwrap();
                }
            }
            "extra" => std::fs::write(target.join("extra"), b"preserve").unwrap(),
            "both" => {
                std::fs::create_dir(&source).unwrap();
            }
            "identity" => {
                let bytes = std::fs::read(target.join("manifest.json")).unwrap();
                std::fs::rename(target.join("manifest.json"), fixture.0.join("old-manifest"))
                    .unwrap();
                std::fs::write(target.join("manifest.json"), bytes).unwrap();
            }
            "orphan" => index = ManagedOwnershipIndexV1::empty(),
            "unconfirmed" => std::fs::write(target.join("ownership-receipt.json"), b"{}").unwrap(),
            _ => {}
        }
        let memory = OwnershipMemory::new(index);
        memory.0.lock().unwrap().2 = failed_save;
        let mut registry = PluginRegistry::initialize(
            vec![],
            Box::new(MemoryPersistence::new(PluginStateFileV2::empty())),
            Box::new(memory),
        );
        registry.apply_local_discovery(fixture.scan()).unwrap();
        let snapshot = snapshot(&registry);
        let summary = &snapshot["managedOwnership"];
        assert_eq!(
            [
                summary["conflictingEntryCount"].as_u64().unwrap(),
                summary["rollbackPendingCount"].as_u64().unwrap(),
                summary["cleanupPendingCount"].as_u64().unwrap()
            ],
            want,
            "{shape}/{failed_save}"
        );
        assert_eq!(
            summary["status"],
            if want == [0, 0, 0] {
                "available"
            } else {
                "degraded"
            }
        );
        if shape != "source" && shape != "absent" {
            assert!(target.is_dir(), "scanner deleted {shape}");
        }
        if shape == "source" && failed_save {
            assert_eq!(snapshot["plugins"][0]["management"], "removalPending");
            assert_eq!(
                snapshot["plugins"][0]["toggleBlockReasonCode"],
                "removalPending"
            );
            assert!(registry
                .set_enabled("com.example.managed", true, "1")
                .is_err());
        }
    }
}

#[test]
fn legacy_manifest_stays_external_when_index_is_unavailable() {
    ownership_unavailable_case(false);
}
#[test]
fn receipted_candidate_becomes_ownership_unavailable_when_index_is_unavailable() {
    ownership_unavailable_case(true);
}
fn ownership_unavailable_case(receipted: bool) {
    let fixture = OwnershipFixture::new();
    let (_, index) = fixture.package();
    if !receipted {
        std::fs::remove_file(
            fixture
                .0
                .join("local/pkg-00000000000000000000000000000001/ownership-receipt.json"),
        )
        .unwrap();
    }
    let memory = OwnershipMemory::new(index);
    memory.0.lock().unwrap().1 = true;
    let mut registry = PluginRegistry::initialize(
        vec![manifest("com.alpha")],
        Box::new(MemoryPersistence::new(PluginStateFileV2::empty())),
        Box::new(memory),
    );
    registry.apply_local_discovery(fixture.scan()).unwrap();
    let value = snapshot(&registry);
    assert_eq!(value["availability"], "available");
    assert_eq!(value["plugins"][0]["management"], "builtIn");
    assert_eq!(
        value["plugins"][1]["management"],
        if receipted {
            "ownershipUnavailable"
        } else {
            "external"
        }
    );
    assert_eq!(
        value["managedOwnership"],
        json!({"status":"unavailable", "conflictingEntryCount":0, "rollbackPendingCount":0, "cleanupPendingCount":0})
    );
    assert!(registry
        .set_enabled("com.example.managed", true, "1")
        .is_ok());
}

#[test]
fn index_without_matching_package_is_conflict() {
    let fixture = OwnershipFixture::new();
    let (_, index) = fixture.package();
    std::fs::rename(
        fixture.0.join("local/pkg-00000000000000000000000000000001"),
        fixture.0.join("kept-external"),
    )
    .unwrap();
    let mut registry = PluginRegistry::initialize(
        vec![],
        Box::new(MemoryPersistence::new(PluginStateFileV2::empty())),
        Box::new(OwnershipMemory::new(index)),
    );
    registry.apply_local_discovery(fixture.scan()).unwrap();
    assert_eq!(
        snapshot(&registry)["managedOwnership"]["conflictingEntryCount"],
        1
    );
}

#[test]
fn managed_disabled_only_is_removable() {
    let fixture = OwnershipFixture::new();
    let (outcome, index) = fixture.package();
    let state = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = PluginRegistry::initialize(
        vec![],
        Box::new(state.clone()),
        Box::new(OwnershipMemory::new(index)),
    );
    registry.apply_local_discovery(outcome).unwrap();
    for (enabled, expected) in [(false, true), (true, false), (false, true)] {
        let mutation = serde_json::to_value(
            registry
                .set_enabled("com.example.managed", enabled, "1")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(mutation["plugin"]["canRemove"], expected);
        assert_eq!(mutation["catalogGeneration"], "1");
    }
}

#[test]
fn mutation_keeps_generation_and_every_structural_ownership_field() {
    let fixture = OwnershipFixture::new();
    let (outcome, index) = fixture.package();
    let mut registry = PluginRegistry::initialize(
        vec![],
        Box::new(MemoryPersistence::new(PluginStateFileV2::empty())),
        Box::new(OwnershipMemory::new(index)),
    );
    registry.apply_local_discovery(outcome).unwrap();
    let before = registry.catalog_snapshot();
    let mutation = registry
        .set_enabled("com.example.managed", true, "1")
        .unwrap();
    assert!(mutation.plugin.same_structure(&before.plugins[0]));
    assert_eq!(mutation.catalog_generation, before.catalog_generation);
    assert_eq!(
        registry.catalog_snapshot().managed_ownership,
        before.managed_ownership
    );
    assert_ne!(mutation.revision, before.revision);
}

#[test]
fn management_eligibility_toggle_block_or_summary_change_advances_generation() {
    let record = local("com.example.local", "Local");
    for management in [
        PluginManagement::Managed,
        PluginManagement::RemovalPending,
        PluginManagement::OwnershipConflict,
    ] {
        let mut registry = MemoryPersistence::new(PluginStateFileV2::empty()).registry(vec![]);
        registry
            .apply_local_discovery(LocalDiscoveryOutcome::available(vec![record.clone()]))
            .unwrap();
        let mut candidate = registry.publication.clone();
        candidate
            .management
            .insert(record.manifest().id.clone(), management);
        registry.publish_candidate(candidate, false).unwrap();
        assert_eq!(registry.catalog_generation(), 2);
    }
    let mut registry = MemoryPersistence::new(PluginStateFileV2::empty()).registry(vec![]);
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![]))
        .unwrap();
    let mut candidate = registry.publication.clone();
    candidate.ownership_summary = ManagedOwnershipSummary::counts(1, 0, 0);
    registry.publish_candidate(candidate, false).unwrap();
    assert_eq!(registry.catalog_generation(), 2);
}

fn pending_fixture(fail_save: bool) -> (OwnershipFixture, OwnershipMemory, PluginRegistry) {
    let fixture = OwnershipFixture::new();
    let (_, mut index) = fixture.package();
    let receipt = index.entries()[0].receipt_id().clone();
    index = index
        .begin_removal(
            &receipt,
            crate::plugin::ownership::RemovalSlot::parse("remove-00000000000000000000000000000001")
                .unwrap(),
        )
        .unwrap();
    let memory = OwnershipMemory::new(index);
    memory.0.lock().unwrap().2 = fail_save;
    let registry = PluginRegistry::initialize(
        vec![],
        Box::new(MemoryPersistence::new(PluginStateFileV2::empty())),
        Box::new(memory.clone()),
    );
    (fixture, memory, registry)
}

#[test]
fn removing_exact_source_without_target_is_rollback_pending() {
    let (fixture, memory, mut registry) = pending_fixture(true);
    registry.apply_local_discovery(fixture.scan()).unwrap();
    let value = snapshot(&registry);
    assert_eq!(value["managedOwnership"]["rollbackPendingCount"], 1);
    assert_eq!(value["plugins"][0]["status"], "disabled");
    assert_eq!(value["plugins"][0]["canToggle"], false);
    assert!(memory.0.lock().unwrap().0.index.entries()[0]
        .removal_slot()
        .is_some());
}

#[test]
fn no_delete_reconcile_rolls_back_exact_source_or_deletes_both_absent_entry() {
    for absent in [false, true] {
        let (fixture, memory, mut registry) = pending_fixture(false);
        let source = fixture.0.join("local/pkg-00000000000000000000000000000001");
        if absent {
            std::fs::rename(&source, fixture.0.join("kept-test-evidence")).unwrap();
        }
        registry.apply_local_discovery(fixture.scan()).unwrap();
        let persisted = memory.0.lock().unwrap().0.index.clone();
        if absent {
            assert!(persisted.entries().is_empty());
            assert!(fixture.0.join("kept-test-evidence/manifest.json").is_file());
        } else {
            assert!(persisted.entries()[0].removal_slot().is_none());
            assert!(source.join("ownership-receipt.json").is_file());
        }
        assert_eq!(
            snapshot(&registry)["managedOwnership"]["status"],
            "available"
        );
    }
}

#[test]
fn one_reconcile_swaps_once_without_observable_intermediate_summary() {
    let (fixture, memory, mut registry) = pending_fixture(true);
    registry.apply_local_discovery(fixture.scan()).unwrap();
    let before = registry.catalog_snapshot();
    assert_eq!(before.managed_ownership.rollback_pending_count, 1);
    memory.0.lock().unwrap().2 = false;
    registry.apply_local_discovery(fixture.scan()).unwrap();
    let after = registry.catalog_snapshot();
    assert_eq!(after.catalog_generation, "2");
    assert_eq!(
        after.managed_ownership,
        ManagedOwnershipSummary::available()
    );
    assert_eq!(
        serde_json::to_value(&after).unwrap()["plugins"][0]["management"],
        "managed"
    );
    assert_eq!(before.catalog_generation, "1");
    assert_eq!(before.managed_ownership.rollback_pending_count, 1);
}

#[test]
fn slot_receipt_or_object_change_advances_generation() {
    let fixture = OwnershipFixture::new();
    let (original, _) = fixture.package();
    for which in ["slot", "receipt", "directory", "manifest"] {
        let mut registry = PluginRegistry::initialize(
            vec![],
            Box::new(MemoryPersistence::new(PluginStateFileV2::empty())),
            crate::plugin::ownership::empty_test_persistence(),
        );
        registry.apply_local_discovery(original.clone()).unwrap();
        let before = snapshot(&registry);
        let mut changed = original.clone();
        let locator = &mut changed.plugins[0].locator;
        match which {
            "slot" => {
                locator.package_slot = crate::plugin::ownership::PackageSlot::parse(
                    "pkg-00000000000000000000000000000002",
                )
                .unwrap()
            }
            "receipt" => locator.receipt = None,
            "directory" => locator.directory_identity.object += 1,
            _ => locator.manifest_identity.object += 1,
        }
        registry.apply_local_discovery(changed).unwrap();
        assert_eq!(snapshot(&registry)["catalogGeneration"], "2", "{which}");
        assert_eq!(
            snapshot(&registry)["plugins"],
            before["plugins"],
            "hidden locator change must still advance generation"
        );
    }
}

#[test]
fn candidate_generation_overflow_preserves_the_entire_old_snapshot() {
    let fixture = OwnershipFixture::new();
    let (original, index) = fixture.package();
    let mut registry = PluginRegistry::initialize(
        vec![],
        Box::new(MemoryPersistence::new(PluginStateFileV2::empty())),
        Box::new(OwnershipMemory::new(index)),
    );
    registry.apply_local_discovery(original.clone()).unwrap();
    registry.set_catalog_generation_for_test(u64::MAX);
    let previous = registry.publication.clone();
    let mut changed = original;
    changed.plugins[0].locator.receipt = None;
    assert!(registry.apply_local_discovery(changed).is_err());
    assert_eq!(registry.catalog_snapshot(), previous.snapshot);
    assert_eq!(registry.publication.locals, previous.locals);
    assert_eq!(registry.publication.locators, previous.locators);
    assert_eq!(registry.publication.management, previous.management);
    assert_eq!(
        registry.publication.ownership.entries(),
        previous.ownership.entries()
    );
}

#[test]
fn equal_revision_and_generation_imply_equal_complete_dto() {
    let mut registry =
        MemoryPersistence::new(PluginStateFileV2::empty()).registry(vec![manifest("com.alpha")]);
    let mut published = BTreeMap::new();
    for enabled in [false, false, true, true, false] {
        let generation = registry.catalog_generation().to_string();
        registry
            .set_enabled("com.alpha", enabled, &generation)
            .unwrap();
        let snapshot = registry.catalog_snapshot();
        let key = (
            snapshot.revision.clone(),
            snapshot.catalog_generation.clone(),
        );
        if let Some(previous) = published.insert(key, snapshot.clone()) {
            assert_eq!(previous, snapshot);
        }
    }
}

#[test]
fn status_only_toggle_changes_can_remove_with_revision_only() {
    let mut registry =
        MemoryPersistence::new(PluginStateFileV2::empty()).registry(vec![manifest("com.alpha")]);
    let before = snapshot(&registry);
    let result =
        serde_json::to_value(registry.set_enabled("com.alpha", true, "0").unwrap()).unwrap();
    assert_eq!(result["catalogGeneration"], before["catalogGeneration"]);
    assert_eq!(result["plugin"]["canRemove"], false);
    assert_eq!(result["plugin"]["management"], "builtIn");
}

#[test]
fn postcommit_save_failure_adopts_the_committed_decision() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    memory.0.lock().unwrap().postcommit_error = true;
    let mut registry = memory.registry(vec![manifest("com.alpha")]);
    assert!(registry.set_enabled("com.alpha", true, "0").is_err());
    assert_eq!(snapshot(&registry)["revision"], "1");
    assert_eq!(snapshot(&registry)["plugins"][0]["status"], "enabled");
}

#[test]
fn every_persistence_outcome_controls_memory_adoption_independently_of_error() {
    use crate::storage::safe_plugin_document::{PersistFailure, PersistOutcome, PersistResult};
    struct OutcomePersistence(PersistOutcome, bool);
    impl PluginStatePersistence for OutcomePersistence {
        fn load(&self) -> AppResult<PluginStateLoad> {
            Ok(PluginStateLoad {
                state: PluginStateFileV2::empty(),
                requires_rewrite: false,
            })
        }
        fn save(&self, _: &PluginStateFileV2) -> PersistResult {
            if self.1 {
                Err(PersistFailure { outcome: self.0 })
            } else {
                Ok(self.0)
            }
        }
    }
    for outcome in [
        PersistOutcome::NotCommitted,
        PersistOutcome::CommittedProcessCrashSafe,
        PersistOutcome::CommittedDurable,
    ] {
        for failure in [false, true] {
            let mut registry = PluginRegistry::initialize(
                vec![manifest("com.alpha")],
                Box::new(OutcomePersistence(outcome, failure)),
                crate::plugin::ownership::empty_test_persistence(),
            );
            let result = registry.set_enabled("com.alpha", true, "0");
            let committed = outcome != PersistOutcome::NotCommitted;
            assert_eq!(result.is_err(), failure || !committed);
            assert_eq!(
                snapshot(&registry)["revision"],
                if committed { "1" } else { "0" }
            );
            assert_eq!(
                snapshot(&registry)["plugins"][0]["status"],
                if committed { "enabled" } else { "disabled" }
            );
        }
    }
}

// Only the external storage boundary is replaced: registry derivation,
// validation, cloning and committing remain real production behavior.
#[derive(Clone)]
struct MemoryPersistence(Arc<Mutex<MemoryState>>);

struct MemoryState {
    persisted: PluginStateFileV2,
    loaded: PluginStateLoad,
    load_error: bool,
    save_error: bool,
    postcommit_error: bool,
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
            postcommit_error: false,
            loads: 0,
            saves: vec![],
        })))
    }

    fn registry(&self, builtins: Vec<PluginManifestV1>) -> PluginRegistry {
        PluginRegistry::initialize(
            builtins,
            Box::new(self.clone()),
            crate::plugin::ownership::empty_test_persistence(),
        )
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

    fn save(
        &self,
        state: &PluginStateFileV2,
    ) -> crate::storage::safe_plugin_document::PersistResult {
        let mut memory = self.0.lock().unwrap();
        memory.saves.push(state.clone());
        if memory.save_error {
            return Err(storage_error().into());
        }
        state
            .validate_for_persistence()
            .map_err(|_| storage_error())?;
        memory.persisted = state.clone();
        memory.loaded = PluginStateLoad {
            state: state.clone(),
            requires_rewrite: false,
        };
        if memory.postcommit_error {
            return Err(crate::storage::safe_plugin_document::PersistFailure {
                outcome:
                    crate::storage::safe_plugin_document::PersistOutcome::CommittedProcessCrashSafe,
            });
        }
        Ok(if cfg!(windows) {
            crate::storage::safe_plugin_document::PersistOutcome::CommittedProcessCrashSafe
        } else {
            crate::storage::safe_plugin_document::PersistOutcome::CommittedDurable
        })
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
            "schemaVersion": 3, "revision": "0", "catalogGeneration": "0",
            "availability": "available", "availabilityReasonCode": null,
            "localDiscovery": {"status": "available", "rejectedPackageCount": 0}, "plugins": [],
            "managedOwnership": {"status":"available","conflictingEntryCount":0,"rollbackPendingCount":0,"cleanupPendingCount":0}
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
            matches!(registry.publication.runtime, Runtime::Available { requires_rewrite, .. } if requires_rewrite == migrated)
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
        } = &registry.publication.runtime
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
    registry.set_catalog_generation_for_test(u64::MAX);
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
        registry.publication.runtime,
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
        registry.publication.runtime,
        Runtime::Available {
            requires_rewrite: false,
            ..
        }
    ));
}

// Catches orphan enabled decisions surviving an import, or optimistic catalog insertion.
#[test]
fn import_disabled_replaces_orphan_authorization_without_adding_catalog_item() {
    let local = local("com.easiflux.local", "Local");
    let memory = MemoryPersistence::new(PluginStateFileV2 {
        schema_version: 2,
        revision: 7,
        entries: vec![local_entry(&local, true)],
    });
    let mut registry = memory.registry(vec![]);
    registry.persist_import_disabled(&local).unwrap();
    assert!(registry.catalog_snapshot().plugins.is_empty());
    assert_eq!(registry.catalog_generation(), 0);
    assert_eq!(snapshot(&registry)["revision"], "8");
    let saved = memory.last_save().unwrap();
    assert_eq!(saved.revision, 8);
    assert_eq!(saved.entries, vec![local_entry(&local, false)]);
    assert_eq!(
        error_code(registry.set_enabled("com.easiflux.local", true, "0")),
        "plugin_not_found"
    );
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![local]))
        .unwrap();
    assert_eq!(snapshot(&registry)["plugins"][0]["status"], "disabled");
}

// Catches missing explicit decisions for fresh imports.
#[test]
fn import_disabled_records_fresh_identity_without_membership() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![]);
    let record = local("com.fresh", "Fresh");
    registry.persist_import_disabled(&record).unwrap();
    assert_eq!(
        memory.last_save().unwrap().entries,
        vec![local_entry(&record, false)]
    );
    assert_eq!(snapshot(&registry)["revision"], "1");
    assert!(registry.catalog_snapshot().plugins.is_empty());
}

fn historical_import_state(revision: u64) -> (PluginRecord, PluginStateFileV2) {
    let old = local("com.alpha", "Old");
    let mut older = manifest("com.alpha");
    older.publisher_id = PluginPublisherId::parse("com.other").unwrap();
    let older = PluginRecord::local_declarative(older).unwrap();
    (
        local("com.alpha", "New"),
        PluginStateFileV2 {
            schema_version: 2,
            revision,
            entries: vec![
                local_entry(&old, true),
                local_entry(&older, true),
                entry("com.alpha", "com.easiflux", true),
                entry("com.orphan", "com.other", true),
            ],
        },
    )
}

// Catches retaining historical publisher/fingerprint authorizations or deleting other sources.
#[test]
fn import_disabled_cleans_all_historical_local_identities_and_sorts() {
    let (record, state) = historical_import_state(9);
    let memory = MemoryPersistence::new(state);
    let mut registry = memory.registry(vec![]);
    registry.persist_import_disabled(&record).unwrap();
    let saved = memory.last_save().unwrap();
    assert_eq!(saved.revision, 10);
    assert_eq!(
        saved.entries,
        vec![
            entry("com.alpha", "com.easiflux", true),
            local_entry(&record, false),
            entry("com.orphan", "com.other", true),
        ]
    );
}

// Catches committing cleanup, revision, or recovery marker before the save succeeds.
#[test]
fn import_disabled_save_failure_preserves_all_identities_revision_and_rewrite() {
    let (record, state) = historical_import_state(7);
    let memory = MemoryPersistence::failing_loaded_from_v1(state.clone());
    let mut registry = memory.registry(vec![]);
    let before = registry.catalog_snapshot();
    let error = registry.persist_import_disabled(&record).unwrap_err().error;
    assert_eq!(
        serde_json::to_value(error).unwrap()["code"],
        "plugin_state_persist_failed"
    );
    assert_eq!(registry.catalog_snapshot(), before);
    match &registry.publication.runtime {
        Runtime::Available {
            state: current,
            requires_rewrite,
            ..
        } => {
            assert_eq!(current, &state);
            assert!(*requires_rewrite);
        }
        _ => panic!("available state must survive failure"),
    }
    assert_eq!(memory.0.lock().unwrap().persisted, state);
    memory.allow_saves();
    registry.persist_import_disabled(&record).unwrap();
    assert_eq!(memory.last_save().unwrap().revision, 8);
    assert!(matches!(
        registry.publication.runtime,
        Runtime::Available {
            requires_rewrite: false,
            ..
        }
    ));
}

// Catches revision exhaustion masking capacity, pruning unrelated identities, or partial commit.
#[test]
fn import_disabled_rejects_513th_retained_identity_before_revision_exhaustion() {
    let state = PluginStateFileV2 {
        schema_version: 2,
        revision: u64::MAX,
        entries: (0..512)
            .map(|n| entry(&format!("com.retained{n}"), "com.easiflux", true))
            .collect(),
    };
    let memory = MemoryPersistence::new(state.clone());
    let mut registry = memory.registry(vec![]);
    let error = registry
        .persist_import_disabled(&local("com.new", "New"))
        .unwrap_err()
        .error;
    assert_eq!(
        serde_json::to_value(error).unwrap()["code"],
        "plugin_state_capacity_exceeded"
    );
    assert!(
        matches!(&registry.publication.runtime, Runtime::Available { state: current, .. } if current == &state)
    );
    assert!(memory.0.lock().unwrap().saves.is_empty());
}

// Catches checking capacity before replacing same-ID/source history.
#[test]
fn import_disabled_cleanup_frees_identity_capacity() {
    let (record, mut state) = historical_import_state(7);
    state
        .entries
        .extend((0..508).map(|n| entry(&format!("com.retained{n}"), "com.easiflux", true)));
    let memory = MemoryPersistence::new(state);
    let mut registry = memory.registry(vec![]);
    registry.persist_import_disabled(&record).unwrap();
    let saved = memory.last_save().unwrap();
    assert_eq!(saved.entries.len(), 511);
    assert_eq!(saved.revision, 8);
    assert_eq!(
        saved
            .entries
            .iter()
            .filter(|entry| entry.source == PluginSource::LocalDeclarative)
            .collect::<Vec<_>>(),
        vec![&local_entry(&record, false)]
    );
}

// Catches redundant saves and revision increments for an already durable disabled identity.
#[test]
fn import_disabled_same_disabled_is_noop_even_at_max_revision() {
    let record = local("com.alpha", "Alpha");
    let memory = MemoryPersistence::new(PluginStateFileV2 {
        schema_version: 2,
        revision: u64::MAX,
        entries: vec![local_entry(&record, false)],
    });
    let mut registry = memory.registry(vec![]);
    registry.persist_import_disabled(&record).unwrap();
    assert_eq!(snapshot(&registry)["revision"], u64::MAX.to_string());
    assert!(memory.0.lock().unwrap().saves.is_empty());
}

// Catches skipping recovery rewrite or incrementing a rewrite-only revision at MAX.
#[test]
fn import_disabled_same_disabled_rewrites_at_max_and_retains_marker_on_failure() {
    let record = local("com.alpha", "Alpha");
    let state = PluginStateFileV2 {
        schema_version: 2,
        revision: u64::MAX,
        entries: vec![local_entry(&record, false)],
    };
    let memory = MemoryPersistence::failing_loaded_from_v1(state.clone());
    let mut registry = memory.registry(vec![]);
    assert!(registry.persist_import_disabled(&record).is_err());
    assert!(
        matches!(&registry.publication.runtime, Runtime::Available { state: current, requires_rewrite: true, .. } if current == &state)
    );
    memory.allow_saves();
    registry.persist_import_disabled(&record).unwrap();
    assert_eq!(memory.last_save().unwrap(), state);
    assert!(matches!(
        registry.publication.runtime,
        Runtime::Available {
            requires_rewrite: false,
            ..
        }
    ));
    registry.persist_import_disabled(&record).unwrap();
    assert_eq!(memory.0.lock().unwrap().saves.len(), 2);
}

// Catches accepting a genuine logical decision without revision headroom.
#[test]
fn import_disabled_changed_decision_rejects_max_revision() {
    let memory = MemoryPersistence::new(PluginStateFileV2 {
        revision: u64::MAX,
        ..PluginStateFileV2::empty()
    });
    let mut registry = memory.registry(vec![]);
    let before = registry.catalog_snapshot();
    let error = registry
        .persist_import_disabled(&local("com.new", "New"))
        .unwrap_err()
        .error;
    assert_eq!(
        serde_json::to_value(error).unwrap()["code"],
        "plugin_revision_exhausted"
    );
    assert_eq!(registry.catalog_snapshot(), before);
    assert!(memory.0.lock().unwrap().saves.is_empty());
}

// Catches broadening the internal import writer into arbitrary-source authorization.
#[test]
fn import_disabled_rejects_builtin_records_without_saving() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![]);
    let record = PluginRecord::built_in(manifest("com.alpha")).unwrap();
    assert!(registry.persist_import_disabled(&record).is_err());
    assert_eq!(snapshot(&registry)["revision"], "0");
    assert!(memory.0.lock().unwrap().saves.is_empty());
}

// Catches internal-only usage being compared for generation or added to catalog IPC.
#[test]
fn budget_only_change_does_not_increment_catalog_generation() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![]);
    let mut outcome = LocalDiscoveryOutcome::available(vec![local("com.alpha", "Alpha")]);
    registry.apply_local_discovery(outcome.clone()).unwrap();
    let before = snapshot(&registry);
    outcome.usage = Some(ScanUsage {
        root_entries: 255,
        packages: 127,
        bytes_read: 2_000_000,
    });
    assert!(!registry.apply_local_discovery(outcome).unwrap());
    assert_eq!(snapshot(&registry), before);
    assert_eq!(registry.catalog_generation(), 1);
}

// Catches only checking local conflicts or accepting an identical installed manifest.
#[test]
fn validate_import_rejects_builtin_and_current_local_id_conflicts() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![manifest("com.builtin")]);
    let installed = local("com.installed", "Installed");
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![installed.clone()]))
        .unwrap();
    for record in [local("com.builtin", "Builtin collision"), installed] {
        assert_eq!(
            registry.validate_import(&record, ScanUsage::default()),
            Err(ImportCommitFailure::IdConflict)
        );
    }
    assert!(memory.0.lock().unwrap().saves.is_empty());
}

// Catches treating partial/failed discovery as sufficient proof of conflict-free capacity.
#[test]
fn validate_import_rejects_unhealthy_local_discovery() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![]);
    for outcome in [
        LocalDiscoveryOutcome::degraded(vec![], 1).unwrap(),
        LocalDiscoveryOutcome::unavailable(),
    ] {
        registry.apply_local_discovery(outcome).unwrap();
        assert_eq!(
            registry.validate_import(&local("com.new", "New"), ScanUsage::default()),
            Err(ImportCommitFailure::DiscoveryUnavailable)
        );
    }
    assert!(memory.0.lock().unwrap().saves.is_empty());
}

// Catches importing while blocked, and retrying a permanently invalid catalog.
#[test]
fn import_preflight_and_disabled_writer_reject_unavailable_runtime() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    memory.0.lock().unwrap().load_error = true;
    let record = local("com.new", "New");
    let mut registry = memory.registry(vec![]);
    assert!(registry.state_requires_retry());
    assert_eq!(
        registry.validate_import(&record, ScanUsage::default()),
        Err(ImportCommitFailure::StateUnavailable)
    );
    let error = registry.persist_import_disabled(&record).unwrap_err().error;
    assert_eq!(
        serde_json::to_value(error).unwrap()["code"],
        "plugin_state_unavailable"
    );
    memory.0.lock().unwrap().load_error = false;
    registry.retry_state_load();
    assert!(!registry.state_requires_retry());
    let mut invalid = memory.registry(vec![manifest("com.dup"), manifest("com.dup")]);
    assert!(!invalid.state_requires_retry());
    assert_eq!(
        invalid.validate_import(&record, ScanUsage::default()),
        Err(ImportCommitFailure::CatalogInvalid)
    );
    let error = invalid.persist_import_disabled(&record).unwrap_err().error;
    assert_eq!(
        serde_json::to_value(error).unwrap()["code"],
        "plugin_catalog_invalid"
    );
    assert!(memory.0.lock().unwrap().saves.is_empty());
}

// Catches admission bypassing any scan budget, or measuring something other than canonical bytes.
#[test]
fn validate_import_enforces_directory_and_canonical_byte_capacity() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![]);
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![]))
        .unwrap();
    let record = local("com.new", "New");
    let bytes = record.canonical_manifest_bytes().unwrap().len();
    let exact = ScanUsage {
        root_entries: 255,
        packages: 127,
        bytes_read: 2_097_152 - bytes,
    };
    assert_eq!(registry.validate_import(&record, exact), Ok(()));
    for usage in [
        ScanUsage {
            root_entries: 256,
            ..exact
        },
        ScanUsage {
            packages: 128,
            ..exact
        },
        ScanUsage {
            bytes_read: exact.bytes_read + 1,
            ..exact
        },
        ScanUsage {
            bytes_read: usize::MAX,
            ..exact
        },
    ] {
        assert_eq!(
            registry.validate_import(&record, usage),
            Err(ImportCommitFailure::CapacityExceeded)
        );
    }
    assert!(memory.0.lock().unwrap().saves.is_empty());
}

// Catches missing final-publication headroom or rejecting the last available generation.
#[test]
fn validate_import_rejects_max_generation_and_accepts_max_minus_one() {
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![]);
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![]))
        .unwrap();
    let record = local("com.new", "New");
    registry.set_catalog_generation_for_test(u64::MAX - 1);
    assert_eq!(
        registry.validate_import(&record, ScanUsage::default()),
        Ok(())
    );
    registry.set_catalog_generation_for_test(u64::MAX);
    assert_eq!(
        registry.validate_import(&record, ScanUsage::default()),
        Err(ImportCommitFailure::CatalogGenerationExhausted)
    );
    assert!(memory.0.lock().unwrap().saves.is_empty());
}

#[test]
fn managed_import_registration_adopts_all_committed_outcomes_without_membership() {
    use crate::storage::managed_plugin_ownership::{
        ManagedOwnershipLoad, ManagedOwnershipPersistence,
    };
    use crate::storage::safe_plugin_document::{PersistFailure, PersistOutcome, PersistResult};
    struct OutcomeOwnership(PersistOutcome, bool);
    impl ManagedOwnershipPersistence for OutcomeOwnership {
        fn load(&self) -> AppResult<ManagedOwnershipLoad> {
            Ok(ManagedOwnershipLoad {
                index: ManagedOwnershipIndexV1::empty(),
                requires_rewrite: false,
            })
        }
        fn save(&self, _: &ManagedOwnershipIndexV1) -> PersistResult {
            if self.1 {
                Err(PersistFailure { outcome: self.0 })
            } else {
                Ok(self.0)
            }
        }
    }
    let fixture = OwnershipFixture::new();
    let (_, registered) = fixture.package();
    let entry = registered.entries()[0].clone();
    for outcome in [
        PersistOutcome::NotCommitted,
        PersistOutcome::CommittedProcessCrashSafe,
        PersistOutcome::CommittedDurable,
    ] {
        for error in [false, true] {
            let memory = MemoryPersistence::new(PluginStateFileV2::empty());
            let mut registry = PluginRegistry::initialize(
                vec![],
                Box::new(memory),
                Box::new(OutcomeOwnership(outcome, error)),
            );
            let before = registry.catalog_snapshot();
            let result = registry.register_managed_import(entry.clone());
            let committed = outcome != PersistOutcome::NotCommitted;
            assert_eq!(result.is_ok(), committed && !error);
            if let Err(failure) = result {
                assert_eq!(failure.persist_outcome, outcome);
            }
            assert_eq!(
                registry.publication.ownership.entries().unwrap(),
                if committed {
                    std::slice::from_ref(&entry)
                } else {
                    &[]
                }
            );
            assert_eq!(registry.catalog_snapshot(), before);
            assert!(registry.publication.locals.is_empty());
        }
    }
}

#[test]
fn managed_import_preflight_reserves_actual_receipt_and_state_without_writes() {
    let record = local("com.new", "New");
    let memory = MemoryPersistence::new(PluginStateFileV2::empty());
    let mut registry = memory.registry(vec![]);
    registry
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![]))
        .unwrap();
    let receipt = OwnershipReceiptV1::new(
        ReceiptId::parse("550e8400e29b41d4a716446655440000").unwrap(),
        PackageSlot::parse("pkg-00000000000000000000000000000000").unwrap(),
        &record,
    )
    .unwrap();
    let package_bytes =
        record.canonical_manifest_bytes().unwrap().len() + receipt.canonical_bytes().unwrap().len();
    let usage = ScanUsage {
        root_entries: 255,
        packages: 127,
        bytes_read: 2_097_152 - package_bytes,
    };
    assert_eq!(
        registry.validate_managed_import(&record, usage, "1"),
        Ok(())
    );
    assert_eq!(
        registry.validate_managed_import(
            &record,
            ScanUsage {
                bytes_read: usage.bytes_read + 1,
                ..usage
            },
            "1"
        ),
        Err(ImportCommitFailure::CapacityExceeded)
    );
    assert_eq!(
        registry.validate_managed_import(&record, usage, "01"),
        Err(ImportCommitFailure::CatalogStale)
    );
    let Runtime::Available { state, .. } = &mut registry.publication.runtime else {
        unreachable!()
    };
    state.revision = u64::MAX;
    assert_eq!(
        registry.validate_managed_import(&record, usage, "1"),
        Err(ImportCommitFailure::RevisionExhausted)
    );
    assert!(memory.0.lock().unwrap().saves.is_empty());
}
