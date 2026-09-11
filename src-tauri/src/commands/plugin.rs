use std::sync::Arc;

use tauri::{State, WebviewWindow};

use crate::error::AppResult;
use crate::plugin::import::dialog::NativeLocalManifestSelector;
use crate::plugin::import::{
    CancelImportResult, CommitImportResult, LocalManifestSelector, PrepareImportResult,
};
use crate::plugin::manifest::{PluginCatalogMutationResult, PluginCatalogSnapshot};
use crate::plugin::removal::RemoveManagedLocalPluginResult;
use crate::plugin::PluginRuntime;

pub(crate) struct PluginCommandState {
    runtime: Arc<PluginRuntime>,
    #[cfg(any(test, feature = "plugin-smoke"))]
    selector: Option<Arc<dyn LocalManifestSelector>>,
}

impl PluginCommandState {
    pub(crate) fn new(runtime: Arc<PluginRuntime>) -> Self {
        Self {
            runtime,
            #[cfg(any(test, feature = "plugin-smoke"))]
            selector: None,
        }
    }

    #[cfg(any(test, feature = "plugin-smoke"))]
    pub(crate) fn with_selector(
        runtime: Arc<PluginRuntime>,
        selector: Arc<dyn LocalManifestSelector>,
    ) -> Self {
        Self {
            runtime,
            selector: Some(selector),
        }
    }

    fn selector(&self, window: WebviewWindow) -> Arc<dyn LocalManifestSelector> {
        #[cfg(any(test, feature = "plugin-smoke"))]
        if let Some(selector) = &self.selector {
            return Arc::clone(selector);
        }
        Arc::new(NativeLocalManifestSelector::new(window))
    }
}

pub(crate) async fn get_plugin_catalog_from(
    runtime: &Arc<PluginRuntime>,
) -> AppResult<PluginCatalogSnapshot> {
    runtime.get_catalog().await
}

pub(crate) async fn reload_plugin_catalog_from(
    runtime: &Arc<PluginRuntime>,
) -> AppResult<PluginCatalogSnapshot> {
    runtime.reload_catalog().await
}

pub(crate) async fn set_plugin_enabled_from(
    runtime: &Arc<PluginRuntime>,
    id: &str,
    enabled: bool,
    expected_catalog_generation: &str,
) -> AppResult<PluginCatalogMutationResult> {
    runtime
        .set_enabled(id, enabled, expected_catalog_generation)
        .await
}

pub(crate) async fn prepare_local_manifest_import_from(
    runtime: &Arc<PluginRuntime>,
    selector: Arc<dyn LocalManifestSelector>,
) -> AppResult<PrepareImportResult> {
    runtime.prepare_import(selector).await
}

pub(crate) fn cancel_local_manifest_import_from(
    runtime: &Arc<PluginRuntime>,
    token: &str,
) -> AppResult<CancelImportResult> {
    runtime.cancel_import(token)
}

pub(crate) async fn commit_local_manifest_import_from(
    runtime: &Arc<PluginRuntime>,
    token: &str,
    expected_catalog_generation: &str,
) -> AppResult<CommitImportResult> {
    runtime
        .commit_import(token, expected_catalog_generation)
        .await
}

pub(crate) async fn remove_managed_local_plugin_from(
    runtime: &Arc<PluginRuntime>,
    id: &str,
    expected_catalog_generation: &str,
) -> AppResult<RemoveManagedLocalPluginResult> {
    runtime
        .remove_managed_local_plugin(id, expected_catalog_generation)
        .await
}

#[tauri::command]
pub async fn get_plugin_catalog(
    state: State<'_, PluginCommandState>,
) -> AppResult<PluginCatalogSnapshot> {
    get_plugin_catalog_from(&state.runtime).await
}

#[tauri::command]
pub async fn reload_plugin_catalog(
    state: State<'_, PluginCommandState>,
) -> AppResult<PluginCatalogSnapshot> {
    reload_plugin_catalog_from(&state.runtime).await
}

#[tauri::command]
pub async fn set_plugin_enabled(
    state: State<'_, PluginCommandState>,
    id: String,
    enabled: bool,
    expected_catalog_generation: String,
) -> AppResult<PluginCatalogMutationResult> {
    set_plugin_enabled_from(&state.runtime, &id, enabled, &expected_catalog_generation).await
}

