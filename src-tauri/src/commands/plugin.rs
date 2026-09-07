use tauri::State;
use tokio::sync::RwLock;

use crate::error::AppResult;
use crate::plugin::manifest::{PluginCatalogMutationResult, PluginCatalogSnapshot};
use crate::plugin::PluginRegistry;
use crate::state::AppState;

pub(crate) async fn get_plugin_catalog_from(
    registry: &RwLock<PluginRegistry>,
) -> AppResult<PluginCatalogSnapshot> {
    let mut registry = registry.write().await;
    registry.retry_state_load();
    Ok(registry.catalog_snapshot())
}

pub(crate) async fn set_plugin_enabled_from(
    registry: &RwLock<PluginRegistry>,
    id: &str,
    enabled: bool,
    expected_catalog_generation: &str,
) -> AppResult<PluginCatalogMutationResult> {
    let mut registry = registry.write().await;
    registry.set_enabled(id, enabled, expected_catalog_generation)
}

#[tauri::command]
pub async fn get_plugin_catalog(state: State<'_, AppState>) -> AppResult<PluginCatalogSnapshot> {
    get_plugin_catalog_from(state.plugins.as_ref()).await
}

#[tauri::command]
pub async fn set_plugin_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
    expected_catalog_generation: String,
) -> AppResult<PluginCatalogMutationResult> {
    set_plugin_enabled_from(
        state.plugins.as_ref(),
        &id,
        enabled,
        &expected_catalog_generation,
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};

    use super::*;
    use crate::error::AppError;
    use crate::plugin::manifest::{PluginId, PluginManifestV1, PluginPublisherId, PluginSource};
    use crate::storage::plugin_state::{
        PluginStateEntryV2, PluginStateFileV2, PluginStateLoad, PluginStatePersistence,
    };

    #[derive(Clone)]
    struct MemoryPersistence(Arc<Mutex<MemoryState>>);

    struct MemoryState {
        persisted: PluginStateFileV2,
        load_error: bool,
        loads: usize,
        saves: Vec<PluginStateFileV2>,
    }

    impl MemoryPersistence {
        fn new(persisted: PluginStateFileV2) -> Self {
            Self(Arc::new(Mutex::new(MemoryState {
                persisted,
                load_error: false,
                loads: 0,
                saves: Vec::new(),
            })))
        }

        fn registry(&self, manifests: Vec<PluginManifestV1>) -> PluginRegistry {
            PluginRegistry::initialize(manifests, Box::new(self.clone()))
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
                requires_rewrite: false,
            })
        }

        fn save(&self, next: &PluginStateFileV2) -> AppResult<()> {
            let mut state = self.0.lock().unwrap();
            state.saves.push(next.clone());
            state.persisted = next.clone();
            Ok(())
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

    // Catches returning the stale unavailable snapshot without asking persistence again.
    #[tokio::test]
    async fn catalog_command_retries_unavailable_state_before_snapshotting() {
        let persistence = MemoryPersistence::new(PluginStateFileV2 {
            schema_version: 2,
            revision: 7,
            entries: vec![enabled_entry()],
        });
        persistence.0.lock().unwrap().load_error = true;
        let registry = RwLock::new(persistence.registry(vec![manifest()]));
        assert_eq!(
            serde_json::to_value(registry.read().await.catalog_snapshot()).unwrap()["availability"],
            "unavailable"
        );

        persistence.0.lock().unwrap().load_error = false;
        let snapshot = get_plugin_catalog_from(&registry).await.unwrap();

        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            json!({
                "schemaVersion": 2,
                "revision": "7",
                "catalogGeneration": "0",
                "availability": "available",
                "availabilityReasonCode": null,
                "localDiscovery": {"status": "available", "rejectedPackageCount": 0},
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
        let registry = RwLock::new(persistence.registry(vec![manifest()]));
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
        assert_eq!(registry.read().await.catalog_snapshot().revision, "0");
    }

    // Catches bypassing the registry transaction or discarding its committed revision/item.
    #[tokio::test]
    async fn mutation_command_returns_the_persisted_registry_result() {
        let persistence = MemoryPersistence::new(PluginStateFileV2::empty());
        let registry = RwLock::new(persistence.registry(vec![manifest()]));

        let result = set_plugin_enabled_from(&registry, "com.easiflux.alpha", true, "0")
            .await
            .unwrap();

        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["schemaVersion"], 2);
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
}
