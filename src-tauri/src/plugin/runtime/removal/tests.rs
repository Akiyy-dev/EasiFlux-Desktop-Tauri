use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::super::PluginRuntime;
use crate::error::AppResult;
use crate::plugin::discovery::*;
use crate::plugin::import::{test_support::VALID, SystemLocalManifestReader};
use crate::plugin::manifest::PluginManifestV1;
use crate::plugin::ownership::*;
use crate::plugin::record::PluginRecord;
use crate::plugin::PluginRegistry;
use crate::storage::local_plugin_package::*;
use crate::storage::managed_plugin_ownership::*;
use crate::storage::plugin_state::*;
use crate::storage::safe_plugin_document::*;

type Events = Arc<Mutex<Vec<&'static str>>>;

const REMOVAL_CRASH_CHILD: &str = "plugin::runtime::removal::tests::removal_crash_child";
const REMOVAL_CRASH_POINTS: [&str; 10] = [
    "disabled-saved",
    "removing-main-committed",
    "rename-returned",
    "quarantine-verified",
    "candidate-scanned",
    "manifest-removed",
    "receipt-removed",
    "directory-removed",
    "index-deleted",
    "published",
];

struct CrashDiskDiscovery(std::path::PathBuf);
impl LocalPluginDiscovery for CrashDiskDiscovery {
    fn discover(&self) -> LocalDiscoveryOutcome {
        discover_from_plugins_root(&self.0)
    }
}

fn removal_disk_runtime(root: &std::path::Path) -> Arc<PluginRuntime> {
    let packages = Arc::new(SystemLocalPluginPackageStorage::with_plugins_root(
        root.to_owned(),
    ));
    Arc::new(PluginRuntime::with_lifecycle_services(
        PluginRegistry::initialize(
            vec![],
            Box::new(PluginStateStore::with_path(root.join("state.json"))),
            Box::new(ManagedOwnershipStore::with_plugins_root(root.to_owned())),
        ),
        Arc::new(CrashDiskDiscovery(root.to_owned())),
        Arc::new(SystemLocalManifestReader),
        packages.clone(),
        packages,
    ))
}

