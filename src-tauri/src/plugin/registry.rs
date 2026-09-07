use std::collections::BTreeMap;
use std::sync::Arc;

use crate::error::{AppError, AppResult};
use crate::storage::plugin_state::{
    PluginStateEntryV2, PluginStateFileV2, PluginStatePersistence, PluginStateStore,
};

use super::builtin::builtin_manifests;
use super::discovery::LocalDiscoveryOutcome;
use super::manifest::{
    LocalDiscoverySummary, PluginAvailability, PluginAvailabilityReason, PluginCatalogItem,
    PluginCatalogMutationResult, PluginCatalogSnapshot, PluginId, PluginManifestV1,
};
use super::record::PluginRecord;

enum Runtime {
    Available {
        state: PluginStateFileV2,
        requires_rewrite: bool,
        persistence: Arc<dyn PluginStatePersistence>,
    },
    Unavailable {
        reason: PluginAvailabilityReason,
        persistence: Arc<dyn PluginStatePersistence>,
    },
}

pub struct PluginRegistry {
    builtins: BTreeMap<PluginId, PluginRecord>,
    locals: BTreeMap<PluginId, PluginRecord>,
    local_summary: LocalDiscoverySummary,
    catalog_generation: u64,
    runtime: Runtime,
}

// Resolve the production store for each operation, so failure to resolve the
// configuration directory remains retryable rather than aborting AppState.
struct SystemPersistence;

impl PluginStatePersistence for SystemPersistence {
    fn load(&self) -> AppResult<crate::storage::plugin_state::PluginStateLoad> {
        PluginStateStore::try_new()?.load()
    }

