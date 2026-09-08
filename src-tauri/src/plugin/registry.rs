use std::collections::BTreeMap;
use std::sync::Arc;

use crate::error::{AppError, AppResult};
use crate::storage::plugin_state::{
    PluginStateEntryV2, PluginStateFileV2, PluginStatePersistence, PluginStateStore,
    MAX_PLUGIN_STATE_ENTRIES,
};

use super::builtin::builtin_manifests;
use super::discovery::LocalPackageLocator;
use super::discovery::{LocalDiscoveryOutcome, ScanUsage};
use super::import::ImportCommitFailure;
use super::manifest::{
    LocalDiscoveryStatus, LocalDiscoverySummary, PluginAvailability, PluginAvailabilityReason,
    PluginCatalogItem, PluginCatalogMutationResult, PluginCatalogSnapshot, PluginId,
    PluginManifestV1, PluginSource,
};
use super::manifest::{ManagedOwnershipSummary, PluginManagement};
use super::ownership::OwnershipRuntime;
use super::record::PluginRecord;
use crate::storage::managed_plugin_ownership::{
    ManagedOwnershipPersistence, ManagedOwnershipStore,
};

#[derive(Clone)]
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

#[derive(Clone)]
pub(crate) struct CatalogPublicationCandidate {
    locals: BTreeMap<PluginId, PluginRecord>,
    locators: BTreeMap<PluginId, LocalPackageLocator>,
    management: BTreeMap<PluginId, PluginManagement>,
    local_summary: LocalDiscoverySummary,
    ownership_summary: ManagedOwnershipSummary,
    discovery: LocalDiscoveryOutcome,
    catalog_generation: u64,
    runtime: Runtime,
    ownership: OwnershipRuntime,
    snapshot: PluginCatalogSnapshot,
}

pub struct PluginRegistry {
    builtins: BTreeMap<PluginId, PluginRecord>,
    publication: CatalogPublicationCandidate,
}

// Resolve the production store for each operation, so failure to resolve the
// configuration directory remains retryable rather than aborting AppState.
struct SystemPersistence;

impl PluginStatePersistence for SystemPersistence {
    fn load(&self) -> AppResult<crate::storage::plugin_state::PluginStateLoad> {
        PluginStateStore::try_new()?.load()
    }

    fn save(
        &self,
        state: &PluginStateFileV2,
    ) -> crate::storage::safe_plugin_document::PersistResult {
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
        Self::initialize(
            builtin_manifests(),
            Box::new(SystemPersistence),
            Box::new(ManagedOwnershipStore::new()),
        )
    }

    /// Each persistence subsystem fails independently, without aborting startup.
    pub(crate) fn initialize(
        builtins: Vec<PluginManifestV1>,
        persistence: Box<dyn PluginStatePersistence>,
        ownership_persistence: Box<dyn ManagedOwnershipPersistence>,
    ) -> Self {
        let persistence: Arc<dyn PluginStatePersistence> = persistence.into();
        let ownership = OwnershipRuntime::initialize(ownership_persistence.into());
        let mut manifests = BTreeMap::new();
        let mut invalid = false;
        for manifest in builtins {
            match PluginRecord::built_in(manifest) {
                Ok(record) if !manifests.contains_key(&record.manifest().id) => {
                    manifests.insert(record.manifest().id.clone(), record);
                }
                _ => {
                    invalid = true;
                    manifests.clear();
                    break;
                }
            }
        }
        let runtime = if invalid {
            Runtime::Unavailable {
                reason: PluginAvailabilityReason::CatalogInvalid,
                persistence,
            }
        } else {
            match persistence.load() {
                Ok(loaded) if loaded.state.validate().is_ok() => Runtime::Available {
                    state: loaded.state,
                    requires_rewrite: loaded.requires_rewrite,
                    persistence,
                },
                _ => Runtime::Unavailable {
                    reason: PluginAvailabilityReason::StateUnavailable,
                    persistence,
                },
            }
        };
        let ownership_summary = if ownership.requires_retry() {
            ManagedOwnershipSummary::unavailable()
        } else {
            ManagedOwnershipSummary::available()
        };
        let snapshot = PluginCatalogSnapshot::new(
            "0".into(),
            "0".into(),
            PluginAvailability::Available,
            None,
            LocalDiscoverySummary::available(),
            ownership_summary.clone(),
            vec![],
        );
        let mut registry = Self {
            builtins: manifests,
            publication: CatalogPublicationCandidate {
                locals: BTreeMap::new(),
                locators: BTreeMap::new(),
                management: BTreeMap::new(),
                local_summary: LocalDiscoverySummary::available(),
                ownership_summary,
                discovery: LocalDiscoveryOutcome {
                    plugins: vec![],
                    occupied_slots: vec![],
                    removals: super::discovery::RemovalDiscoveryOutcome::available(),
                    summary: LocalDiscoverySummary::available(),
                    usage: None,
                },
                catalog_generation: 0,
                runtime,
                ownership,
                snapshot,
            },
        };
        registry.publication.snapshot = registry.derive_snapshot(&registry.publication);
        registry
    }