fn run_removal_crash_child(root: &std::path::Path, checkpoint: &str) {
    use std::{
        process::Command,
        time::{Duration, Instant},
    };
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", REMOVAL_CRASH_CHILD, "--nocapture"])
        .env_clear()
        .env("EASIFLUX_PLUGIN_TEST_ROOT", root)
        .env("EASIFLUX_PLUGIN_TEST_CHECKPOINT", checkpoint)
        .env("EASIFLUX_PLUGIN_TEST_CHILD", "removal")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(73), "removal checkpoint {checkpoint}");
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("removal child timed out at {checkpoint}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

// Capture only disposable package evidence, not state/index convergence writes.
fn package_evidence(
    root: &std::path::Path,
) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(
        base: &std::path::Path,
        path: &std::path::Path,
        result: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
    ) {
        if !path.exists() {
            return;
        }
        result.insert(path.strip_prefix(base).unwrap().to_owned(), vec![]);
        for item in std::fs::read_dir(path).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                visit(base, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(base).unwrap().to_owned(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut result = std::collections::BTreeMap::new();
    visit(root, &root.join("local"), &mut result);
    visit(root, &root.join("removal-staging"), &mut result);
    result
}

#[tokio::test]
async fn removal_process_crash_checkpoints_converge_without_automatic_delete() {
    use crate::storage::local_plugin_import::LocalManifestImportStorage;
    // Only injected, canonical test roots; no native user profile is opened.
    let base = std::env::current_dir().unwrap().join("target");
    std::fs::create_dir_all(&base).unwrap();
    for checkpoint in REMOVAL_CRASH_POINTS {
        for damage in if checkpoint == "quarantine-verified" {
            vec!["none", "manifest-only", "mismatch"]
        } else {
            vec!["none"]
        } {
            let temp = tempfile::Builder::new()
                .prefix("plugin-runtime-removal-")
                .tempdir_in(&base)
                .unwrap();
            let disposable = temp.path().canonicalize().unwrap();
            let root = disposable.join("plugins");
            let packages = SystemLocalPluginPackageStorage::with_plugins_root(root.clone());
            let record = record();
            let promotion = packages
                .prepare_stage(&record, &record.canonical_manifest_bytes().unwrap())
                .unwrap()
                .promote()
                .unwrap();
            let source = root
                .join("local")
                .join(promotion.entry.package_slot().as_str());
            let index = ManagedOwnershipStore::with_plugins_root(root.clone());
            index
                .save(
                    &ManagedOwnershipIndexV1::empty()
                        .register(promotion.entry)
                        .unwrap(),
                )
                .unwrap();
            let child_root = disposable.clone();
            tokio::task::spawn_blocking(move || run_removal_crash_child(&child_root, checkpoint))
                .await
                .unwrap();
            let before_index = index.load().unwrap().index;
            let pre_rename = matches!(checkpoint, "disabled-saved" | "removing-main-committed");
            assert_eq!(source.exists(), pre_rename, "{checkpoint}");
            let observations = discover_from_plugins_root(&root).removals.observations;
            let expected_shape = match checkpoint {
                "rename-returned" | "quarantine-verified" | "candidate-scanned" => {
                    Some(RemovalObservationShape::Full)
                }
                "manifest-removed" => Some(RemovalObservationShape::ReceiptOnly),
                "receipt-removed" => Some(RemovalObservationShape::EmptyDirectory),
                _ => None,
            };
            assert_eq!(
                observations.first().map(|o| o.shape),
                expected_shape,
                "{checkpoint}"
            );
            assert_eq!(
                before_index.entries().len(),
                usize::from(!matches!(checkpoint, "index-deleted" | "published"))
            );
            if !pre_rename && !before_index.entries().is_empty() {
                assert!(before_index.entries()[0].removal_slot().is_some());
            }
            if damage != "none" {
                let quarantine = root
                    .join("removal-staging")
                    .join(observations[0].removal_slot.as_str());
                if damage == "manifest-only" {
                    std::fs::remove_file(quarantine.join("ownership-receipt.json")).unwrap();
                } else {
                    std::fs::write(quarantine.join("manifest.json"), b"{}").unwrap();
                }
            }
            let evidence = package_evidence(&root);
            let runtime = removal_disk_runtime(&root);
            for snapshot in [
                runtime.get_catalog().await.unwrap(),
                runtime.reload_catalog().await.unwrap(),
            ] {
                let snapshot = serde_json::to_value(snapshot).unwrap();
                assert_eq!(
                    package_evidence(&root),
                    evidence,
                    "restart/reload changed evidence: {checkpoint}/{damage}"
                );
                if pre_rename {
                    assert_eq!(snapshot["plugins"][0]["management"], "managed");
                    assert_eq!(snapshot["plugins"][0]["status"], "disabled");
                    assert!(index.load().unwrap().index.entries()[0]
                        .removal_slot()
                        .is_none());
                } else {
                    assert!(snapshot["plugins"].as_array().unwrap().is_empty());
                    if damage != "none" {
                        assert_eq!(snapshot["managedOwnership"]["conflictingEntryCount"], 1);
                    } else if expected_shape.is_some() {
                        assert_eq!(snapshot["managedOwnership"]["cleanupPendingCount"], 1);
                        assert_eq!(index.load().unwrap().index.entries().len(), 1);
                    } else {
                        assert!(index.load().unwrap().index.entries().is_empty());
                    }
                }
            }
            let state = PluginStateStore::with_path(root.join("state.json"))
                .load()
                .unwrap()
                .state;
            assert_eq!(state.entries.len(), 1);
            assert!(state.entries.iter().all(|entry| !entry.enabled));
        }
    }
}

#[tokio::test]
#[ignore = "launched only by removal_process_crash_checkpoints_converge_without_automatic_delete"]
async fn removal_crash_child() {
    let Some(root) = std::env::var_os("EASIFLUX_PLUGIN_TEST_ROOT").map(std::path::PathBuf::from)
    else {
        return;
    };
    assert_eq!(
        std::env::var("EASIFLUX_PLUGIN_TEST_CHILD").unwrap(),
        "removal"
    );
    assert_eq!(root.canonicalize().unwrap(), root);
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("plugin-runtime-removal-"));
    let checkpoint = std::env::var("EASIFLUX_PLUGIN_TEST_CHECKPOINT").unwrap();
    assert!(REMOVAL_CRASH_POINTS.contains(&checkpoint.as_str()));
    let runtime = removal_disk_runtime(&root.join("plugins"));
    let snapshot = runtime.get_catalog().await.unwrap();
    let result = runtime
        .remove_managed_local_plugin("com.example.notes", &snapshot.catalog_generation)
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(result).unwrap()["status"], "removed");
    panic!("removal checkpoint {checkpoint} was not reached");
}
type Saves = Arc<Mutex<VecDeque<(PersistOutcome, bool)>>>;
type Probe = Arc<
    Mutex<
        Option<(
            std::sync::Weak<PluginRuntime>,
            crate::plugin::manifest::PluginCatalogSnapshot,
        )>,
    >,
>;

fn probe_publication(probe: &Probe) {
    if let Some((runtime, baseline)) = &*probe.lock().unwrap() {
        let runtime = runtime.upgrade().unwrap();
        let registry = runtime
            .registry
            .try_read()
            .expect("registry guard must not cross I/O");
        assert_eq!(
            registry.catalog_snapshot(),
            *baseline,
            "intermediate publication leaked"
        );
    }
}

fn save_result(queue: &Saves) -> PersistResult {
    let (outcome, error) = queue
        .lock()
        .unwrap()
        .pop_front()
        .unwrap_or((PersistOutcome::CommittedDurable, false));
    if error {
        Err(PersistFailure { outcome })
    } else {
        Ok(outcome)
    }
}

#[derive(Clone)]
struct StateSpy {
    state: Arc<Mutex<PluginStateFileV2>>,
    events: Events,
    saves: Saves,
    probe: Probe,
}
impl PluginStatePersistence for StateSpy {
    fn load(&self) -> AppResult<PluginStateLoad> {
        self.events.lock().unwrap().push("load-state");
        Ok(PluginStateLoad {
            state: self.state.lock().unwrap().clone(),
            requires_rewrite: false,
        })
    }
    fn save(&self, next: &PluginStateFileV2) -> PersistResult {
        probe_publication(&self.probe);
        self.events.lock().unwrap().push("save-disabled-barrier");
        let result = save_result(&self.saves);
        let outcome = result
            .as_ref()
            .copied()
            .unwrap_or_else(|error| error.outcome);
        if persist_outcome_committed(outcome) {
            *self.state.lock().unwrap() = next.clone();
        }
        result
    }
}

#[derive(Clone)]
struct IndexSpy {
    index: Arc<Mutex<ManagedOwnershipIndexV1>>,
    events: Events,
    saves: Saves,
    probe: Probe,
}
impl ManagedOwnershipPersistence for IndexSpy {
    fn load(&self) -> AppResult<ManagedOwnershipLoad> {
        self.events.lock().unwrap().push("load-index");
        Ok(ManagedOwnershipLoad {
            index: self.index.lock().unwrap().clone(),
            requires_rewrite: false,
        })
    }
    fn save(&self, next: &ManagedOwnershipIndexV1) -> PersistResult {
        probe_publication(&self.probe);
        self.events
            .lock()
            .unwrap()
            .push(if next.entries().is_empty() {
                "delete-entry"
            } else if next.entries()[0].removal_slot().is_some() {
                "save-removing-barrier"
            } else {
                "rollback"
            });
        let result = save_result(&self.saves);
        let outcome = result
            .as_ref()
            .copied()
            .unwrap_or_else(|error| error.outcome);
        if persist_outcome_committed(outcome) {
            *self.index.lock().unwrap() = next.clone();
        }
        result
    }
}

#[derive(Clone)]
struct DiscoverySpy {
    outcome: Arc<Mutex<LocalDiscoveryOutcome>>,
    events: Events,
    probe: Probe,
}
impl LocalPluginDiscovery for DiscoverySpy {
    fn discover(&self) -> LocalDiscoveryOutcome {
        probe_publication(&self.probe);
        self.events.lock().unwrap().push("scan-candidate");
        self.outcome.lock().unwrap().clone()
    }
}

#[derive(Clone, Copy, Default)]
enum Rename {
    #[default]
    Commit,
    Ambiguous,
    AmbiguousUnchanged,
    Collision,
}
#[derive(Clone)]
struct PackageSpy {
    events: Events,
    discovery: DiscoverySpy,
    prepare_failure: Arc<Mutex<Option<RemovalStorageFailure>>>,
    final_failure: Arc<Mutex<bool>>,
    rename: Arc<Mutex<Rename>>,
    cleanup: Arc<Mutex<CleanupOutcome>>,
    scan_failure: Arc<Mutex<bool>>,
    scan_conflict: Arc<Mutex<bool>>,
    external: Arc<Mutex<bool>>,
}
impl ManagedLocalPluginRemovalStorage for PackageSpy {
    fn prepare(
        &self,
        locator: &LocalPackageLocator,
        entry: &ManagedOwnershipEntryV1,
    ) -> Result<Box<dyn OwnedRemoval>, RemovalStorageFailure> {
        probe_publication(&self.discovery.probe);
        self.events.lock().unwrap().push("open-source");
        assert!(entry.matches_locator(locator));
        if let Some(error) = *self.prepare_failure.lock().unwrap() {
            return Err(error);
        }
        self.events.lock().unwrap().push("reserve-one-slot");
        Ok(Box::new(OwnedSpy {
            storage: self.clone(),
            entry: entry.clone(),
            slot: slot(),
        }))
    }
}
struct OwnedSpy {
    storage: PackageSpy,
    entry: ManagedOwnershipEntryV1,
    slot: RemovalSlot,
}
impl OwnedRemoval for OwnedSpy {
    fn removal_slot(&self) -> &RemovalSlot {
        &self.slot
    }
    fn reverify_before_rename(&self) -> Result<(), RemovalStorageFailure> {
        probe_publication(&self.storage.discovery.probe);
        self.storage.events.lock().unwrap().push("final-reverify");
        if *self.storage.final_failure.lock().unwrap() {
            Err(RemovalStorageFailure::IdentityChanged)
        } else {
            Ok(())
        }
    }
    fn quarantine_once(&mut self) -> QuarantineRenameOutcome {
        probe_publication(&self.storage.discovery.probe);
        self.storage.events.lock().unwrap().push("rename-once");
        if matches!(*self.storage.rename.lock().unwrap(), Rename::Collision) {
            return QuarantineRenameOutcome::ProvenNotCommitted(
                RemovalStorageFailure::ProvenWriteFailure,
            );
        }
        if matches!(
            *self.storage.rename.lock().unwrap(),
            Rename::AmbiguousUnchanged
        ) {
            return QuarantineRenameOutcome::CommitUnconfirmed;
        }
        let mut physical = self.storage.discovery.outcome.lock().unwrap();
        let source = physical.plugins.remove(0);
        physical.occupied_slots.clear();
        physical.removals.observations.push(RemovalObservation {
            removal_slot: self.slot.clone(),
            directory_identity: source.locator.directory_identity,
            manifest: Some((source.record.clone(), source.locator.manifest_identity)),
            receipt: source.locator.receipt.clone(),
            shape: RemovalObservationShape::Full,
        });
        physical.removals.occupied_slots.push(self.slot.clone());
        if *self.storage.scan_conflict.lock().unwrap() {
            physical.removals.observations[0].directory_identity.object += 100;
        }
        if *self.storage.external.lock().unwrap() {
            let mut external = source;
            external.locator.receipt = None;
            external.locator.package_slot =
                PackageSlot::parse("pkg-00000000000000000000000000000055").unwrap();
            physical.plugins.push(external);
        }
        if *self.storage.scan_failure.lock().unwrap() {
            *physical = LocalDiscoveryOutcome::unavailable();
        }
        if matches!(*self.storage.rename.lock().unwrap(), Rename::Ambiguous) {
            QuarantineRenameOutcome::CommitUnconfirmed
        } else {
            QuarantineRenameOutcome::Committed(self.evidence())
        }
    }
    fn verify_quarantine(&self) -> Result<VerifiedQuarantine, RemovalStorageFailure> {
        probe_publication(&self.storage.discovery.probe);
        self.storage
            .events
            .lock()
            .unwrap()
            .push("verify-quarantine");
        if matches!(
            *self.storage.rename.lock().unwrap(),
            Rename::AmbiguousUnchanged
        ) {
            return Err(RemovalStorageFailure::IdentityChanged);
        }
        Ok(self.evidence())
    }
    fn cleanup_once(&mut self) -> CleanupOutcome {
        probe_publication(&self.storage.discovery.probe);
        self.storage.events.lock().unwrap().push("cleanup-once");
        *self.storage.cleanup.lock().unwrap()
    }
}
impl OwnedSpy {
    fn evidence(&self) -> VerifiedQuarantine {
        VerifiedQuarantine {
            entry: Box::new(self.entry.begin_removal(self.slot.clone())),
            removal_slot: self.slot.clone(),
        }
    }
}

fn slot() -> RemovalSlot {
    RemovalSlot::parse("remove-550e8400e29b41d4a716446655440099").unwrap()
}
fn record() -> PluginRecord {
    PluginRecord::local_declarative(serde_json::from_slice::<PluginManifestV1>(VALID).unwrap())
        .unwrap()
}
struct Fixture {
    runtime: Arc<PluginRuntime>,
    state: StateSpy,
    index: IndexSpy,
    package: PackageSpy,
    generation: String,
}
impl Fixture {
    async fn new() -> Self {
        Self::with_revisions(0, 1).await
    }
    async fn with_revisions(state_revision: u64, index_revision: u64) -> Self {
        let events = Arc::new(Mutex::new(vec![]));
        let probe: Probe = Default::default();
        let record = record();
        let receipt = OwnershipReceiptV1::new(
            ReceiptId::parse("550e8400e29b41d4a716446655440000").unwrap(),
            PackageSlot::parse("pkg-550e8400e29b41d4a716446655440001").unwrap(),
            &record,
        )
        .unwrap();
        let verified = VerifiedPackageReceipt {
            canonical_sha256: receipt.canonical_sha256(),
            model: receipt,
            file_identity: FileIdentity {
                volume: 1,
                object: 3,
            },
        };
        let locator = LocalPackageLocator {
            package_slot: verified.model.package_slot().clone(),
            directory_identity: FileIdentity {
                volume: 1,
                object: 1,
            },
            manifest_identity: FileIdentity {
                volume: 1,
                object: 2,
            },
            receipt: Some(verified.clone()),
        };
        let entry = ManagedOwnershipEntryV1::managed(
            verified,
            &record,
            locator.directory_identity,
            locator.manifest_identity,
        )
        .unwrap();
        let mut wire: Value = serde_json::from_slice(
            &ManagedOwnershipIndexV1::empty()
                .register(entry)
                .unwrap()
                .canonical_bytes()
                .unwrap(),
        )
        .unwrap();
        wire["revision"] = index_revision.to_string().into();
        let index = IndexSpy {
            index: Arc::new(Mutex::new(
                ManagedOwnershipIndexV1::parse(&serde_json::to_vec(&wire).unwrap()).unwrap(),
            )),
            events: events.clone(),
            saves: Default::default(),
            probe: probe.clone(),
        };
        let state = StateSpy {
            state: Arc::new(Mutex::new(PluginStateFileV2 {
                schema_version: 2,
                revision: state_revision,
                entries: vec![],
            })),
            events: events.clone(),
            saves: Default::default(),
            probe: probe.clone(),
        };
        let mut initial = LocalDiscoveryOutcome::available(vec![]);
        initial.plugins.push(DiscoveredLocalPlugin {
            record,
            locator: locator.clone(),
        });
        initial.occupied_slots.push(locator.package_slot);
        let discovery = DiscoverySpy {
            outcome: Arc::new(Mutex::new(initial)),
            events: events.clone(),
            probe,
        };
        let package = PackageSpy {
            events: events.clone(),
            discovery: discovery.clone(),
            prepare_failure: Default::default(),
            final_failure: Default::default(),
            rename: Default::default(),
            cleanup: Arc::new(Mutex::new(CleanupOutcome::Removed)),
            scan_failure: Default::default(),
            scan_conflict: Default::default(),
            external: Default::default(),
        };
        let registry =
            PluginRegistry::initialize(vec![], Box::new(state.clone()), Box::new(index.clone()));
        let runtime = Arc::new(PluginRuntime::with_lifecycle_services(
            registry,
            Arc::new(discovery),
            Arc::new(SystemLocalManifestReader),
            Arc::new(SystemLocalPluginPackageStorage::new()),
            Arc::new(package.clone()),
        ));
        let snapshot = runtime.get_catalog().await.unwrap();
        let generation = snapshot.catalog_generation;
        events.lock().unwrap().clear();
        Self {
            runtime,
            state,
            index,
            package,
            generation,
        }
    }
    async fn remove(&self) -> Value {
        serde_json::to_value(
            self.runtime
                .remove_managed_local_plugin("com.example.notes", &self.generation)
                .await
                .unwrap(),
        )
        .unwrap()
    }
    fn events(&self) -> Vec<&'static str> {
        self.package.events.lock().unwrap().clone()
    }
    fn reject(result: &Value, code: &str, saved: bool) {
        assert_eq!(result["status"], "notRemoved", "{result}");
        assert_eq!(result["reasonCode"], code);
        assert_eq!(result["disabledDecisionSaved"], saved);
        assert!(result.get("pluginId").is_none());
    }
}