    fn save(&self, state: &PluginStateFileV2) -> AppResult<()> {
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
            let record = match PluginRecord::built_in(manifest) {
                Ok(record) if !manifests.contains_key(&record.manifest().id) => record,
                _ => {
                    return Self {
                        builtins: BTreeMap::new(),
                        locals: BTreeMap::new(),
                        local_summary: LocalDiscoverySummary::available(),
                        catalog_generation: 0,
                        runtime: Runtime::Unavailable {
                            reason: PluginAvailabilityReason::CatalogInvalid,
                            persistence,
                        },
                    };
                }
            };
            let id = record.manifest().id.clone();
            manifests.insert(id, record);
        }
        let mut registry = Self {
            builtins: manifests,
            locals: BTreeMap::new(),
            local_summary: LocalDiscoverySummary::available(),
            catalog_generation: 0,
            runtime: Runtime::Unavailable {
                reason: PluginAvailabilityReason::StateUnavailable,
                persistence,
            },
        };
        registry.retry_state_load();
        registry
    }

    pub(crate) fn catalog_generation(&self) -> u64 {
        self.catalog_generation
    }

    // Task 5 wires this policy operation into the runtime's explicit discovery lifecycle.
    #[allow(dead_code)]
    pub(crate) fn apply_local_discovery(
        &mut self,
        outcome: LocalDiscoveryOutcome,
    ) -> AppResult<bool> {
        let mut locals = BTreeMap::new();
        let mut collisions = 0;
        for record in outcome.records {
            if self.builtins.contains_key(&record.manifest().id) {
                collisions += 1;
            } else {
                locals.insert(record.manifest().id.clone(), record);
            }
        }
        let summary = outcome.summary.with_additional_rejections(collisions);
        if self.catalog_generation != 0 && self.locals == locals && self.local_summary == summary {
            return Ok(false);
        }
        let generation = self.catalog_generation.checked_add(1).ok_or_else(|| {
            plugin_error(
                "plugin_catalog_generation_exhausted",
                "插件目录版本已达上限",
            )
        })?;
        self.locals = locals;
        self.local_summary = summary;
        self.catalog_generation = generation;
        Ok(true)
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
        let records: BTreeMap<_, _> = self.builtins.iter().chain(self.locals.iter()).collect();
        let plugins = records
            .into_values()
            .map(|record| match &self.runtime {
                Runtime::Available { state, .. } => catalog_item(record, is_enabled(state, record)),
                Runtime::Unavailable { reason, .. } => PluginCatalogItem::blocked(
                    record.manifest().clone(),
                    record.source(),
                    reason.clone(),
                ),
            })
            .collect();
        PluginCatalogSnapshot::new(
            revision.to_string(),
            self.catalog_generation().to_string(),
            availability,
            reason,
            self.local_summary.clone(),
            plugins,
        )
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
            Ok(loaded) if loaded.state.validate().is_ok() => {
                self.runtime = Runtime::Available {
                    state: loaded.state,
                    requires_rewrite: loaded.requires_rewrite,
                    persistence: Arc::clone(persistence),
                };
            }
            Ok(_) => tracing::warn!("invalid loaded plugin state"),
            Err(_) => tracing::warn!("plugin state load unavailable"),
        }
    }

    /// Synchronous clone -> persist -> commit transaction. The command layer must
    /// hold one `state.plugins.write().await` guard across this entire call.
    pub(crate) fn set_enabled(
        &mut self,
        id: &str,
        enabled: bool,
        expected_catalog_generation: &str,
    ) -> AppResult<PluginCatalogMutationResult> {
        if expected_catalog_generation.parse::<u64>().ok() != Some(self.catalog_generation)
            || expected_catalog_generation != self.catalog_generation.to_string()
        {
            return Err(plugin_error(
                "plugin_catalog_stale",
                "插件目录已更新，请刷新后重试",
            ));
        }
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
        let record = self
            .builtins
            .get(&id)
            .or_else(|| self.locals.get(&id))
            .ok_or_else(|| plugin_error("plugin_not_found", "插件不存在"))?;
        let Runtime::Available {
            state,
            requires_rewrite,
            persistence,
        } = &mut self.runtime
        else {
            return Err(plugin_error(
                "plugin_state_unavailable",
                "插件状态存储暂不可用",
            ));
        };
        // An explicit decision replaces all prior identities for this ID/source,
        // even when changed content is already effectively disabled.
        let identity = record.identity();
        let mut next = state.clone();
        sort_state_entries(&mut next.entries);
        let previous_entries = next.entries.clone();
        next.entries
            .retain(|entry| entry.id != identity.id || entry.source != identity.source);
        next.entries.push(PluginStateEntryV2 {
            id: identity.id,
            source: identity.source,
            publisher_id: identity.publisher_id,
            approval_fingerprint: identity.approval_fingerprint,
            enabled,
        });
        sort_state_entries(&mut next.entries);
        // Serialization order is not a logical decision. It may require a save,
        // like v1 migration, but must not consume a revision (even at u64::MAX).
        let decision_changed = next.entries != previous_entries;
        if next == *state && !*requires_rewrite {
            return Ok(PluginCatalogMutationResult::new(
                state.revision.to_string(),
                self.catalog_generation.to_string(),
                catalog_item(record, enabled),
            ));
        }
        if decision_changed {
            next.revision = state
                .revision
                .checked_add(1)
                .ok_or_else(|| plugin_error("plugin_revision_exhausted", "插件状态版本已达上限"))?;
        }
        next.validate_for_persistence()
            .map_err(|_| plugin_error("plugin_state_persist_failed", "插件状态保存失败"))?;
        persistence
            .save(&next)
            .map_err(|_| plugin_error("plugin_state_persist_failed", "插件状态保存失败"))?;
        *state = next;
        *requires_rewrite = false;
        Ok(PluginCatalogMutationResult::new(
            state.revision.to_string(),
            self.catalog_generation.to_string(),
            catalog_item(record, enabled),
        ))
    }
}

fn sort_state_entries(entries: &mut [PluginStateEntryV2]) {
    entries.sort_by(|a, b| {
        (&a.id, a.source, &a.publisher_id, &a.approval_fingerprint).cmp(&(
            &b.id,
            b.source,
            &b.publisher_id,
            &b.approval_fingerprint,
        ))
    });
}

fn matches_identity(entry: &PluginStateEntryV2, record: &PluginRecord) -> bool {
    let identity = record.identity();
    entry.id == identity.id
        && entry.source == identity.source
        && entry.publisher_id == identity.publisher_id
        && entry.approval_fingerprint == identity.approval_fingerprint
}

fn is_enabled(state: &PluginStateFileV2, record: &PluginRecord) -> bool {
    state
        .entries
        .iter()
        .any(|entry| matches_identity(entry, record) && entry.enabled)
}

fn catalog_item(record: &PluginRecord, enabled: bool) -> PluginCatalogItem {
    if enabled {
        PluginCatalogItem::enabled(record.manifest().clone(), record.source())
    } else {
        PluginCatalogItem::disabled(record.manifest().clone(), record.source())
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
