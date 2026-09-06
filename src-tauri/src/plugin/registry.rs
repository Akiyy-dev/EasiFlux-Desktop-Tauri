use std::collections::BTreeMap;
use std::sync::Arc;

use crate::error::{AppError, AppResult};
use crate::storage::plugin_state::{
    PluginStateEntryV1, PluginStateFileV1, PluginStatePersistence, PluginStateStore,
};

use super::builtin::builtin_manifests;
use super::manifest::{
    PluginAvailability, PluginAvailabilityReason, PluginCatalogItem, PluginCatalogMutationResult,
    PluginCatalogSnapshot, PluginId, PluginManifestV1, PluginSource, APPROVAL_FINGERPRINT_NONE,
};

enum Runtime {
    Available {
        state: PluginStateFileV1,
        persistence: Arc<dyn PluginStatePersistence>,
    },
    Unavailable {
        reason: PluginAvailabilityReason,
        persistence: Arc<dyn PluginStatePersistence>,
    },
}

pub struct PluginRegistry {
    manifests: BTreeMap<PluginId, PluginManifestV1>,
    runtime: Runtime,
}

// Resolve the production store for each operation, so failure to resolve the
// configuration directory remains retryable rather than aborting AppState.
struct SystemPersistence;

impl PluginStatePersistence for SystemPersistence {
    fn load(&self) -> AppResult<PluginStateFileV1> {
        PluginStateStore::try_new()?.load()
    }

    fn save(&self, state: &PluginStateFileV1) -> AppResult<()> {
        PluginStateStore::try_new()?.save(state)
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::initialize(builtin_manifests(), Box::new(SystemPersistence))
    }

    /// Plugin failures are represented in the snapshot, never returned to startup.
    pub(crate) fn initialize(
        builtins: Vec<PluginManifestV1>,
        persistence: Box<dyn PluginStatePersistence>,
    ) -> Self {
        let persistence: Arc<dyn PluginStatePersistence> = persistence.into();
        let mut manifests = BTreeMap::new();
        for manifest in builtins {
            if manifest.validate().is_err() || manifests.contains_key(&manifest.id) {
                return Self {
                    manifests: BTreeMap::new(),
                    runtime: Runtime::Unavailable {
                        reason: PluginAvailabilityReason::CatalogInvalid,
                        persistence,
                    },
                };
            }
            manifests.insert(manifest.id.clone(), manifest);
        }
        let mut registry = Self {
            manifests,
            runtime: Runtime::Unavailable {
                reason: PluginAvailabilityReason::StateUnavailable,
                persistence,
            },
        };
        registry.retry_state_load();
        registry
    }

    pub fn catalog_snapshot(&self) -> PluginCatalogSnapshot {
        let (revision, availability, reason) = match &self.runtime {
            Runtime::Available { state, .. } => {
                (state.revision, PluginAvailability::Available, None)
            }
            Runtime::Unavailable { reason, .. } => {
                (0, PluginAvailability::Unavailable, Some(reason.clone()))
            }
        };
        let plugins = self
            .manifests
            .values()
            .map(|manifest| match &self.runtime {
                Runtime::Available { state, .. } => {
                    catalog_item(manifest, is_enabled(state, manifest))
                }
                Runtime::Unavailable { reason, .. } => PluginCatalogItem::blocked(
                    manifest.clone(),
                    PluginSource::BuiltIn,
                    reason.clone(),
                ),
            })
            .collect();
        PluginCatalogSnapshot::new(revision, availability, reason, plugins)
    }

    /// Call only while holding the registry's write guard, never through a read guard.
    pub fn retry_state_load(&mut self) {
        let Runtime::Unavailable {
            reason: PluginAvailabilityReason::StateUnavailable,
            persistence,
        } = &self.runtime
        else {
            return;
        };
        match persistence.load() {
            Ok(state) if state.validate().is_ok() => {
                self.runtime = Runtime::Available {
                    state,
                    persistence: Arc::clone(persistence),
                };
            }
            Ok(_) => tracing::warn!("invalid loaded plugin state"),
            Err(_) => tracing::warn!("plugin state load unavailable"),
        }
    }

    /// Synchronous clone -> persist -> commit transaction. The command layer must
    /// hold one `state.plugins.write().await` guard across this entire call.
    pub fn set_enabled(
        &mut self,
        id: &str,
        enabled: bool,
    ) -> AppResult<PluginCatalogMutationResult> {
        let id =
            PluginId::parse(id).map_err(|_| plugin_error("plugin_invalid_id", "插件标识无效"))?;
        if matches!(
            self.runtime,
            Runtime::Unavailable {
                reason: PluginAvailabilityReason::CatalogInvalid,
                ..
            }
        ) {
            return Err(plugin_error("plugin_catalog_invalid", "插件目录无效"));
        }
        let manifest = self
            .manifests
            .get(&id)
            .ok_or_else(|| plugin_error("plugin_not_found", "插件不存在"))?;
        let Runtime::Available { state, persistence } = &mut self.runtime else {
            return Err(plugin_error(
                "plugin_state_unavailable",
                "插件状态存储暂不可用",
            ));
        };
        // Every valid phase-0 built-in is toggleable; unavailable items were
        // rejected above. No executable activation or capability grants occur.
        if is_enabled(state, manifest) == enabled {
            return Ok(PluginCatalogMutationResult::new(
                state.revision,
                catalog_item(manifest, enabled),
            ));
        }
        let mut next = state.clone();
        next.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| plugin_error("plugin_revision_exhausted", "插件状态版本已达上限"))?;
        if let Some(entry) = next
            .entries
            .iter_mut()
            .find(|entry| matches_identity(entry, manifest))
        {
            entry.enabled = enabled;
        } else {
            next.entries.push(PluginStateEntryV1 {
                id: manifest.id.clone(),
                source: PluginSource::BuiltIn,
                publisher_id: manifest.publisher_id.clone(),
                approval_fingerprint: APPROVAL_FINGERPRINT_NONE.into(),
                enabled,
            });
        }
        persistence
            .save(&next)
            .map_err(|_| plugin_error("plugin_state_persist_failed", "插件状态保存失败"))?;
        *state = next;
        Ok(PluginCatalogMutationResult::new(
            state.revision,
            catalog_item(manifest, enabled),
        ))
    }
}

fn matches_identity(entry: &PluginStateEntryV1, manifest: &PluginManifestV1) -> bool {
    entry.id == manifest.id
        && entry.source == PluginSource::BuiltIn
        && entry.publisher_id == manifest.publisher_id
        && entry.approval_fingerprint == APPROVAL_FINGERPRINT_NONE
}

fn is_enabled(state: &PluginStateFileV1, manifest: &PluginManifestV1) -> bool {
    state
        .entries
        .iter()
        .any(|entry| matches_identity(entry, manifest) && entry.enabled)
}

fn catalog_item(manifest: &PluginManifestV1, enabled: bool) -> PluginCatalogItem {
    if enabled {
        PluginCatalogItem::enabled(manifest.clone(), PluginSource::BuiltIn)
    } else {
        PluginCatalogItem::disabled(manifest.clone(), PluginSource::BuiltIn)
    }
}

fn plugin_error(code: &'static str, message: &'static str) -> AppError {
    AppError::Plugin {
        code,
        message,
        diagnostic: None,
    }
}

#[cfg(test)]
mod tests;