#[tokio::test]
async fn remove_has_one_commit_point_one_cleanup_attempt_and_one_publication() {
    let fixture = Fixture::new().await;
    let baseline = fixture.runtime.registry.read().await.catalog_snapshot();
    *fixture.package.discovery.probe.lock().unwrap() =
        Some((Arc::downgrade(&fixture.runtime), baseline));
    let result = fixture.remove().await;
    assert_eq!(result["status"], "removed", "{result}");
    assert_eq!(result["snapshot"]["catalogGeneration"], "2");
    assert_eq!(result["snapshot"]["revision"], "1");
    assert_eq!(result["snapshot"]["plugins"], serde_json::json!([]));
    assert_eq!(
        fixture.events(),
        [
            "open-source",
            "reserve-one-slot",
            "save-disabled-barrier",
            "save-removing-barrier",
            "final-reverify",
            "rename-once",
            "verify-quarantine",
            "scan-candidate",
            "cleanup-once",
            "delete-entry"
        ]
    );
    assert!(fixture.index.index.lock().unwrap().entries().is_empty());
}

#[tokio::test]
async fn same_generation_higher_revision_reenable_is_rejected_before_removal_storage() {
    let fixture = Fixture::new().await;
    let toggled = fixture
        .runtime
        .set_enabled("com.example.notes", true, &fixture.generation)
        .await
        .unwrap();
    assert_eq!(toggled.catalog_generation, fixture.generation);
    fixture.package.events.lock().unwrap().clear();
    Fixture::reject(
        &fixture.remove().await,
        "plugin_remove_requires_disabled",
        false,
    );
    assert!(fixture.events().is_empty());
}