#[tauri::command]
pub async fn prepare_local_manifest_import(
    window: WebviewWindow,
    state: State<'_, PluginCommandState>,
) -> AppResult<PrepareImportResult> {
    let selector = state.selector(window);
    prepare_local_manifest_import_from(&state.runtime, selector).await
}

#[tauri::command]
pub async fn cancel_local_manifest_import(
    state: State<'_, PluginCommandState>,
    token: String,
) -> AppResult<CancelImportResult> {
    cancel_local_manifest_import_from(&state.runtime, &token)
}

#[tauri::command]
pub async fn commit_local_manifest_import(
    state: State<'_, PluginCommandState>,
    token: String,
    expected_catalog_generation: String,
) -> AppResult<CommitImportResult> {
    commit_local_manifest_import_from(&state.runtime, &token, &expected_catalog_generation).await
}

#[tauri::command]
pub async fn remove_managed_local_plugin(
    state: State<'_, PluginCommandState>,
    id: String,
    expected_catalog_generation: String,
) -> AppResult<RemoveManagedLocalPluginResult> {
    remove_managed_local_plugin_from(&state.runtime, &id, &expected_catalog_generation).await
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};

    use super::*;
    use crate::error::AppError;
    use crate::plugin::discovery::{
        LocalDiscoveryOutcome, LocalPackageLocator, LocalPluginDiscovery,
    };
    use crate::plugin::import::source::LocalManifestReader;
    use crate::plugin::import::{
        test_support::VALID, ImportCommitFailure, LocalManifestSelector, PrepareImportResult,
        PreparedManifest, SelectedManifestSource, SystemLocalManifestReader,
    };
    use crate::plugin::manifest::{PluginId, PluginManifestV1, PluginPublisherId, PluginSource};
    use crate::plugin::ownership::ManagedOwnershipEntryV1;
    use crate::plugin::record::PluginRecord;
    use crate::plugin::removal::{BeforeDisabledFailure, RemoveManagedLocalPluginResult};
    use crate::plugin::PluginRegistry;
    use crate::storage::local_plugin_import::{LocalManifestImportStorage, OwnedImportStage};
    use crate::storage::local_plugin_package::{
        ManagedLocalPluginRemovalStorage, OwnedRemoval, RemovalStorageFailure,
    };
    use crate::storage::plugin_state::{
        PluginStateEntryV2, PluginStateFileV2, PluginStateLoad, PluginStatePersistence,
    };

    #[derive(Clone)]
    struct MemoryPersistence(Arc<Mutex<MemoryState>>);

    struct MemoryState {
        persisted: PluginStateFileV2,
        load_error: bool,
        requires_rewrite: bool,
        loads: usize,
        saves: Vec<PluginStateFileV2>,
    }

    impl MemoryPersistence {
        fn new(persisted: PluginStateFileV2) -> Self {
            Self(Arc::new(Mutex::new(MemoryState {
                persisted,
                load_error: false,
                requires_rewrite: false,
                loads: 0,
                saves: Vec::new(),
            })))
        }

        fn registry(&self, manifests: Vec<PluginManifestV1>) -> PluginRegistry {
            PluginRegistry::initialize(
                manifests,
                Box::new(self.clone()),
                crate::plugin::ownership::empty_test_persistence(),
            )
        }
    }

    impl PluginStatePersistence for MemoryPersistence {
        fn load(&self) -> AppResult<PluginStateLoad> {
            let mut state = self.0.lock().unwrap();
            state.loads += 1;
            if state.load_error {
                return Err(storage_error());
            }
            Ok(PluginStateLoad {
                state: state.persisted.clone(),
                requires_rewrite: state.requires_rewrite,
            })
        }

        fn save(
            &self,
            next: &PluginStateFileV2,
        ) -> crate::storage::safe_plugin_document::PersistResult {
            let mut state = self.0.lock().unwrap();
            state.saves.push(next.clone());
            state.persisted = next.clone();
            Ok(if cfg!(windows) {
                crate::storage::safe_plugin_document::PersistOutcome::CommittedProcessCrashSafe
            } else {
                crate::storage::safe_plugin_document::PersistOutcome::CommittedDurable
            })
        }
    }

    fn storage_error() -> AppError {
        AppError::Plugin {
            code: "plugin_state_unavailable",
            message: "插件状态存储暂不可用",
            diagnostic: Some("private config path".into()),
        }
    }

    fn manifest() -> PluginManifestV1 {
        serde_json::from_value(json!({
            "schemaVersion": 1,
            "id": "com.easiflux.alpha",
            "name": "Alpha",
            "version": "1.2.3",
            "description": "Metadata only",
            "publisherId": "com.easiflux",
            "publisher": "EasiFlux",
            "contributions": [],
            "requestedCapabilities": []
        }))
        .unwrap()
    }

    fn enabled_entry() -> PluginStateEntryV2 {
        PluginStateEntryV2 {
            id: PluginId::parse("com.easiflux.alpha").unwrap(),
            source: PluginSource::BuiltIn,
            publisher_id: PluginPublisherId::parse("com.easiflux").unwrap(),
            approval_fingerprint: "v1:none".into(),
            enabled: true,
        }
    }

    struct FixedDiscovery(LocalDiscoveryOutcome);

    impl LocalPluginDiscovery for FixedDiscovery {
        fn discover(&self) -> LocalDiscoveryOutcome {
            self.0.clone()
        }
    }

    struct FixedSelector(PathBuf);

    impl LocalManifestSelector for FixedSelector {
        fn select(
            &self,
        ) -> Pin<Box<dyn Future<Output = AppResult<SelectedManifestSource>> + Send + '_>> {
            let path = self.0.clone();
            Box::pin(async move { Ok(SelectedManifestSource::Selected(path)) })
        }
    }

    struct RecordingReader {
        paths: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl LocalManifestReader for RecordingReader {
        fn read(&self, path: &Path) -> AppResult<PreparedManifest> {
            self.paths.lock().unwrap().push(path.to_path_buf());
            PreparedManifest::parse(VALID)
        }
    }

    struct CountingStorage(Arc<AtomicUsize>);

    impl LocalManifestImportStorage for CountingStorage {
        fn prepare_stage(
            &self,
            _record: &crate::plugin::record::PluginRecord,
            _bytes: &[u8],
        ) -> Result<Box<dyn OwnedImportStage>, ImportCommitFailure> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(ImportCommitFailure::WriteFailed)
        }
    }

    struct CountingRemovalStorage(Arc<AtomicUsize>);

    impl ManagedLocalPluginRemovalStorage for CountingRemovalStorage {
        fn prepare(
            &self,
            _locator: &LocalPackageLocator,
            _entry: &ManagedOwnershipEntryV1,
        ) -> Result<Box<dyn OwnedRemoval>, RemovalStorageFailure> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(RemovalStorageFailure::Unavailable)
        }
    }

    fn import_runtime_with(
        persistence: &MemoryPersistence,
        paths: Arc<Mutex<Vec<PathBuf>>>,
        stage_calls: Arc<AtomicUsize>,
    ) -> Arc<PluginRuntime> {
        Arc::new(PluginRuntime::with_import_services(
            persistence.registry(vec![]),
            Arc::new(FixedDiscovery(LocalDiscoveryOutcome::available(vec![]))),
            Arc::new(RecordingReader { paths }),
            Arc::new(CountingStorage(stage_calls)),
        ))
    }

    fn runtime_with(persistence: &MemoryPersistence) -> Arc<PluginRuntime> {
        Arc::new(PluginRuntime::initialize(
            persistence.registry(vec![manifest()]),
            Arc::new(FixedDiscovery(LocalDiscoveryOutcome::available(vec![]))),
        ))
    }

    fn removal_runtime_with(
        persistence: &MemoryPersistence,
        removal_calls: Arc<AtomicUsize>,
    ) -> Arc<PluginRuntime> {
        Arc::new(PluginRuntime::with_lifecycle_services(
            persistence.registry(vec![]),
            Arc::new(FixedDiscovery(LocalDiscoveryOutcome::available(vec![]))),
            Arc::new(SystemLocalManifestReader),
            Arc::new(CountingStorage(Arc::new(AtomicUsize::new(0)))),
            Arc::new(CountingRemovalStorage(removal_calls)),
        ))
    }

    // Catches returning the stale unavailable snapshot without asking persistence again.
    #[tokio::test]
    async fn catalog_command_retries_unavailable_state_before_snapshotting() {
        let persistence = MemoryPersistence::new(PluginStateFileV2 {
            schema_version: 2,
            revision: 7,
            entries: vec![enabled_entry()],
        });
        persistence.0.lock().unwrap().load_error = true;
        let registry = runtime_with(&persistence);

        persistence.0.lock().unwrap().load_error = false;
        let snapshot = get_plugin_catalog_from(&registry).await.unwrap();

        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            json!({
                "schemaVersion": 3,
                "revision": "7",
                "catalogGeneration": "1",
                "availability": "available",
                "availabilityReasonCode": null,
                "localDiscovery": {"status": "available", "rejectedPackageCount": 0},
                "managedOwnership": {"status":"available","conflictingEntryCount":0,"rollbackPendingCount":0,"cleanupPendingCount":0},
                "plugins": [{
                    "manifest": {
                        "schemaVersion": 1,
                        "id": "com.easiflux.alpha",
                        "name": "Alpha",
                        "version": "1.2.3",
                        "description": "Metadata only",
                        "publisherId": "com.easiflux",
                        "publisher": "EasiFlux",
                        "contributions": [],
                        "requestedCapabilities": []
                    },
                    "source": "builtIn",
                    "management": "builtIn", "canRemove": false, "toggleBlockReasonCode": null,
                    "grantedCapabilities": [],
                    "status": "enabled",
                    "canToggle": true,
                    "statusReasonCode": null
                }]
            })
        );
        assert_eq!(persistence.0.lock().unwrap().loads, 2);
    }

    // Catches a helper substituting the current generation for the caller's stale token.
    #[tokio::test]
    async fn mutation_command_rejects_stale_or_noncanonical_generation_without_storage_access() {
        let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
        let registry = runtime_with(&persistence);
        for generation in ["1", "00", "+0", "", "18446744073709551616"] {
            let error = set_plugin_enabled_from(&registry, "com.easiflux.alpha", true, generation)
                .await
                .unwrap_err();
            assert_eq!(
                serde_json::to_value(error).unwrap()["code"],
                "plugin_catalog_stale"
            );
        }
        assert_eq!(persistence.0.lock().unwrap().loads, 1);
        assert!(persistence.0.lock().unwrap().saves.is_empty());
        assert_eq!(
            get_plugin_catalog_from(&registry).await.unwrap().revision,
            "0"
        );
    }

    // Catches losing the capacity code or transaction atomicity at the command boundary.
    #[tokio::test]
    async fn mutation_command_capacity_exceeded_preserves_transaction_and_rewrite() {
        assert_capacity_command_is_atomic(7).await;
    }

    #[tokio::test]
    async fn mutation_command_capacity_exceeded_precedes_revision_exhaustion() {
        assert_capacity_command_is_atomic(u64::MAX).await;
    }

    async fn assert_capacity_command_is_atomic(revision: u64) {
        let original = PluginStateFileV2 {
            schema_version: 2,
            revision,
            entries: (0..512)
                .map(|n| PluginStateEntryV2 {
                    id: PluginId::parse(format!("com.example.orphan{n:03}")).unwrap(),
                    enabled: false,
                    ..enabled_entry()
                })
                .collect(),
        };
        let persistence = MemoryPersistence::new(original.clone());
        persistence.0.lock().unwrap().requires_rewrite = true;
        let mut retained = manifest();
        retained.id = PluginId::parse("com.example.orphan000").unwrap();
        let runtime = Arc::new(PluginRuntime::initialize(
            persistence.registry(vec![manifest(), retained]),
            Arc::new(FixedDiscovery(LocalDiscoveryOutcome::available(vec![]))),
        ));
        let before = get_plugin_catalog_from(&runtime).await.unwrap();
        let error = set_plugin_enabled_from(&runtime, "com.easiflux.alpha", true, "1")
            .await
            .unwrap_err();
        let error = serde_json::to_value(error).unwrap();
        assert_eq!(error["code"], "plugin_state_capacity_exceeded");
        assert_eq!(error.as_object().unwrap().len(), 2);
        assert_eq!(get_plugin_catalog_from(&runtime).await.unwrap(), before);
        {
            let memory = persistence.0.lock().unwrap();
            assert!(memory.saves.is_empty());
            assert_eq!(memory.persisted, original);
        }
        // A same-value decision still rewrites after the failed transaction,
        // proving the registry retained its rewrite marker, including at u64::MAX.
        let rewritten = set_plugin_enabled_from(&runtime, "com.example.orphan000", false, "1")
            .await
            .unwrap();
        assert_eq!(rewritten.revision, revision.to_string());
        let memory = persistence.0.lock().unwrap();
        assert_eq!(memory.saves, vec![original.clone()]);
        assert_eq!(memory.persisted, original);
    }

    // Catches bypassing the registry transaction or discarding its committed revision/item.
    #[tokio::test]
    async fn mutation_command_returns_the_persisted_registry_result() {
        let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
        let registry = runtime_with(&persistence);

        let result = set_plugin_enabled_from(&registry, "com.easiflux.alpha", true, "0")
            .await
            .unwrap();

        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["schemaVersion"], 3);
        assert_eq!(value["revision"], "1");
        assert_eq!(value["catalogGeneration"], "0");
        assert_eq!(value["plugin"]["manifest"]["id"], "com.easiflux.alpha");
        assert_eq!(value["plugin"]["status"], "enabled");
        assert_eq!(value["plugin"]["canToggle"], true);
        assert_eq!(value["plugin"]["statusReasonCode"], Value::Null);
        let persisted = persistence.0.lock().unwrap();
        assert_eq!(persisted.saves.len(), 1);
        assert_eq!(persisted.persisted.revision, 1);
        assert_eq!(persisted.persisted.entries.len(), 1);
        assert!(persisted.persisted.entries[0].enabled);
    }

    // Catches omitting local discovery/reload at the IPC adapter or dropping the token.
    #[tokio::test]
    async fn mutation_command_requires_expected_catalog_generation_after_local_reload() {
        let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
        let local = PluginRecord::local_declarative(manifest()).unwrap();
        let runtime = Arc::new(PluginRuntime::initialize(
            persistence.registry(vec![]),
            Arc::new(FixedDiscovery(LocalDiscoveryOutcome::available(vec![
                local,
            ]))),
        ));
        let snapshot = reload_plugin_catalog_from(&runtime).await.unwrap();
        assert_eq!(snapshot.catalog_generation, "1");
        assert_eq!(snapshot.revision, "0");
        let result = set_plugin_enabled_from(&runtime, "com.easiflux.alpha", true, "1")
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({
                "schemaVersion": 3, "catalogGeneration": "1", "revision": "1",
                "plugin": {
                    "manifest": {
                        "schemaVersion": 1, "id": "com.easiflux.alpha", "name": "Alpha",
                        "version": "1.2.3", "description": "Metadata only",
                        "publisherId": "com.easiflux", "publisher": "EasiFlux",
                        "contributions": [], "requestedCapabilities": []
                    },
                    "source": "localDeclarative", "grantedCapabilities": [],
                    "management": "external", "canRemove": false, "toggleBlockReasonCode": null,
                    "status": "enabled", "canToggle": true, "statusReasonCode": null
                }
            })
        );
        let stale = set_plugin_enabled_from(&runtime, "com.easiflux.alpha", false, "0")
            .await
            .unwrap_err();
        assert_eq!(
            serde_json::to_value(stale).unwrap()["code"],
            "plugin_catalog_stale"
        );
        let again = reload_plugin_catalog_from(&runtime).await.unwrap();
        assert_eq!(again.catalog_generation, "1");
        assert_eq!(again.revision, "1");
        assert_eq!(persistence.0.lock().unwrap().saves.len(), 1);
    }

    #[test]
    fn public_import_command_signatures_expose_no_path_or_manifest_inputs() {
        fn prepare_only(window: WebviewWindow, state: State<'_, PluginCommandState>) {
            drop(prepare_local_manifest_import(window, state));
        }
        fn cancel_only(state: State<'_, PluginCommandState>, token: String) {
            drop(cancel_local_manifest_import(state, token));
        }
        fn commit_only(
            state: State<'_, PluginCommandState>,
            token: String,
            expected_catalog_generation: String,
        ) {
            drop(commit_local_manifest_import(
                state,
                token,
                expected_catalog_generation,
            ));
        }

        let _ = (prepare_only, cancel_only, commit_only);
    }

    #[tokio::test]
    async fn prepare_command_helper_returns_only_canonical_content_not_source_details() {
        let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
        let paths = Arc::new(Mutex::new(Vec::new()));
        let stage_calls = Arc::new(AtomicUsize::new(0));
        let runtime = import_runtime_with(&persistence, Arc::clone(&paths), stage_calls);
        let private_path = PathBuf::from(r"C:\private-source\never-leak.json");

        let result = prepare_local_manifest_import_from(
            &runtime,
            Arc::new(FixedSelector(private_path.clone())),
        )
        .await
        .unwrap();
        let wire = serde_json::to_value(&result).unwrap();
        let keys = wire
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            [
                "catalogGeneration",
                "expiresInSeconds",
                "manifest",
                "schemaVersion",
                "status",
                "token",
            ]
        );
        assert_eq!(*paths.lock().unwrap(), [private_path.clone()]);
        let serialized = serde_json::to_string(&wire).unwrap();
        assert!(!serialized.contains(private_path.to_string_lossy().as_ref()));
        assert!(!wire.as_object().unwrap().contains_key("path"));
        assert!(!wire.as_object().unwrap().contains_key("rawJson"));
        assert!(!wire.as_object().unwrap().contains_key("source"));
    }

    #[tokio::test]
    async fn commit_command_helper_forwards_expected_generation_unchanged() {
        let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
        let paths = Arc::new(Mutex::new(Vec::new()));
        let stage_calls = Arc::new(AtomicUsize::new(0));
        let runtime = import_runtime_with(&persistence, paths, Arc::clone(&stage_calls));
        let prepared = prepare_local_manifest_import_from(
            &runtime,
            Arc::new(FixedSelector(PathBuf::from(r"C:\selected.json"))),
        )
        .await
        .unwrap();
        let PrepareImportResult::Ready(preview) = prepared else {
            panic!("ready preview required");
        };
        let noncanonical = format!("0{}", preview.catalog_generation);

        let result = commit_local_manifest_import_from(&runtime, &preview.token, &noncanonical)
            .await
            .unwrap();
        let wire = serde_json::to_value(result).unwrap();
        assert_eq!(wire["status"], "notImported");
        assert_eq!(wire["reasonCode"], "plugin_catalog_stale");
        assert_eq!(wire["disabledDecisionSaved"], false);
        assert_eq!(stage_calls.load(Ordering::SeqCst), 0);
        assert!(persistence.0.lock().unwrap().saves.is_empty());
    }

    #[tokio::test]
    async fn cancel_command_helper_forwards_only_the_token() {
        let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
        let paths = Arc::new(Mutex::new(Vec::new()));
        let runtime = import_runtime_with(&persistence, paths, Arc::new(AtomicUsize::new(0)));
        let prepared = prepare_local_manifest_import_from(
            &runtime,
            Arc::new(FixedSelector(PathBuf::from(r"C:\selected.json"))),
        )
        .await
        .unwrap();
        let PrepareImportResult::Ready(preview) = prepared else {
            panic!("ready preview required");
        };

        let result = cancel_local_manifest_import_from(&runtime, &preview.token).unwrap();
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({"schemaVersion": 1, "status": "cancelled"})
        );
        assert!(persistence.0.lock().unwrap().saves.is_empty());
    }

    // Catches expanding the public command with a path, slot, receipt, fingerprint,
    // publisher, object identity, or raw manifest input, or normalizing either input.
    #[tokio::test]
    async fn remove_helper_forwards_only_id_and_expected_generation() {
        fn command_only(
            state: State<'_, PluginCommandState>,
            id: String,
            expected_catalog_generation: String,
        ) {
            drop(remove_managed_local_plugin(
                state,
                id,
                expected_catalog_generation,
            ));
        }

        let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
        let removal_calls = Arc::new(AtomicUsize::new(0));
        let runtime = removal_runtime_with(&persistence, Arc::clone(&removal_calls));
        let snapshot = runtime.get_catalog().await.unwrap();

        let noncanonical_id = remove_managed_local_plugin_from(
            &runtime,
            "com.EasiFlux.notes",
            &snapshot.catalog_generation,
        )
        .await;
        let Err(noncanonical_id) = noncanonical_id else {
            panic!("noncanonical id must remain invalid");
        };
        assert_eq!(
            serde_json::to_value(noncanonical_id).unwrap()["code"],
            "plugin_invalid_id"
        );

        let noncanonical_generation = remove_managed_local_plugin_from(
            &runtime,
            "com.example.notes",
            &format!("0{}", snapshot.catalog_generation),
        )
        .await
        .unwrap();
        let wire = serde_json::to_value(noncanonical_generation).unwrap();
        assert_eq!(wire["status"], "notRemoved");
        assert_eq!(wire["reasonCode"], "plugin_catalog_stale");
        assert_eq!(removal_calls.load(Ordering::SeqCst), 0);
        let _ = command_only;
    }

    // Catches dropping or reshaping any Task 6 removal result at the command boundary.
    #[tokio::test]
    async fn remove_command_serializes_every_result_branch() {
        let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
        let runtime = removal_runtime_with(&persistence, Arc::new(AtomicUsize::new(0)));
        let snapshot = runtime.get_catalog().await.unwrap();
        let snapshot_wire = serde_json::to_value(&snapshot).unwrap();
        let cases = [
            (
                RemoveManagedLocalPluginResult::removed(
                    "com.example.notes".into(),
                    snapshot.clone(),
                ),
                json!({
                    "schemaVersion": 1,
                    "status": "removed",
                    "pluginId": "com.example.notes",
                    "snapshot": snapshot_wire.clone()
                }),
            ),
            (
                RemoveManagedLocalPluginResult::removed_cleanup_pending(
                    "com.example.notes".into(),
                    snapshot.clone(),
                ),
                json!({
                    "schemaVersion": 1,
                    "status": "removedCleanupPending",
                    "pluginId": "com.example.notes",
                    "snapshot": snapshot_wire.clone()
                }),
            ),
            (
                RemoveManagedLocalPluginResult::removed_catalog_unconfirmed(
                    "com.example.notes".into(),
                    snapshot.clone(),
                ),
                json!({
                    "schemaVersion": 1,
                    "status": "removedCatalogUnconfirmed",
                    "pluginId": "com.example.notes",
                    "reasonCode": "plugin_remove_publication_unconfirmed",
                    "snapshot": snapshot_wire.clone()
                }),
            ),
            (
                RemoveManagedLocalPluginResult::not_removed(
                    BeforeDisabledFailure::NotManaged,
                    snapshot,
                ),
                json!({
                    "schemaVersion": 1,
                    "status": "notRemoved",
                    "disabledDecisionSaved": false,
                    "reasonCode": "plugin_remove_not_managed",
                    "snapshot": snapshot_wire
                }),
            ),
        ];

        for (result, expected) in cases {
            assert_eq!(serde_json::to_value(result).unwrap(), expected);
        }
    }

    // Catches translating a normal business refusal into a rejected IPC promise.
    #[tokio::test]
    async fn remove_command_keeps_business_rejections_in_ok() {
        let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
        let removal_calls = Arc::new(AtomicUsize::new(0));
        let runtime = removal_runtime_with(&persistence, Arc::clone(&removal_calls));
        let snapshot = runtime.get_catalog().await.unwrap();

        let result = remove_managed_local_plugin_from(
            &runtime,
            "com.example.notes",
            &snapshot.catalog_generation,
        )
        .await;
        assert!(result.is_ok(), "business refusal must remain an Ok result");
        let wire = serde_json::to_value(result.unwrap()).unwrap();
        assert_eq!(wire["status"], "notRemoved");
        assert_eq!(wire["reasonCode"], "plugin_remove_not_managed");
        assert_eq!(wire["disabledDecisionSaved"], false);
        assert_eq!(removal_calls.load(Ordering::SeqCst), 0);
    }
}