    pub(crate) fn catalog_generation(&self) -> u64 {
        self.publication.catalog_generation
    }

    #[cfg(test)]
    pub(crate) fn set_catalog_generation_for_test(&mut self, generation: u64) {
        self.publication.catalog_generation = generation;
        self.publication.snapshot.catalog_generation = generation.to_string();
    }

    pub(crate) fn state_requires_retry(&self) -> bool {
        matches!(
            self.publication.runtime,
            Runtime::Unavailable {
                reason: PluginAvailabilityReason::StateUnavailable,
                ..
            }
        )
    }

    pub(crate) fn validate_import(
        &self,
        record: &PluginRecord,
        usage: ScanUsage,
    ) -> Result<(), ImportCommitFailure> {
        match &self.publication.runtime {
            Runtime::Unavailable {
                reason: PluginAvailabilityReason::CatalogInvalid,
                ..
            } => {
                return Err(ImportCommitFailure::CatalogInvalid);
            }
            Runtime::Unavailable { .. } => return Err(ImportCommitFailure::StateUnavailable),
            Runtime::Available { .. } => {}
        }
        if self.publication.local_summary.status != LocalDiscoveryStatus::Available {
            return Err(ImportCommitFailure::DiscoveryUnavailable);
        }
        if self.builtins.contains_key(&record.manifest().id)
            || self.publication.locals.contains_key(&record.manifest().id)
        {
            return Err(ImportCommitFailure::IdConflict);
        }
        let bytes = record
            .canonical_manifest_bytes()
            .map_err(|_| ImportCommitFailure::CapacityExceeded)?;
        if !usage.can_add_manifest(bytes.len()) {
            return Err(ImportCommitFailure::CapacityExceeded);
        }
        if self.publication.catalog_generation == u64::MAX {
            return Err(ImportCommitFailure::CatalogGenerationExhausted);
        }
        Ok(())
    }

    /// Record only the disabled decision; authoritative discovery owns membership.
    pub(crate) fn persist_import_disabled(&mut self, record: &PluginRecord) -> AppResult<()> {
        if record.source() != PluginSource::LocalDeclarative
            || matches!(
                self.publication.runtime,
                Runtime::Unavailable {
                    reason: PluginAvailabilityReason::CatalogInvalid,
                    ..
                }
            )
        {
            return Err(plugin_error("plugin_catalog_invalid", "插件目录无效"));
        }
        self.persist_decision(record, false)
    }

    pub(crate) fn apply_local_discovery(
        &mut self,
        outcome: LocalDiscoveryOutcome,
    ) -> AppResult<bool> {
        let mut next = self.publication.clone();
        let (ownership, management, summary) = next
            .ownership
            .reconcile(&outcome, next.catalog_generation != u64::MAX);
        next.ownership = ownership;
        next.management = management;
        next.ownership_summary = summary;
        next.locals.clear();
        next.locators.clear();
        let mut collisions = 0;
        for package in &outcome.plugins {
            if self.builtins.contains_key(&package.record.manifest().id) {
                collisions += 1;
            } else {
                next.locals
                    .insert(package.record.manifest().id.clone(), package.record.clone());
                next.locators.insert(
                    package.record.manifest().id.clone(),
                    package.locator.clone(),
                );
            }
        }
        next.local_summary = outcome
            .summary
            .clone()
            .with_additional_rejections(collisions);
        next.discovery = outcome;
        let state_retry = Self::load_state_if_needed(&mut next);
        let published = self.publish_candidate(next, true)?;
        state_retry?;
        Ok(published)
    }