#[tokio::test]
async fn preflight_revision_and_generation_failures_never_access_storage() {
    for (state, index, code) in [
        (u64::MAX, 1, "plugin_revision_exhausted"),
        (0, u64::MAX - 1, "plugin_ownership_revision_exhausted"),
        (0, u64::MAX, "plugin_ownership_revision_exhausted"),
    ] {
        let fixture = Fixture::with_revisions(state, index).await;
        Fixture::reject(&fixture.remove().await, code, false);
        assert!(fixture.events().is_empty());
    }
    for generation in ["01", "0", "18446744073709551616"] {
        let mut fixture = Fixture::new().await;
        fixture.generation = generation.into();
        Fixture::reject(&fixture.remove().await, "plugin_catalog_stale", false);
        assert!(fixture.events().is_empty());
    }
    let mut fixture = Fixture::new().await;
    fixture
        .runtime
        .registry
        .write()
        .await
        .set_catalog_generation_for_test(u64::MAX);
    fixture.generation = u64::MAX.to_string();
    Fixture::reject(
        &fixture.remove().await,
        "plugin_catalog_generation_exhausted",
        false,
    );
    assert!(fixture.events().is_empty());
}

#[tokio::test]
async fn state_barrier_failure_never_saves_removing_or_renames() {
    let fixture = Fixture::new().await;
    fixture
        .state
        .saves
        .lock()
        .unwrap()
        .push_back((PersistOutcome::NotCommitted, true));
    Fixture::reject(
        &fixture.remove().await,
        "plugin_state_persist_failed",
        false,
    );
    assert_eq!(
        fixture.events(),
        ["open-source", "reserve-one-slot", "save-disabled-barrier"]
    );
}

#[tokio::test]
async fn initial_identity_change_is_false_phase() {
    let fixture = Fixture::new().await;
    *fixture.package.prepare_failure.lock().unwrap() = Some(RemovalStorageFailure::IdentityChanged);
    Fixture::reject(
        &fixture.remove().await,
        "plugin_remove_identity_changed",
        false,
    );
    assert_eq!(fixture.events(), ["open-source"]);
}

#[tokio::test]
async fn final_identity_change_is_true_phase_and_rolls_back() {
    let fixture = Fixture::new().await;
    *fixture.package.final_failure.lock().unwrap() = true;
    Fixture::reject(
        &fixture.remove().await,
        "plugin_remove_identity_changed",
        true,
    );
    assert_eq!(fixture.events().last(), Some(&"rollback"));
    assert!(!fixture.events().contains(&"rename-once"));
    assert!(fixture.index.index.lock().unwrap().entries()[0]
        .removal_slot()
        .is_none());
}

#[tokio::test]
async fn one_slot_collision_rolls_back_and_ends_without_retry() {
    let fixture = Fixture::new().await;
    *fixture.package.rename.lock().unwrap() = Rename::Collision;
    Fixture::reject(&fixture.remove().await, "plugin_remove_write_failed", true);
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|&&e| e == "rename-once")
            .count(),
        1
    );
    assert_eq!(fixture.events().last(), Some(&"rollback"));
}

