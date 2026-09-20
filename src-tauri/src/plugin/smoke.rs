use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::plugin::discovery::{
    discover_from_plugins_root, LocalDiscoveryOutcome, LocalPluginDiscovery,
};
use crate::plugin::import::{
    LocalManifestReader, LocalManifestSelector, PreparedManifest, SelectedManifestSource,
    SystemLocalManifestReader,
};
use crate::plugin::manifest::PluginSource;
use crate::plugin::{PluginRegistry, PluginRuntime};
use crate::storage::local_plugin_package::SystemLocalPluginPackageStorage;
use crate::storage::managed_plugin_ownership::{
    ManagedOwnershipPersistence, ManagedOwnershipStore,
};
use crate::storage::plugin_state::{PluginStatePersistence, PluginStateStore};

mod account_host;
pub(crate) use account_host::SmokeWorkflowHost;

const FIXTURE_MANIFEST_BYTES: &[u8] =
    include_bytes!("../../../examples/plugins/series-sma/manifest.json");
const ACCOUNT_FIXTURE_MANIFEST_BYTES: &[u8] =
    include_bytes!("../../../examples/plugins/account-workflow/manifest.json");
const FIXTURE_PLUGIN_ID: &str = "com.easiflux.examples.series-sma";
const ACCOUNT_FIXTURE_PLUGIN_ID: &str = "com.easiflux.examples.account-workflow";
const OUTSIDE_ARTIFACT_ROOT: &str =
    "plugin smoke parent is outside the build checkout target directory";

pub(crate) struct PluginSmokeProfile {
    pub(crate) root: PathBuf,
    pub(crate) source: PathBuf,
    pub(crate) account_source: PathBuf,
    runtime: Arc<PluginRuntime>,
    workflow_host: Arc<SmokeWorkflowHost>,
}