    pub(crate) fn ownership_requires_retry(&self) -> bool {
        self.publication.ownership.requires_retry()
    }

    /// Compare the complete semantic basis before assigning a checked generation.
    /// No intermediate ownership/state/index classification can be observed.
    fn publish_candidate(
        &mut self,
        mut next: CatalogPublicationCandidate,
        first_scan: bool,
    ) -> AppResult<bool> {
        next.snapshot = self.derive_snapshot(&next);
        let old = &self.publication;
        let structural = next.locals != old.locals
            || next.locators != old.locators
            || next.management != old.management
            || next.local_summary != old.local_summary
            || next.ownership_summary != old.ownership_summary
            || next.ownership.entries() != old.ownership.entries()
            || next.discovery.occupied_slots != old.discovery.occupied_slots
            || next.discovery.removals.observations != old.discovery.removals.observations
            || next.discovery.removals.occupied_slots != old.discovery.removals.occupied_slots
            || next.snapshot.availability != old.snapshot.availability
            || next.snapshot.availability_reason_code != old.snapshot.availability_reason_code
            || next.snapshot.plugins.len() != old.snapshot.plugins.len()
            || next
                .snapshot
                .plugins
                .iter()
                .zip(&old.snapshot.plugins)
                .any(|(next, old)| !next.same_structure(old))
            || (next.snapshot.revision == old.snapshot.revision && next.snapshot != old.snapshot)
            || (first_scan && old.catalog_generation == 0);
        if structural {
            next.catalog_generation = old.catalog_generation.checked_add(1).ok_or_else(|| {
                plugin_error(
                    "plugin_catalog_generation_exhausted",
                    "插件目录版本已达上限",
                )
            })?;
            next.snapshot.catalog_generation = next.catalog_generation.to_string();
        }
        let changed = next.snapshot != old.snapshot;
        self.publication = next;
        Ok(changed)
    }

    pub fn catalog_snapshot(&self) -> PluginCatalogSnapshot {
        self.publication.snapshot.clone()
    }

    fn derive_snapshot(&self, next: &CatalogPublicationCandidate) -> PluginCatalogSnapshot {
        let (revision, availability, reason) = match &next.runtime {
            Runtime::Available { state, .. } => {
                (state.revision, PluginAvailability::Available, None)
            }
            Runtime::Unavailable { reason, .. } => {
                (0, PluginAvailability::Unavailable, Some(reason.clone()))
            }
        };
        let records: BTreeMap<_, _> = self.builtins.iter().chain(next.locals.iter()).collect();
        let plugins = records
            .into_iter()
            .map(|(id, record)| {
                let item = match &next.runtime {
                    Runtime::Available { state, .. } => {
                        catalog_item(record, is_enabled(state, record))
                    }
                    Runtime::Unavailable { reason, .. } => PluginCatalogItem::blocked(
                        record.manifest().clone(),
                        record.source(),
                        reason.clone(),
                    ),
                };
                item.with_management(if record.source() == PluginSource::BuiltIn {
                    PluginManagement::BuiltIn
                } else {
                    next.management
                        .get(id)
                        .copied()
                        .unwrap_or(PluginManagement::External)
                })
            })
            .collect();
        PluginCatalogSnapshot::new(
            revision.to_string(),
            next.catalog_generation.to_string(),
            availability,
            reason,
            next.local_summary.clone(),
            next.ownership_summary.clone(),
            plugins,
        )
    }