#[tokio::test]
async fn ambiguous_rename_never_returns_not_removed() {
    let fixture = Fixture::new().await;
    *fixture.package.rename.lock().unwrap() = Rename::Ambiguous;
    let result = fixture.remove().await;
    assert_ne!(result["status"], "notRemoved");
    assert!(!fixture.events().contains(&"rollback"));
}

#[tokio::test]
async fn postrename_scan_failure_still_attempts_cleanup_once_then_returns_unconfirmed() {
    let fixture = Fixture::new().await;
    *fixture.package.scan_failure.lock().unwrap() = true;
    let before = fixture.runtime.registry.read().await.catalog_snapshot();
    let result = fixture.remove().await;
    assert_eq!(result["status"], "removedCatalogUnconfirmed");
    assert_eq!(result["snapshot"], serde_json::to_value(before).unwrap());
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|&&e| e == "cleanup-once")
            .count(),
        1
    );
}

#[tokio::test]
async fn cleanup_failure_keeps_removing_and_returns_cleanup_pending() {
    let fixture = Fixture::new().await;
    *fixture.package.cleanup.lock().unwrap() =
        CleanupOutcome::Pending(KnownCleanupShape::ReceiptOnly);
    let result = fixture.remove().await;
    assert_eq!(result["status"], "removedCleanupPending", "{result}");
    assert!(!fixture.events().contains(&"delete-entry"));
    assert!(fixture.index.index.lock().unwrap().entries()[0]
        .removal_slot()
        .is_some());
}

#[tokio::test]
async fn cleanup_success_then_index_failure_returns_cleanup_pending() {
    let fixture = Fixture::new().await;
    fixture.index.saves.lock().unwrap().extend([
        (PersistOutcome::CommittedDurable, false),
        (PersistOutcome::NotCommitted, true),
    ]);
    let result = fixture.remove().await;
    assert_eq!(result["status"], "removedCleanupPending", "{result}");
}

#[tokio::test]
async fn committed_index_delete_error_returns_unconfirmed_not_zero_count_pending() {
    let fixture = Fixture::new().await;
    fixture.index.saves.lock().unwrap().extend([
        (PersistOutcome::CommittedDurable, false),
        (PersistOutcome::CommittedDurable, true),
    ]);
    let result = fixture.remove().await;
    assert_eq!(result["status"], "removedCatalogUnconfirmed");
    assert!(fixture.index.index.lock().unwrap().entries().is_empty());
}

#[tokio::test]
async fn both_absent_permits_exactly_one_index_delete() {
    let fixture = Fixture::new().await;
    *fixture.package.cleanup.lock().unwrap() =
        CleanupOutcome::Pending(KnownCleanupShape::BothAbsent);
    assert_eq!(fixture.remove().await["status"], "removed");
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|&&e| e == "delete-entry")
            .count(),
        1
    );
}

#[tokio::test]
async fn same_id_external_arriving_after_commit_is_not_the_removed_identity() {
    let fixture = Fixture::new().await;
    *fixture.package.external.lock().unwrap() = true;
    let result = fixture.remove().await;
    assert_eq!(result["status"], "removed", "{result}");
    assert_eq!(result["snapshot"]["plugins"][0]["management"], "external");
}

#[tokio::test]
async fn maximum_minus_one_generation_succeeds_with_one_increment() {
    let mut fixture = Fixture::new().await;
    fixture
        .runtime
        .registry
        .write()
        .await
        .set_catalog_generation_for_test(u64::MAX - 1);
    fixture.generation = (u64::MAX - 1).to_string();
    let result = fixture.remove().await;
    assert_eq!(result["status"], "removed");
    assert_eq!(
        result["snapshot"]["catalogGeneration"],
        u64::MAX.to_string()
    );
}

#[tokio::test]
async fn startup_and_reload_only_observe_quarantine() {
    for shape in [
        RemovalObservationShape::Full,
        RemovalObservationShape::ReceiptOnly,
        RemovalObservationShape::EmptyDirectory,
        RemovalObservationShape::ManifestOnly,
        RemovalObservationShape::Unknown,
    ] {
        let fixture = Fixture::new().await;
        let entry = fixture.index.index.lock().unwrap().entries()[0].clone();
        *fixture.index.index.lock().unwrap() = ManagedOwnershipIndexV1::empty()
            .register(entry.begin_removal(slot()))
            .unwrap();
        let mut physical = fixture.package.discovery.outcome.lock().unwrap();
        let source = physical.plugins.remove(0);
        physical.occupied_slots.clear();
        physical.removals.occupied_slots.push(slot());
        physical.removals.observations.push(RemovalObservation {
            removal_slot: slot(),
            directory_identity: source.locator.directory_identity,
            manifest: matches!(
                shape,
                RemovalObservationShape::Full | RemovalObservationShape::ManifestOnly
            )
            .then_some((source.record, source.locator.manifest_identity)),
            receipt: matches!(
                shape,
                RemovalObservationShape::Full | RemovalObservationShape::ReceiptOnly
            )
            .then_some(source.locator.receipt.unwrap()),
            shape,
        });
        drop(physical);
        // A new process observes the same crash shape, and reload only repeats
        // observation; neither path is permitted to invoke package storage.
        let registry = PluginRegistry::initialize(
            vec![],
            Box::new(fixture.state.clone()),
            Box::new(fixture.index.clone()),
        );
        let restarted = Arc::new(PluginRuntime::with_lifecycle_services(
            registry,
            Arc::new(fixture.package.discovery.clone()),
            Arc::new(SystemLocalManifestReader),
            Arc::new(SystemLocalPluginPackageStorage::new()),
            Arc::new(fixture.package.clone()),
        ));
        fixture.package.events.lock().unwrap().clear();
        restarted.get_catalog().await.unwrap();
        restarted.reload_catalog().await.unwrap();
        assert_eq!(
            fixture.events(),
            [
                "scan-candidate",
                "load-index",
                "scan-candidate",
                "load-index"
            ]
        );
    }
}

