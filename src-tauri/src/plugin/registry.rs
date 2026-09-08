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
use super::ownership::RemovalSlot;
use super::ownership::{
    FileIdentity, ManagedOwnershipEntryV1, ManagedOwnershipIndexV1, OwnershipFailure,
    OwnershipReceiptV1, OwnershipRuntime, PackageSlot, ReceiptId, VerifiedPackageReceipt,
    MAX_MANAGED_OWNERSHIP_BYTES, MAX_MANAGED_OWNERSHIP_ENTRIES,
};
use super::record::PluginRecord;
use super::removal::BeforeDisabledFailure;
use crate::storage::managed_plugin_ownership::{
    ManagedOwnershipPersistence, ManagedOwnershipStore,
};
use crate::storage::safe_plugin_document::{persist_outcome_committed, PersistOutcome};
use crate::storage::safe_plugin_document::{PersistFailure, PersistResult};

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

#[derive(Clone)]
pub struct PluginRegistry {
    builtins: BTreeMap<PluginId, PluginRecord>,
    publication: CatalogPublicationCandidate,
    // Presentation may remain frozen after an uncertain remove, but it cannot
    // authorize another mutation against newer adopted lifecycle documents.
    removal_authority_unreconciled: bool,
}

#[derive(Debug)]
pub(crate) struct OwnershipMutationFailure {
    pub(crate) failure: OwnershipFailure,
    pub(crate) persist_outcome: PersistOutcome,
}

#[derive(Debug)]
pub(crate) struct StateDecisionFailure {
    pub(crate) error: AppError,
    pub(crate) persist_outcome: PersistOutcome,
}

pub(crate) struct RemovalPreflight {
    pub(crate) record: PluginRecord,
    pub(crate) locator: LocalPackageLocator,
    pub(crate) entry: ManagedOwnershipEntryV1,
}

impl From<AppError> for StateDecisionFailure {
    fn from(error: AppError) -> Self {
        Self {
            error,
            persist_outcome: PersistOutcome::NotCommitted,
        }
    }
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
            removal_authority_unreconciled: false,
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

    /// Pure admission against the complete current publication. No recovery,
    /// discovery, persistence, root access, or random slot allocation is allowed.
    pub(crate) fn preflight_removal(
        &self,
        id: &PluginId,
        expected_generation: &str,
    ) -> Result<RemovalPreflight, BeforeDisabledFailure> {
        self.preflight_removal_with_limits(
            id,
            expected_generation,
            #[cfg(test)]
            crate::storage::plugin_state::MAX_PLUGIN_STATE_BYTES,
            #[cfg(test)]
            MAX_MANAGED_OWNERSHIP_BYTES,
        )
    }