    fn load_state_if_needed(next: &mut CatalogPublicationCandidate) -> AppResult<()> {
        let Runtime::Unavailable {
            reason: PluginAvailabilityReason::StateUnavailable,
            persistence,
        } = &next.runtime
        else {
            return Ok(());
        };
        let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| persistence.load()))
            .map_err(|_| AppError::Internal("插件目录请求暂不可用".into()))?;
        if let Ok(loaded) = loaded {
            if loaded.state.validate().is_ok() {
                next.runtime = Runtime::Available {
                    state: loaded.state,
                    requires_rewrite: loaded.requires_rewrite,
                    persistence: Arc::clone(persistence),
                };
            }
        }
        Ok(())
    }

    /// Retry cannot publish a different DTO with the same revision/generation pair.
    pub fn retry_state_load(&mut self) {
        let mut next = self.publication.clone();
        if Self::load_state_if_needed(&mut next).is_err() {
            return;
        }
        if self.publish_candidate(next, false).is_err() {
            tracing::warn!("plugin catalog generation exhausted during state recovery");
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
        if expected_catalog_generation.parse::<u64>().ok()
            != Some(self.publication.catalog_generation)
            || expected_catalog_generation != self.publication.catalog_generation.to_string()
        {
            return Err(plugin_error(
                "plugin_catalog_stale",
                "插件目录已更新，请刷新后重试",
            ));
        }
        let id =
            PluginId::parse(id).map_err(|_| plugin_error("plugin_invalid_id", "插件标识无效"))?;
        if matches!(
            self.publication.runtime,
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
            .or_else(|| self.publication.locals.get(&id))
            .cloned()
            .ok_or_else(|| plugin_error("plugin_not_found", "插件不存在"))?;
        if self.publication.management.get(&id) == Some(&PluginManagement::RemovalPending) {
            return Err(plugin_error("plugin_removal_pending", "插件移除回退待处理"));
        }
        self.persist_decision(&record, enabled)?;
        let Runtime::Available { state, .. } = &self.publication.runtime else {
            unreachable!("a successful decision requires available state");
        };
        Ok(PluginCatalogMutationResult::new(
            state.revision.to_string(),
            self.publication.catalog_generation.to_string(),
            catalog_item(&record, enabled).with_management(
                self.publication
                    .management
                    .get(&id)
                    .copied()
                    .unwrap_or(PluginManagement::BuiltIn),
            ),
        ))
    }

    fn persist_decision(&mut self, record: &PluginRecord, enabled: bool) -> AppResult<()> {
        let mut candidate = self.publication.clone();
        let Runtime::Available {
            state,
            requires_rewrite,
            persistence,
        } = &mut candidate.runtime
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
        // Capacity is a distinct transaction outcome and must not be masked by
        // revision exhaustion or flattened into a persistence failure.
        if next.entries.len() > MAX_PLUGIN_STATE_ENTRIES {
            return Err(plugin_error(
                "plugin_state_capacity_exceeded",
                "插件状态容量已达上限",
            ));
        }
        sort_state_entries(&mut next.entries);
        // Serialization order is not a logical decision. It may require a save,
        // like v1 migration, but must not consume a revision (even at u64::MAX).
        let decision_changed = next.entries != previous_entries;
        if next == *state && !*requires_rewrite {
            return Ok(());
        }
        if decision_changed {
            next.revision = state
                .revision
                .checked_add(1)
                .ok_or_else(|| plugin_error("plugin_revision_exhausted", "插件状态版本已达上限"))?;
        }
        next.validate_for_persistence()
            .map_err(|_| plugin_error("plugin_state_persist_failed", "插件状态保存失败"))?;
        let result = persistence.save(&next);
        let outcome = match &result {
            Ok(outcome) => *outcome,
            Err(failure) => failure.outcome,
        };
        if crate::storage::safe_plugin_document::persist_outcome_committed(outcome) {
            *state = next;
            *requires_rewrite = false;
            candidate.snapshot = self.derive_snapshot(&candidate);
            self.publication = candidate;
        }
        match result {
            Ok(outcome)
                if crate::storage::safe_plugin_document::persist_outcome_committed(outcome) =>
            {
                Ok(())
            }
            _ => Err(plugin_error(
                "plugin_state_persist_failed",
                "插件状态保存失败",
            )),
        }
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