#[tokio::test]
async fn startup_and_reload_only_write_safe_absence_or_rollback_transitions() {
    for shape in ["absent", "source", "orphan", "mismatch"] {
        let fixture = Fixture::new().await;
        let entry = fixture.index.index.lock().unwrap().entries()[0].clone();
        *fixture.index.index.lock().unwrap() = ManagedOwnershipIndexV1::empty()
            .register(entry.begin_removal(slot()))
            .unwrap();
        let mut physical = fixture.package.discovery.outcome.lock().unwrap();
        if shape != "source" {
            physical.plugins.clear();
            physical.occupied_slots.clear();
        }
        if matches!(shape, "orphan" | "mismatch") {
            let target_slot = if shape == "orphan" {
                RemovalSlot::parse("remove-00000000000000000000000000000077").unwrap()
            } else {
                slot()
            };
            physical.removals.occupied_slots.push(target_slot.clone());
            physical.removals.observations.push(RemovalObservation {
                removal_slot: target_slot,
                directory_identity: FileIdentity {
                    volume: 1,
                    object: 99,
                },
                manifest: None,
                receipt: None,
                shape: RemovalObservationShape::EmptyDirectory,
            });
        }
        drop(physical);
        let registry = PluginRegistry::initialize(
            vec![],
            Box::new(fixture.state.clone()),
            Box::new(fixture.index.clone()),
        );
        let restarted = Arc::new(PluginRuntime::with_lifecycle_services(
            registry,
            Arc::new(fixture.package.discovery.clone()),
            Arc::new(SystemLocalManifestReader),
            Arc::new(SystemLocalPluginPackageStorage::new()),
            Arc::new(fixture.package.clone()),
        ));
        fixture.package.events.lock().unwrap().clear();
        restarted.get_catalog().await.unwrap();
        restarted.reload_catalog().await.unwrap();
        assert!(!fixture.events().iter().any(|event| matches!(
            *event,
            "open-source"
                | "reserve-one-slot"
                | "rename-once"
                | "cleanup-once"
                | "save-disabled-barrier"
                | "save-removing-barrier"
        )));
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|&&e| e == "delete-entry")
                .count(),
            usize::from(matches!(shape, "absent" | "orphan"))
        );
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|&&e| e == "rollback")
                .count(),
            usize::from(shape == "source")
        );
    }
}

#[tokio::test]
async fn every_disabled_barrier_outcome_is_adopted_without_authorizing_on_error() {
    for outcome in [
        PersistOutcome::NotCommitted,
        PersistOutcome::CommittedProcessCrashSafe,
        PersistOutcome::CommittedDurable,
    ] {
        for error in [false, true] {
            let fixture = Fixture::new().await;
            fixture
                .state
                .saves
                .lock()
                .unwrap()
                .push_back((outcome, error));
            let result = fixture.remove().await;
            let continue_allowed = !error && outcome_satisfies_destructive_barrier(outcome);
            assert_eq!(fixture.events().contains(&"rename-once"), continue_allowed);
            assert_eq!(
                fixture.state.state.lock().unwrap().revision,
                u64::from(persist_outcome_committed(outcome))
            );
            if !continue_allowed {
                Fixture::reject(&result, "plugin_state_persist_failed", false);
            }
        }
    }
}

#[tokio::test]
async fn ownership_barrier_failure_never_renames_and_adopts_committed_document() {
    for outcome in [
        PersistOutcome::NotCommitted,
        PersistOutcome::CommittedProcessCrashSafe,
        PersistOutcome::CommittedDurable,
    ] {
        for error in [false, true] {
            let fixture = Fixture::new().await;
            fixture
                .index
                .saves
                .lock()
                .unwrap()
                .extend([(outcome, error), (PersistOutcome::NotCommitted, true)]);
            let result = fixture.remove().await;
            let continue_allowed = !error && outcome_satisfies_destructive_barrier(outcome);
            assert_eq!(fixture.events().contains(&"rename-once"), continue_allowed);
            if !continue_allowed {
                Fixture::reject(&result, "plugin_ownership_persist_failed", true);
                assert_eq!(
                    fixture.index.index.lock().unwrap().entries()[0]
                        .removal_slot()
                        .is_some(),
                    persist_outcome_committed(outcome)
                );
            }
        }
    }
}

#[tokio::test]
async fn rollback_failure_publishes_removal_pending_once() {
    for outcome in [
        PersistOutcome::NotCommitted,
        PersistOutcome::CommittedProcessCrashSafe,
        PersistOutcome::CommittedDurable,
    ] {
        for error in [false, true] {
            let fixture = Fixture::new().await;
            *fixture.package.final_failure.lock().unwrap() = true;
            fixture
                .index
                .saves
                .lock()
                .unwrap()
                .extend([(PersistOutcome::CommittedDurable, false), (outcome, error)]);
            let result = fixture.remove().await;
            let committed = persist_outcome_committed(outcome);
            Fixture::reject(
                &result,
                if committed {
                    "plugin_remove_identity_changed"
                } else {
                    "plugin_ownership_persist_failed"
                },
                true,
            );
            assert_eq!(
                result["snapshot"]["plugins"][0]["management"],
                if committed {
                    "managed"
                } else {
                    "removalPending"
                }
            );
            assert_eq!(
                result["snapshot"]["catalogGeneration"],
                if committed { "1" } else { "2" }
            );
            assert_eq!(
                fixture
                    .events()
                    .iter()
                    .filter(|&&e| e == "rollback")
                    .count(),
                1
            );
            assert!(!fixture
                .events()
                .iter()
                .any(|e| e.starts_with("load-") || *e == "scan-candidate"));
        }
    }
}

#[tokio::test]
async fn every_index_deletion_outcome_has_a_valid_result_and_adoption() {
    for outcome in [
        PersistOutcome::NotCommitted,
        PersistOutcome::CommittedProcessCrashSafe,
        PersistOutcome::CommittedDurable,
    ] {
        for error in [false, true] {
            let fixture = Fixture::new().await;
            fixture
                .index
                .saves
                .lock()
                .unwrap()
                .extend([(PersistOutcome::CommittedDurable, false), (outcome, error)]);
            let result = fixture.remove().await;
            let committed = persist_outcome_committed(outcome);
            assert_eq!(
                fixture.index.index.lock().unwrap().entries().is_empty(),
                committed
            );
            assert_eq!(
                result["status"],
                if !committed {
                    "removedCleanupPending"
                } else if error {
                    "removedCatalogUnconfirmed"
                } else {
                    "removed"
                }
            );
            assert_eq!(
                result["snapshot"]["managedOwnership"]["cleanupPendingCount"],
                u32::from(!committed)
            );
        }
    }
}

#[tokio::test]
async fn mandatory_disabled_rewrite_prunes_old_enabled_identity_even_on_canonical_noop() {
    let fixture = Fixture::new().await;
    fixture
        .runtime
        .set_enabled("com.example.notes", false, &fixture.generation)
        .await
        .unwrap();
    fixture.package.events.lock().unwrap().clear();
    let before = fixture.state.state.lock().unwrap().revision;
    assert_eq!(fixture.remove().await["status"], "removed");
    assert_eq!(fixture.state.state.lock().unwrap().revision, before + 1);
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|&&e| e == "save-disabled-barrier")
            .count(),
        1
    );
}