    fn preflight_removal_with_limits(
        &self,
        id: &PluginId,
        expected_generation: &str,
        #[cfg(test)] state_byte_limit: usize,
        #[cfg(test)] ownership_byte_limit: usize,
    ) -> Result<RemovalPreflight, BeforeDisabledFailure> {
        #[cfg(not(test))]
        let state_byte_limit = crate::storage::plugin_state::MAX_PLUGIN_STATE_BYTES;
        use BeforeDisabledFailure as Failure;
        let published = &self.publication;
        if self.removal_authority_unreconciled
            || expected_generation != published.catalog_generation.to_string()
        {
            return Err(Failure::CatalogStale);
        }
        let state = match &published.runtime {
            Runtime::Available { state, .. } => state,
            Runtime::Unavailable {
                reason: PluginAvailabilityReason::CatalogInvalid,
                ..
            } => return Err(Failure::CatalogInvalid),
            _ => return Err(Failure::StateUnavailable),
        };
        if published.catalog_generation == 0
            || published.discovery.summary.status == LocalDiscoveryStatus::Unavailable
            || published.discovery.removals.status == LocalDiscoveryStatus::Unavailable
        {
            return Err(Failure::DiscoveryUnavailable);
        }
        let OwnershipRuntime::Available {
            index,
            requires_rewrite: false,
            ..
        } = &published.ownership
        else {
            return Err(Failure::OwnershipUnavailable);
        };
        if self.builtins.contains_key(id) {
            return Err(Failure::NotManaged);
        }
        let record = published.locals.get(id).ok_or(Failure::NotManaged)?;
        match Self::management_for_record(published, record) {
            PluginManagement::Managed => (),
            PluginManagement::OwnershipConflict => return Err(Failure::OwnershipConflict),
            PluginManagement::OwnershipUnavailable => return Err(Failure::OwnershipUnavailable),
            _ => return Err(Failure::NotManaged),
        }
        let packages: Vec<_> = published
            .discovery
            .plugins
            .iter()
            .filter(|package| &package.record.manifest().id == id)
            .collect();
        if packages.len() != 1 {
            return Err(Failure::OwnershipConflict);
        }
        let package = packages[0];
        let entry = index
            .entries()
            .iter()
            .find(|entry| entry.proves_managed(package))
            .ok_or(Failure::OwnershipConflict)?;
        if is_enabled(state, record) {
            return Err(Failure::RequiresDisabled);
        }

        let next_state = removal_disabled_candidate(state, record);
        if next_state.entries.len() > MAX_PLUGIN_STATE_ENTRIES
            || serde_json::to_vec(&next_state)
                .map_err(|_| Failure::StateCapacityExceeded)?
                .len()
                > state_byte_limit
        {
            return Err(Failure::StateCapacityExceeded);
        }
        index
            .preflight_removal_capacity(
                entry.receipt_id(),
                #[cfg(test)]
                ownership_byte_limit,
            )
            .map_err(|_| Failure::OwnershipCapacityExceeded)?;
        published
            .catalog_generation
            .checked_add(1)
            .ok_or(Failure::CatalogGenerationExhausted)?;
        state
            .revision
            .checked_add(1)
            .ok_or(Failure::RevisionExhausted)?;
        index
            .require_revision_headroom(2)
            .map_err(|_| Failure::OwnershipRevisionExhausted)?;
        Ok(RemovalPreflight {
            record: record.clone(),
            locator: package.locator.clone(),
            entry: entry.clone(),
        })
    }

    /// Called on the task-owned document candidate, never under a registry guard.
    /// Even an already-canonical disabled identity must cross this save barrier.
    pub(crate) fn persist_disabled_before_removal(
        &mut self,
        record: &PluginRecord,
    ) -> PersistResult {
        let Runtime::Available {
            state,
            requires_rewrite,
            persistence,
        } = &mut self.publication.runtime
        else {
            return Err(PersistFailure {
                outcome: PersistOutcome::NotCommitted,
            });
        };
        let next = removal_disabled_candidate(state, record);
        let result = persistence.save_before_destructive_rename(&next);
        if persist_outcome_committed(persistence_outcome(&result)) {
            *state = next;
            *requires_rewrite = false;
        }
        result
    }

    pub(crate) fn persist_removing(
        &mut self,
        receipt: &ReceiptId,
        slot: &RemovalSlot,
    ) -> PersistResult {
        self.mutate_removal_index(|index| index.begin_removal(receipt, slot.clone()))
    }

    pub(crate) fn rollback_removing(&mut self, receipt: &ReceiptId) -> PersistResult {
        self.mutate_removal_index(|index| index.restore_managed(receipt))
    }

    pub(crate) fn delete_removed_entry(&mut self, receipt: &ReceiptId) -> PersistResult {
        self.mutate_removal_index(|index| index.remove(receipt))
    }

    fn mutate_removal_index(
        &mut self,
        mutation: impl FnOnce(
            &ManagedOwnershipIndexV1,
        ) -> Result<ManagedOwnershipIndexV1, OwnershipFailure>,
    ) -> PersistResult {
        let OwnershipRuntime::Available {
            index,
            requires_rewrite,
            persistence,
        } = &mut self.publication.ownership
        else {
            return Err(PersistFailure {
                outcome: PersistOutcome::NotCommitted,
            });
        };
        let next = mutation(index).map_err(|_| PersistFailure {
            outcome: PersistOutcome::NotCommitted,
        })?;
        let result = persistence.save(&next);
        if persist_outcome_committed(persistence_outcome(&result)) {
            *index = next;
            *requires_rewrite = false;
        }
        result
    }

    pub(crate) fn removal_discovery_baseline(&self) -> LocalDiscoveryOutcome {
        self.publication.discovery.clone()
    }