impl PluginSmokeProfile {
    pub(crate) fn create(parent: &Path) -> Result<Self, String> {
        if !parent.is_absolute() {
            return Err("plugin smoke parent must be absolute".into());
        }
        let metadata = std::fs::metadata(parent)
            .map_err(|error| format!("plugin smoke parent is unavailable: {error}"))?;
        if !metadata.is_dir() {
            return Err("plugin smoke parent must be a directory".into());
        }
        let parent = parent
            .canonicalize()
            .map_err(|error| format!("failed to canonicalize plugin smoke parent: {error}"))?;
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or_else(|| "plugin smoke build checkout is unavailable".to_owned())?
            .canonicalize()
            .map_err(|error| format!("failed to canonicalize plugin smoke checkout: {error}"))?;
        let artifact_root = checkout
            .join("target")
            .canonicalize()
            .map_err(|error| format!("plugin smoke build target is unavailable: {error}"))?;
        if !artifact_root.starts_with(&checkout) {
            return Err("plugin smoke build target must remain within checkout".into());
        }
        if !parent.starts_with(&artifact_root) {
            return Err(OUTSIDE_ARTIFACT_ROOT.into());
        }
        let root = parent.join(format!("plugin-smoke-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root)
            .map_err(|error| format!("failed to reserve plugin smoke root: {error}"))?;
        let root = root
            .canonicalize()
            .map_err(|error| format!("failed to canonicalize plugin smoke root: {error}"))?;
        if !root.starts_with(&checkout) {
            return Err("plugin smoke profile root escaped build checkout".into());
        }
        let source_dir = root.join("source");
        std::fs::create_dir(&source_dir)
            .map_err(|error| format!("failed to create plugin smoke source directory: {error}"))?;
        let source = source_dir.join("manifest.json");
        std::fs::write(&source, FIXTURE_MANIFEST_BYTES)
            .map_err(|error| format!("failed to write plugin smoke fixture: {error}"))?;
        let source = source
            .canonicalize()
            .map_err(|error| format!("failed to canonicalize plugin smoke fixture: {error}"))?;
        let account_source = source_dir.join("account-workflow.json");
        std::fs::write(&account_source, ACCOUNT_FIXTURE_MANIFEST_BYTES)
            .map_err(|error| format!("failed to write account workflow smoke fixture: {error}"))?;
        let account_source = account_source.canonicalize().map_err(|error| {
            format!("failed to canonicalize account workflow smoke fixture: {error}")
        })?;

        let plugins_root = root.join("plugins");
        let packages = Arc::new(SystemLocalPluginPackageStorage::with_plugins_root(
            plugins_root.clone(),
        ));
        packages
            .open()
            .map_err(|error| format!("failed to initialize plugin smoke package roots: {error}"))?;
        let registry = PluginRegistry::initialize(
            crate::plugin::builtin::builtin_manifests(),
            Box::new(PluginStateStore::with_path(plugins_root.join("state.json"))),
            Box::new(ManagedOwnershipStore::with_plugins_root(
                plugins_root.clone(),
            )),
        );
        let runtime = Arc::new(PluginRuntime::with_lifecycle_services(
            registry,
            Arc::new(FixedRootDiscovery(plugins_root)),
            Arc::new(FixtureOnlyManifestReader {
                sources: [source.clone(), account_source.clone()],
            }),
            packages.clone(),
            packages,
        ));

        Ok(Self {
            root,
            source,
            account_source,
            runtime,
            workflow_host: Arc::new(SmokeWorkflowHost::new()),
        })
    }

    pub(crate) fn runtime(&self) -> Arc<PluginRuntime> {
        Arc::clone(&self.runtime)
    }

    pub(crate) fn selector(&self) -> Arc<dyn LocalManifestSelector> {
        Arc::new(SequencedFixtureSelector {
            sources: [self.source.clone(), self.account_source.clone()],
            selections: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    pub(crate) fn workflow_host(&self) -> Arc<dyn crate::plugin::workflow::WorkflowHost> {
        self.workflow_host.clone()
    }

    pub(crate) fn inspect_final_state(&self) -> Result<SmokeNativeChecks, String> {
        let plugins_root = self.root.join("plugins");
        let state = PluginStateStore::with_path(plugins_root.join("state.json"))
            .load()
            .map_err(|error| format!("failed to inspect plugin smoke state: {error}"))?;
        let ownership = ManagedOwnershipStore::with_plugins_root(plugins_root.clone())
            .load()
            .map_err(|error| format!("failed to inspect plugin smoke ownership: {error}"))?;
        let disabled = |plugin_id: &str| {
            state.state.entries.iter().any(|entry| {
                entry.id.as_str() == plugin_id
                    && entry.source == PluginSource::LocalDeclarative
                    && !entry.enabled
            })
        };
        let (place_mutations, cancel_mutations) = self.workflow_host.mutation_counts();
        Ok(SmokeNativeChecks {
            source_unchanged: std::fs::read(&self.source)
                .is_ok_and(|bytes| bytes == FIXTURE_MANIFEST_BYTES)
                && std::fs::read(&self.account_source)
                    .is_ok_and(|bytes| bytes == ACCOUNT_FIXTURE_MANIFEST_BYTES),
            disabled_decision_retained: disabled(FIXTURE_PLUGIN_ID)
                && disabled(ACCOUNT_FIXTURE_PLUGIN_ID),
            ownership_empty: ownership.index.entries().is_empty(),
            local_empty: directory_is_empty(&plugins_root.join("local"))?,
            staging_empty: directory_is_empty(&plugins_root.join("import-staging"))?
                && directory_is_empty(&plugins_root.join("removal-staging"))?,
            place_mutations,
            cancel_mutations,
        })
    }
}

struct FixedRootDiscovery(PathBuf);

impl LocalPluginDiscovery for FixedRootDiscovery {
    fn discover(&self) -> LocalDiscoveryOutcome {
        discover_from_plugins_root(&self.0)
    }
}

struct FixtureOnlyManifestReader {
    sources: [PathBuf; 2],
}

impl LocalManifestReader for FixtureOnlyManifestReader {
    fn read(&self, path: &Path) -> AppResult<PreparedManifest> {
        let selected = path.canonicalize().map_err(|_| source_rejected())?;
        if !self.sources.contains(&selected) {
            return Err(source_rejected());
        }
        SystemLocalManifestReader.read(&selected)
    }
}

fn source_rejected() -> AppError {
    AppError::Plugin {
        code: "plugin_import_source_rejected",
        message: "无法安全读取所选文件，请选择普通本地 JSON 文件。",
        diagnostic: None,
    }
}

fn directory_is_empty(path: &Path) -> Result<bool, String> {
    let mut entries = std::fs::read_dir(path)
        .map_err(|error| format!("failed to inspect plugin smoke directory: {error}"))?;
    Ok(entries.next().is_none())
}

#[cfg(test)]
struct FixedFixtureSelector(PathBuf);

#[cfg(test)]
impl LocalManifestSelector for FixedFixtureSelector {
    fn select(
        &self,
    ) -> Pin<Box<dyn Future<Output = AppResult<SelectedManifestSource>> + Send + '_>> {
        let source = self.0.clone();
        Box::pin(async move { Ok(SelectedManifestSource::Selected(source)) })
    }
}

struct SequencedFixtureSelector {
    sources: [PathBuf; 2],
    selections: std::sync::atomic::AtomicUsize,
}

impl LocalManifestSelector for SequencedFixtureSelector {
    fn select(
        &self,
    ) -> Pin<Box<dyn Future<Output = AppResult<SelectedManifestSource>> + Send + '_>> {
        use std::sync::atomic::Ordering;

        // The legacy flow previews/cancels once, then imports. Every later selection
        // resolves to the account-workflow fixture. No frontend path is accepted.
        let index = usize::from(self.selections.fetch_add(1, Ordering::AcqRel) >= 2);
        let source = self.sources[index].clone();
        Box::pin(async move { Ok(SelectedManifestSource::Selected(source)) })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SmokeNativeChecks {
    pub(crate) source_unchanged: bool,
    pub(crate) disabled_decision_retained: bool,
    pub(crate) ownership_empty: bool,
    pub(crate) local_empty: bool,
    pub(crate) staging_empty: bool,
    pub(crate) place_mutations: usize,
    pub(crate) cancel_mutations: usize,
}

impl SmokeNativeChecks {
    pub(crate) fn passed(&self) -> bool {
        self.source_unchanged
            && self.disabled_decision_retained
            && self.ownership_empty
            && self.local_empty
            && self.staging_empty
            && self.place_mutations == 1
            && self.cancel_mutations == 1
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::AtomicBool;

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde_json::Value;
    use tempfile::TempDir;

    use super::*;
    use crate::plugin::compute::execute_guest;
    use crate::plugin::contribution::PluginCommandParams;
    use crate::plugin::manifest::PluginManifestV1;

    const SERIES_SMA_MANIFEST: &str =
        include_str!("../../../examples/plugins/series-sma/manifest.json");
    const SERIES_SMA_WAT: &str = include_str!("../../../examples/plugins/series-sma/plugin.wat");
    const ACCOUNT_WORKFLOW_MANIFEST: &str =
        include_str!("../../../examples/plugins/account-workflow/manifest.json");
    const ACCOUNT_WORKFLOW_WAT: [(&str, &str); 3] = [
        (
            "account.available-balance",
            include_str!("../../../examples/plugins/account-workflow/balance.wat"),
        ),
        (
            "account.place-order",
            include_str!("../../../examples/plugins/account-workflow/place.wat"),
        ),
        (
            "account.cancel-first-open-order",
            include_str!("../../../examples/plugins/account-workflow/cancel.wat"),
        ),
    ];

    fn fixture_parent() -> TempDir {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target");
        fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("plugin-smoke-test-parent-")
            .tempdir_in(base)
            .unwrap()
    }

    fn smoke_children(parent: &Path) -> Vec<PathBuf> {
        fs::read_dir(parent)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("plugin-smoke-"))
            })
            .collect()
    }

    fn child_names(parent: &Path) -> Vec<std::ffi::OsString> {
        let mut names = fs::read_dir(parent)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    // Catches a checked-in manifest blob that is unrelated to its author-readable source,
    // or an example that delegates SMA arithmetic to the host.
    #[test]
    fn checked_in_series_sma_source_matches_manifest_and_runs_in_production_interpreter() {
        let manifest: PluginManifestV1 = serde_json::from_str(SERIES_SMA_MANIFEST).unwrap();
        assert_eq!(manifest.schema_version, 4);
        assert_eq!(manifest.contributions.len(), 1);
        let params = match &manifest.contributions[0].params {
            PluginCommandParams::ComputeSeries(params) => params,
            other => panic!("series SMA contribution is not executable: {other:?}"),
        };

        let compiled = wat::parse_str(SERIES_SMA_WAT).unwrap();
        assert_eq!(params.module_base64, STANDARD.encode(compiled));
        assert_eq!(
            execute_guest(
                params,
                &[1.0, 2.0, 3.0, 4.0, 5.0],
                3.0,
                &AtomicBool::new(false),
            )
            .unwrap(),
            4.0
        );
        for parameter in [2.5, 6.0] {
            let error = execute_guest(
                params,
                &[1.0, 2.0, 3.0, 4.0, 5.0],
                parameter,
                &AtomicBool::new(false),
            )
            .unwrap_err();
            assert_eq!(
                serde_json::to_value(error).unwrap()["code"],
                "plugin_compute_invalid_output"
            );
        }
    }

    // Catches stale Base64, host-computed demo results, or a guest that ignores the
    // granted snapshot/user proposal while still producing superficially valid JSON.
    #[test]
    fn checked_in_account_workflow_sources_match_and_run_in_production_interpreter() {
        let manifest: PluginManifestV1 =
            serde_json::from_str(ACCOUNT_WORKFLOW_MANIFEST).unwrap();
        assert_eq!(manifest.schema_version, 5);
        assert_eq!(manifest.contributions.len(), 3);

        let execute = |contribution_id: &str, context: Value, input: Value| -> Value {
            let contribution = manifest
                .contributions
                .iter()
                .find(|item| item.contribution_id.as_str() == contribution_id)
                .unwrap();
            let params = match &contribution.params {
                PluginCommandParams::AccountWorkflow(params) => params,
                other => panic!("account workflow contribution is not executable: {other:?}"),
            };
            let source = ACCOUNT_WORKFLOW_WAT
                .iter()
                .find_map(|(id, source)| (*id == contribution_id).then_some(*source))
                .unwrap();
            let compiled = wat::parse_str(source).unwrap();
            assert_eq!(params.module_base64, STANDARD.encode(&compiled));
            let output = crate::plugin::compute::sandbox::execute_json(
                &compiled,
                &serde_json::to_vec(&context).unwrap(),
                &serde_json::to_vec(&input).unwrap(),
                &AtomicBool::new(false),
            )
            .unwrap();
            serde_json::from_slice(&output).unwrap()
        };
        let snapshot = |available: &str, order_id: &str| {
            serde_json::json!({
                "schemaVersion": 1,
                "account": {
                    "accountId": "plugin-smoke-account",
                    "sessionEpoch": "1",
                    "environment": "Injected synthetic smoke host"
                },
                "capturedAtMs": "1789948800000",
                "symbol": "BTCUSDT",
                "grantedCapabilities": [
                    "account.read", "balances.read", "orders.read",
                    "trade.place", "trade.cancel"
                ],
                "balances": {
                    "items": [{"asset": "USDT", "available": available, "frozen": "0", "total": available}],
                    "fetchedAtMs": "1789948799900", "partial": true
                },
                "positions": null,
                "orders": {
                    "items": [{
                        "orderId": order_id, "symbol": "BTCUSDT", "side": "Buy",
                        "orderType": "Limit", "price": "50000", "qty": "0.001",
                        "status": "New", "orderLinkId": "plugin-smoke-existing-link",
                        "filledQty": "0", "avgPrice": "0"
                    }],
                    "fetchedAtMs": "1789948799950", "partial": true
                },
                "market": null
            })
        };

        assert_eq!(
            execute(
                "account.available-balance",
                snapshot("12.5", "plugin-smoke-open-order"),
                serde_json::json!({})
            ),
            serde_json::json!({"kind": "display", "text": "Available balance: 12.5"})
        );
        assert_eq!(
            execute(
                "account.available-balance",
                snapshot("8", "plugin-smoke-open-order"),
                serde_json::json!({})
            ),
            serde_json::json!({"kind": "display", "text": "Available balance: 8"})
        );
        let proposal = serde_json::json!({
            "kind": "placeOrder",
            "order": {
                "symbol": "BTCUSDT", "side": "Buy", "orderType": "Limit",
                "qty": "0.002", "price": "49000", "timeInForce": "GTC",
                "positionIdx": 1, "reduceOnly": false
            }
        });
        assert_eq!(
            execute(
                "account.place-order",
                snapshot("12.5", "plugin-smoke-open-order"),
                proposal.clone()
            ),
            proposal
        );
        assert_eq!(
            execute(
                "account.cancel-first-open-order",
                snapshot("12.5", "captured-order-77"),
                serde_json::json!({})
            ),
            serde_json::json!({
                "kind": "cancelOrder",
                "order": {"symbol": "BTCUSDT", "orderId": "captured-order-77"}
            })
        );
    }

    // Catches accepting an ambiguous parent before reserving a fresh owned child.
    #[test]
    fn create_rejects_relative_missing_and_non_directory_parents_without_child_creation() {
        let relative = Path::new("target");
        fs::create_dir_all(relative).unwrap();
        assert_eq!(
            PluginSmokeProfile::create(relative).err().unwrap(),
            "plugin smoke parent must be absolute"
        );

        let missing_parent = fixture_parent();
        let missing = missing_parent.path().join("missing");
        assert!(PluginSmokeProfile::create(&missing).is_err());
        assert!(smoke_children(missing_parent.path()).is_empty());

        let file_parent = fixture_parent();
        let file = file_parent.path().join("sentinel.txt");
        fs::write(&file, b"fixture-owned sentinel").unwrap();
        assert!(PluginSmokeProfile::create(&file).is_err());
        assert!(smoke_children(file_parent.path()).is_empty());
        assert_eq!(fs::read(file).unwrap(), b"fixture-owned sentinel");
    }

    // Catches reserving a UUID child before rejecting an unsupported parent topology.
    #[test]
    fn create_rejects_system_temp_parent_as_out_of_workspace_without_writes() {
        let parent = tempfile::tempdir().unwrap();
        let before = child_names(parent.path());

        let error = PluginSmokeProfile::create(parent.path()).err().unwrap();

        assert_eq!(
            error,
            "plugin smoke parent is outside the build checkout target directory"
        );
        assert_eq!(child_names(parent.path()), before);
    }

    // Catches profile reuse or accidental sharing of one runtime/root.
    #[test]
    fn create_reserves_distinct_fresh_profiles() {
        let parent = fixture_parent();
        let first = PluginSmokeProfile::create(parent.path()).unwrap();
        let second = PluginSmokeProfile::create(parent.path()).unwrap();

        assert_ne!(first.root, second.root);
        assert_ne!(first.source, second.source);
        assert_ne!(first.account_source, second.account_source);
        assert_ne!(first.source, first.account_source);
        assert_eq!(smoke_children(parent.path()).len(), 2);
    }

    // Catches a fixture selector bypass that lets the runtime read any selected file.
    #[tokio::test]
    async fn runtime_rejects_non_fixture_source_before_parsing_its_bytes() {
        let parent = fixture_parent();
        let profile = PluginSmokeProfile::create(parent.path()).unwrap();
        let sentinel = profile.root.join("independent-sentinel.json");
        fs::write(&sentinel, FIXTURE_MANIFEST_BYTES).unwrap();

        let error = match profile
            .runtime()
            .prepare_import(Arc::new(FixedFixtureSelector(sentinel)))
            .await
        {
            Ok(_) => panic!("non-fixture source was accepted"),
            Err(error) => error,
        };

        assert_eq!(
            serde_json::to_value(error).unwrap()["code"],
            "plugin_import_source_rejected"
        );
    }

    // Catches live-store fallback, shared profiles, destructive source handling, or lost disable state.
    #[tokio::test]
    async fn real_import_remove_cycle_is_isolated_and_retains_native_invariants() {
        let parent = fixture_parent();
        let profile = PluginSmokeProfile::create(parent.path()).unwrap();
        let other = PluginSmokeProfile::create(parent.path()).unwrap();
        let source_before = fs::read(&profile.source).unwrap();
        let runtime = profile.runtime();

        runtime.get_catalog().await.unwrap();
        let ready = serde_json::to_value(runtime.prepare_import(profile.selector()).await.unwrap())
            .unwrap();
        assert_eq!(ready["status"], "ready");
        let imported = runtime
            .commit_import(
                ready["token"].as_str().unwrap(),
                ready["catalogGeneration"].as_str().unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(imported).unwrap()["status"],
            "imported"
        );

        let catalog = runtime.get_catalog().await.unwrap();
        let removed = runtime
            .remove_managed_local_plugin(FIXTURE_PLUGIN_ID, &catalog.catalog_generation)
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(removed).unwrap()["status"], "removed");

        assert_eq!(fs::read(&profile.source).unwrap(), source_before);
        assert!(!profile.inspect_final_state().unwrap().passed());
        let other_catalog =
            serde_json::to_value(other.runtime().get_catalog().await.unwrap()).unwrap();
        assert!(!other_catalog["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["manifest"]["id"] == FIXTURE_PLUGIN_ID));

        let checks = serde_json::to_value(profile.inspect_final_state().unwrap()).unwrap();
        assert_eq!(
            checks,
            serde_json::json!({
                "sourceUnchanged": true,
                "disabledDecisionRetained": false,
                "ownershipEmpty": true,
                "localEmpty": true,
                "stagingEmpty": true,
                "placeMutations": 0,
                "cancelMutations": 0
            })
        );
        assert_eq!(ready["schemaVersion"], 2);
        assert_eq!(
            ready["assessment"],
            serde_json::json!({ "kind": "notInCatalog" })
        );
        assert_eq!(ready["manifest"]["id"], FIXTURE_PLUGIN_ID);
        assert_eq!(ready.get("sourcePath"), None::<&Value>);
    }
}