#[tokio::test]
async fn caller_cancellation_does_not_cancel_owned_remove() {
    let fixture = Fixture::new().await;
    let guard = fixture.runtime.operation_gate.clone().lock_owned().await;
    let mut request = Box::pin(
        fixture
            .runtime
            .remove_managed_local_plugin("com.example.notes", &fixture.generation),
    );
    assert!(futures_util::poll!(&mut request).is_pending());
    drop(request);
    let finished = fixture.runtime.reserve_operation().enter();
    drop(guard);
    let _after = tokio::time::timeout(std::time::Duration::from_secs(10), finished)
        .await
        .unwrap();
    assert!(fixture.index.index.lock().unwrap().entries().is_empty());
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|&&e| e == "rename-once")
            .count(),
        1
    );
}

#[tokio::test]
async fn remove_import_toggle_reload_are_fifo() {
    use crate::plugin::import::PreparedManifest;
    let fixture = Fixture::new().await;
    let now = std::time::Instant::now();
    let preview = fixture
        .runtime
        .import_sessions
        .reserve_prepare(now)
        .unwrap()
        .publish(PreparedManifest::parse(VALID).unwrap(), 1, now)
        .unwrap();
    let guard = fixture.runtime.operation_gate.clone().lock_owned().await;
    let mut remove = Box::pin(
        fixture
            .runtime
            .remove_managed_local_plugin("com.example.notes", "1"),
    );
    let mut import = Box::pin(fixture.runtime.commit_import(&preview.token, "1"));
    let mut toggle = Box::pin(fixture.runtime.set_enabled("com.example.notes", true, "1"));
    let mut reload = Box::pin(fixture.runtime.reload_catalog());
    assert!(futures_util::poll!(&mut remove).is_pending());
    assert!(futures_util::poll!(&mut import).is_pending());
    assert!(futures_util::poll!(&mut toggle).is_pending());
    assert!(futures_util::poll!(&mut reload).is_pending());
    assert!(fixture.events().is_empty());
    drop(guard);
    assert_eq!(
        serde_json::to_value(remove.await.unwrap()).unwrap()["status"],
        "removed"
    );
    assert_eq!(
        serde_json::to_value(import.await.unwrap()).unwrap()["reasonCode"],
        "plugin_catalog_stale"
    );
    assert!(matches!(
        toggle.await,
        Err(crate::error::AppError::Plugin {
            code: "plugin_catalog_stale",
            ..
        })
    ));
    reload.await.unwrap();
    let events = fixture.events();
    assert_eq!(events.iter().position(|&e| e == "cleanup-once"), Some(8));
    assert_eq!(events.iter().position(|&e| e == "load-index"), Some(11));
}

#[tokio::test]
async fn ambiguous_cleanup_preserves_frozen_complete_baseline() {
    let fixture = Fixture::new().await;
    *fixture.package.cleanup.lock().unwrap() = CleanupOutcome::Conflict;
    let baseline = fixture.runtime.registry.read().await.catalog_snapshot();
    let result = fixture.remove().await;
    assert_eq!(result["status"], "removedCatalogUnconfirmed");
    assert_eq!(result["snapshot"], serde_json::to_value(baseline).unwrap());
}

#[tokio::test]
async fn authoritative_scan_proven_conflict_can_publish_once() {
    let fixture = Fixture::new().await;
    *fixture.package.cleanup.lock().unwrap() = CleanupOutcome::Conflict;
    *fixture.package.scan_conflict.lock().unwrap() = true;
    let result = fixture.remove().await;
    assert_eq!(result["status"], "removedCatalogUnconfirmed");
    assert_eq!(result["snapshot"]["catalogGeneration"], "2");
    assert_eq!(
        result["snapshot"]["managedOwnership"]["conflictingEntryCount"],
        1
    );
}

#[tokio::test]
async fn ambiguous_rename_with_unchanged_source_preserves_baseline() {
    let fixture = Fixture::new().await;
    *fixture.package.rename.lock().unwrap() = Rename::AmbiguousUnchanged;
    *fixture.package.cleanup.lock().unwrap() = CleanupOutcome::Conflict;
    let baseline = fixture.runtime.registry.read().await.catalog_snapshot();
    let result = fixture.remove().await;
    assert_eq!(result["status"], "removedCatalogUnconfirmed");
    assert_eq!(result["snapshot"], serde_json::to_value(baseline).unwrap());
    assert!(!fixture.events().contains(&"rollback"));
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|&&e| e == "cleanup-once")
            .count(),
        1
    );
}

#[tokio::test]
async fn ambiguous_removal_rejects_same_generation_queued_enable_until_reload() {
    let fixture = Fixture::new().await;
    *fixture.package.rename.lock().unwrap() = Rename::AmbiguousUnchanged;
    *fixture.package.cleanup.lock().unwrap() = CleanupOutcome::Conflict;
    let baseline = fixture.runtime.registry.read().await.catalog_snapshot();
    let guard = fixture.runtime.operation_gate.clone().lock_owned().await;
    let mut remove = Box::pin(
        fixture
            .runtime
            .remove_managed_local_plugin("com.example.notes", "1"),
    );
    let mut toggle = Box::pin(fixture.runtime.set_enabled("com.example.notes", true, "1"));
    assert!(futures_util::poll!(&mut remove).is_pending());
    assert!(futures_util::poll!(&mut toggle).is_pending());
    drop(guard);
    let removed = serde_json::to_value(remove.await.unwrap()).unwrap();
    assert_eq!(removed["status"], "removedCatalogUnconfirmed");
    assert_eq!(
        removed["snapshot"],
        serde_json::to_value(&baseline).unwrap()
    );
    let toggled = toggle.await;
    assert!(
        matches!(
            toggled,
            Err(crate::error::AppError::Plugin {
                code: "plugin_catalog_stale",
                ..
            })
        ),
        "an adopted removing entry must not be enabled from the frozen catalog: {toggled:?}"
    );
    assert_eq!(
        fixture.runtime.registry.read().await.catalog_snapshot(),
        baseline
    );
    assert!(fixture.index.index.lock().unwrap().entries()[0]
        .removal_slot()
        .is_some());
    assert!(fixture
        .state
        .state
        .lock()
        .unwrap()
        .entries
        .iter()
        .all(|entry| !entry.enabled));
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|&&event| event == "save-disabled-barrier")
            .count(),
        1
    );

    fixture.package.events.lock().unwrap().clear();
    Fixture::reject(&fixture.remove().await, "plugin_catalog_stale", false);
    assert!(fixture.events().is_empty());
    let reloaded = fixture.runtime.reload_catalog().await.unwrap();
    assert!(fixture.index.index.lock().unwrap().entries()[0]
        .removal_slot()
        .is_none());
    fixture
        .runtime
        .set_enabled("com.example.notes", true, &reloaded.catalog_generation)
        .await
        .unwrap();
}