    /// The only final reconciliation used by an owned remove: deterministic,
    /// no-load/no-write, and not published until the complete result is known.
    pub(crate) fn build_reconciled_candidate(&mut self, outcome: LocalDiscoveryOutcome) {
        let next = &mut self.publication;
        let (ownership, management, summary) = next.ownership.reconcile_loaded_without_io(&outcome);
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
        next.management.retain(|id, _| next.locals.contains_key(id));
        next.local_summary = outcome
            .summary
            .clone()
            .with_additional_rejections(collisions);
        next.discovery = outcome;
        self.publication.snapshot = self.derive_snapshot(&self.publication);
    }

    /// Adopt documents immediately, but compare publication against the frozen
    /// complete baseline, not against those already-adopted document revisions.
    pub(crate) fn adopt_removal_documents(&mut self, candidate: &Self) {
        self.removal_authority_unreconciled |=
            self.publication.ownership.entries() != candidate.publication.ownership.entries();
        self.publication.runtime = candidate.publication.runtime.clone();
        self.publication.ownership = candidate.publication.ownership.clone();
    }

    pub(crate) fn publish_candidate_once(
        &mut self,
        candidate: Self,
        baseline: &Self,
    ) -> AppResult<PluginCatalogSnapshot> {
        let mut publisher = baseline.clone();
        publisher.publish_candidate(candidate.publication, false)?;
        *self = publisher;
        Ok(self.catalog_snapshot())
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
        if self.removal_authority_unreconciled {
            return Err(ImportCommitFailure::CatalogStale);
        }
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

    /// Pure preflight: reserve the complete two-file package and both documents
    /// before storage is allowed to create even the staging parent.
    pub(crate) fn validate_managed_import(
        &self,
        record: &PluginRecord,
        usage: ScanUsage,
        expected_generation: &str,
    ) -> Result<(), ImportCommitFailure> {
        if expected_generation != self.catalog_generation().to_string() {
            return Err(ImportCommitFailure::CatalogStale);
        }
        self.validate_import(record, usage)?;
        let OwnershipRuntime::Available {
            index,
            requires_rewrite: false,
            ..
        } = &self.publication.ownership
        else {
            return Err(ImportCommitFailure::OwnershipUnavailable);
        };
        if self.publication.ownership_summary.status == LocalDiscoveryStatus::Unavailable {
            return Err(ImportCommitFailure::OwnershipUnavailable);
        }
        if index.entries().len() >= MAX_MANAGED_OWNERSHIP_ENTRIES {
            return Err(ImportCommitFailure::OwnershipCapacityExceeded);
        }
        index
            .require_revision_headroom(1)
            .map_err(|_| ImportCommitFailure::OwnershipRevisionExhausted)?;
        // Slot/UUID/OS identities all have fixed serialized widths. This sizing
        // receipt is never staged or registered and contains no deletion authority.
        let receipt = OwnershipReceiptV1::new(
            ReceiptId::parse("550e8400e29b41d4a716446655440000").unwrap(),
            PackageSlot::parse("pkg-00000000000000000000000000000000").unwrap(),
            record,
        )
        .map_err(|_| ImportCommitFailure::CatalogInvalid)?;
        let receipt_bytes = receipt
            .canonical_bytes()
            .map_err(|_| ImportCommitFailure::CapacityExceeded)?;
        let manifest_bytes = record
            .canonical_manifest_bytes()
            .map_err(|_| ImportCommitFailure::CapacityExceeded)?;
        if usage
            .bytes_read
            .checked_add(manifest_bytes.len())
            .and_then(|bytes| bytes.checked_add(receipt_bytes.len()))
            .is_none_or(|bytes| bytes > 2_097_152)
        {
            return Err(ImportCommitFailure::CapacityExceeded);
        }
        let sizing_entry = ManagedOwnershipEntryV1::managed(
            VerifiedPackageReceipt {
                canonical_sha256: receipt.canonical_sha256(),
                model: receipt,
                file_identity: FileIdentity {
                    volume: 0,
                    object: 3,
                },
            },
            record,
            FileIdentity {
                volume: 0,
                object: 1,
            },
            FileIdentity {
                volume: 0,
                object: 2,
            },
        )
        .map_err(|_| ImportCommitFailure::CatalogInvalid)?;
        let empty = ManagedOwnershipIndexV1::empty();
        let added_bytes = empty
            .register(sizing_entry)
            .and_then(|index| index.canonical_bytes())
            .map_err(|_| ImportCommitFailure::OwnershipCapacityExceeded)?
            .len()
            - empty.canonical_bytes().unwrap().len();
        let current_bytes = index
            .canonical_bytes()
            .map_err(|_| ImportCommitFailure::OwnershipCapacityExceeded)?
            .len();
        // One optional entry separator and one checked revision digit of reserve.
        if current_bytes + added_bytes + usize::from(!index.entries().is_empty()) + 1
            > MAX_MANAGED_OWNERSHIP_BYTES
        {
            return Err(ImportCommitFailure::OwnershipCapacityExceeded);
        }
        let Runtime::Available { state, .. } = &self.publication.runtime else {
            unreachable!()
        };
        let identity = record.identity();
        let mut next = state.clone();
        next.entries
            .retain(|entry| entry.id != identity.id || entry.source != identity.source);
        next.entries.push(PluginStateEntryV2 {
            id: identity.id,
            source: identity.source,
            publisher_id: identity.publisher_id,
            approval_fingerprint: identity.approval_fingerprint,
            enabled: false,
        });
        if next.entries.len() > MAX_PLUGIN_STATE_ENTRIES {
            return Err(ImportCommitFailure::StateCapacityExceeded);
        }
        sort_state_entries(&mut next.entries);
        let mut previous_entries = state.entries.clone();
        sort_state_entries(&mut previous_entries);
        if next.entries != previous_entries {
            next.revision = state
                .revision
                .checked_add(1)
                .ok_or(ImportCommitFailure::RevisionExhausted)?;
        }
        next.validate_for_persistence()
            .map_err(|_| ImportCommitFailure::StatePersistFailed)?;
        Ok(())
    }

    /// Register only proven promotion evidence. This adopts committed document
    /// outcomes, including errors, without inventing any catalog member.
    pub(crate) fn register_managed_import(
        &mut self,
        entry: ManagedOwnershipEntryV1,
    ) -> Result<PersistOutcome, OwnershipMutationFailure> {
        let failure = |failure| OwnershipMutationFailure {
            failure,
            persist_outcome: PersistOutcome::NotCommitted,
        };
        let OwnershipRuntime::Available {
            index,
            requires_rewrite: false,
            persistence,
        } = &mut self.publication.ownership
        else {
            return Err(failure(OwnershipFailure::Unavailable));
        };
        let next = index.register(entry).map_err(failure)?;
        let result = persistence.save(&next);
        let outcome = match &result {
            Ok(outcome) => *outcome,
            Err(failure) => failure.outcome,
        };
        if persist_outcome_committed(outcome) {
            *index = next;
        }
        match result {
            Ok(outcome) if persist_outcome_committed(outcome) => Ok(outcome),
            _ => Err(OwnershipMutationFailure {
                failure: OwnershipFailure::PersistFailed,
                persist_outcome: outcome,
            }),
        }
    }

    /// Persisted documents survive a failed/uncertain publication. The previous
    /// complete DTO remains authoritative until a complete candidate can replace it.
    pub(crate) fn adopt_import_documents(&mut self, candidate: &Self) {
        self.publication.runtime = candidate.publication.runtime.clone();
        self.publication.ownership = candidate.publication.ownership.clone();
    }

    pub(crate) fn publish_import_candidate(&mut self, candidate: Self) -> AppResult<()> {
        let unreconciled = candidate.removal_authority_unreconciled;
        self.publish_candidate(candidate.publication, false)?;
        // Import's private authoritative scan is also a valid reconciliation,
        // but a failed or unavailable scan must not clear the safety barrier.
        self.removal_authority_unreconciled = unreconciled;
        Ok(())
    }

    pub(crate) fn confirms_import_locator(
        &self,
        record: &PluginRecord,
        locator: &LocalPackageLocator,
    ) -> bool {
        self.publication.locals.get(&record.manifest().id) == Some(record)
            && self.publication.locators.get(&record.manifest().id) == Some(locator)
    }

    /// Record only the disabled decision; authoritative discovery owns membership.
    pub(crate) fn persist_import_disabled(
        &mut self,
        record: &PluginRecord,
    ) -> Result<(), StateDecisionFailure> {
        if record.source() != PluginSource::LocalDeclarative
            || matches!(
                self.publication.runtime,
                Runtime::Unavailable {
                    reason: PluginAvailabilityReason::CatalogInvalid,
                    ..
                }
            )
        {
            return Err(plugin_error("plugin_catalog_invalid", "插件目录无效").into());
        }
        self.persist_decision_with_outcome(record, false)
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
        let authority_reconciled = outcome.summary.status != LocalDiscoveryStatus::Unavailable
            && outcome.removals.status != LocalDiscoveryStatus::Unavailable
            && !next.ownership.requires_retry();
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
        next.management.retain(|id, _| next.locals.contains_key(id));
        next.local_summary = outcome
            .summary
            .clone()
            .with_additional_rejections(collisions);
        next.discovery = outcome;
        let state_retry = Self::load_state_if_needed(&mut next);
        let published = self.publish_candidate(next, true)?;
        if authority_reconciled {
            self.removal_authority_unreconciled = false;
        }
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
            .into_values()
            .map(|record| {
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
                item.with_management(Self::management_for_record(next, record))
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

    fn management_for_record(
        publication: &CatalogPublicationCandidate,
        record: &PluginRecord,
    ) -> PluginManagement {
        match record.source() {
            PluginSource::BuiltIn => PluginManagement::BuiltIn,
            PluginSource::LocalDeclarative => publication
                .management
                .get(&record.manifest().id)
                .copied()
                .unwrap_or(PluginManagement::External),
        }
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
        if self.removal_authority_unreconciled
            || expected_catalog_generation.parse::<u64>().ok()
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
        let management = Self::management_for_record(&self.publication, &record);
        if management == PluginManagement::RemovalPending {
            return Err(plugin_error("plugin_removal_pending", "插件移除回退待处理"));
        }
        self.persist_decision(&record, enabled)?;
        let Runtime::Available { state, .. } = &self.publication.runtime else {
            unreachable!("a successful decision requires available state");
        };
        Ok(PluginCatalogMutationResult::new(
            state.revision.to_string(),
            self.publication.catalog_generation.to_string(),
            catalog_item(&record, enabled).with_management(management),
        ))
    }

    fn persist_decision(&mut self, record: &PluginRecord, enabled: bool) -> AppResult<()> {
        self.persist_decision_with_outcome(record, enabled)
            .map_err(|failure| failure.error)
    }

    fn persist_decision_with_outcome(
        &mut self,
        record: &PluginRecord,
        enabled: bool,
    ) -> Result<(), StateDecisionFailure> {
        if self.removal_authority_unreconciled {
            return Err(
                plugin_error("plugin_catalog_stale", "插件目录已更新，请刷新后重试").into(),
            );
        }
        let mut candidate = self.publication.clone();
        let Runtime::Available {
            state,
            requires_rewrite,
            persistence,
        } = &mut candidate.runtime
        else {
            return Err(plugin_error("plugin_state_unavailable", "插件状态存储暂不可用").into());
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
            return Err(
                plugin_error("plugin_state_capacity_exceeded", "插件状态容量已达上限").into(),
            );
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
            _ => Err(StateDecisionFailure {
                error: plugin_error("plugin_state_persist_failed", "插件状态保存失败"),
                persist_outcome: outcome,
            }),
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

fn removal_disabled_candidate(
    state: &PluginStateFileV2,
    record: &PluginRecord,
) -> PluginStateFileV2 {
    let identity = record.identity();
    let mut next = state.clone();
    next.entries
        .retain(|entry| entry.id != identity.id || entry.source != identity.source);
    next.entries.push(PluginStateEntryV2 {
        id: identity.id,
        source: identity.source,
        publisher_id: identity.publisher_id,
        approval_fingerprint: identity.approval_fingerprint,
        enabled: false,
    });
    sort_state_entries(&mut next.entries);
    next.revision = state.revision.saturating_add(1);
    next
}

pub(crate) fn persistence_outcome(result: &PersistResult) -> PersistOutcome {
    result
        .as_ref()
        .copied()
        .unwrap_or_else(|failure| failure.outcome)
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