#[tokio::test]
async fn deleted_owned_identity_cannot_be_enabled_from_unconfirmed_frozen_catalog() {
    let fixture = Fixture::new().await;
    *fixture.package.scan_failure.lock().unwrap() = true;
    let baseline = fixture.runtime.registry.read().await.catalog_snapshot();
    let guard = fixture.runtime.operation_gate.clone().lock_owned().await;
    let mut remove = Box::pin(
        fixture
            .runtime
            .remove_managed_local_plugin("com.example.notes", "1"),
    );
    let mut toggle = Box::pin(fixture.runtime.set_enabled("com.example.notes", true, "1"));
    assert!(futures_util::poll!(&mut remove).is_pending());
    assert!(futures_util::poll!(&mut toggle).is_pending());
    drop(guard);
    assert_eq!(
        serde_json::to_value(remove.await.unwrap()).unwrap()["status"],
        "removedCatalogUnconfirmed"
    );
    assert!(fixture.index.index.lock().unwrap().entries().is_empty());
    let toggled = toggle.await;
    assert!(
        matches!(
            toggled,
            Err(crate::error::AppError::Plugin {
                code: "plugin_catalog_stale",
                ..
            })
        ),
        "a deleted ownership entry must not be enabled from the frozen catalog: {toggled:?}"
    );
    assert_eq!(
        fixture.runtime.registry.read().await.catalog_snapshot(),
        baseline
    );
    assert_eq!(fixture.state.state.lock().unwrap().revision, 1);

    let mut replacement = record().manifest().clone();
    replacement.id = crate::plugin::manifest::PluginId::parse("com.example.replacement").unwrap();
    let replacement = PluginRecord::local_declarative(replacement).unwrap();
    let unavailable = fixture.runtime.reload_catalog().await.unwrap();
    assert_eq!(
        unavailable.local_discovery.status,
        crate::plugin::manifest::LocalDiscoveryStatus::Unavailable
    );
    assert_eq!(
        fixture
            .runtime
            .registry
            .read()
            .await
            .validate_import(&replacement, ScanUsage::default()),
        Err(crate::plugin::import::ImportCommitFailure::CatalogStale)
    );
    *fixture.package.discovery.outcome.lock().unwrap() = LocalDiscoveryOutcome::available(vec![]);
    fixture.runtime.reload_catalog().await.unwrap();
    assert_eq!(
        fixture
            .runtime
            .registry
            .read()
            .await
            .validate_import(&replacement, ScanUsage::default()),
        Ok(())
    );
}

#[tokio::test]
async fn confirmed_rollback_does_not_leave_mutation_authority_blocked() {
    let fixture = Fixture::new().await;
    *fixture.package.final_failure.lock().unwrap() = true;
    Fixture::reject(
        &fixture.remove().await,
        "plugin_remove_identity_changed",
        true,
    );
    fixture
        .runtime
        .set_enabled("com.example.notes", true, "1")
        .await
        .unwrap();
    assert!(fixture
        .state
        .state
        .lock()
        .unwrap()
        .entries
        .iter()
        .any(|entry| entry.enabled));
}

#[tokio::test]
async fn authoritative_import_prescan_can_reconcile_unresolved_removal_authority() {
    use crate::plugin::import::PreparedManifest;
    let fixture = Fixture::new().await;
    *fixture.package.rename.lock().unwrap() = Rename::AmbiguousUnchanged;
    *fixture.package.cleanup.lock().unwrap() = CleanupOutcome::Conflict;
    let now = std::time::Instant::now();
    let preview = fixture
        .runtime
        .import_sessions
        .reserve_prepare(now)
        .unwrap()
        .publish(PreparedManifest::parse(VALID).unwrap(), 1, now)
        .unwrap();
    assert_eq!(
        fixture.remove().await["status"],
        "removedCatalogUnconfirmed"
    );
    let result = serde_json::to_value(
        fixture
            .runtime
            .commit_import(&preview.token, "1")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["reasonCode"], "plugin_catalog_stale");
    let reconciled = fixture.runtime.registry.read().await.catalog_snapshot();
    assert_eq!(reconciled.catalog_generation, "2");
    fixture
        .runtime
        .set_enabled("com.example.notes", true, &reconciled.catalog_generation)
        .await
        .unwrap();
    assert!(fixture.index.index.lock().unwrap().entries()[0]
        .removal_slot()
        .is_none());
}

#[tokio::test]
async fn fully_clean_result_returns_removed_with_real_shared_package_storage() {
    use crate::storage::local_plugin_import::LocalManifestImportStorage;
    struct DiskDiscovery(std::path::PathBuf);
    impl LocalPluginDiscovery for DiskDiscovery {
        fn discover(&self) -> LocalDiscoveryOutcome {
            discover_from_plugins_root(&self.0)
        }
    }
    // The sandbox denies safe ancestor traversal through the user-home chain.
    let base = std::env::current_dir().unwrap().join("target");
    std::fs::create_dir_all(&base).unwrap();
    let directory = tempfile::tempdir_in(base).unwrap();
    let root = directory.path().canonicalize().unwrap().join("plugins");
    let packages = Arc::new(SystemLocalPluginPackageStorage::with_plugins_root(
        root.clone(),
    ));
    let record = record();
    let mut stage = packages
        .prepare_stage(&record, &record.canonical_manifest_bytes().unwrap())
        .unwrap();
    let promotion = stage.promote().unwrap();
    let index_store = ManagedOwnershipStore::with_plugins_root(root.clone());
    index_store
        .save(
            &ManagedOwnershipIndexV1::empty()
                .register(promotion.entry)
                .unwrap(),
        )
        .unwrap();
    let state_store = PluginStateStore::with_path(root.join("state.json"));
    let registry = PluginRegistry::initialize(vec![], Box::new(state_store), Box::new(index_store));
    let runtime = Arc::new(PluginRuntime::with_lifecycle_services(
        registry,
        Arc::new(DiskDiscovery(root.clone())),
        Arc::new(SystemLocalManifestReader),
        packages.clone(),
        packages,
    ));
    let initial = runtime.get_catalog().await.unwrap();
    let result = serde_json::to_value(
        runtime
            .remove_managed_local_plugin("com.example.notes", &initial.catalog_generation)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(result["status"], "removed", "{result}");
    assert_eq!(std::fs::read_dir(root.join("local")).unwrap().count(), 0);
    assert_eq!(
        std::fs::read_dir(root.join("removal-staging"))
            .unwrap()
            .count(),
        0
    );
    assert!(ManagedOwnershipStore::with_plugins_root(root)
        .load()
        .unwrap()
        .index
        .entries()
        .is_empty());
}
