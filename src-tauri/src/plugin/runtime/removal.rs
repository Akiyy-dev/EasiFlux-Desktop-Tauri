use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures_util::FutureExt;

use crate::error::{AppError, AppResult};
use crate::plugin::discovery::{
    LocalDiscoveryOutcome, RemovalObservation, RemovalObservationShape,
};
use crate::plugin::manifest::{LocalDiscoveryStatus, PluginId};
use crate::plugin::registry::{persistence_outcome, RemovalPreflight};
use crate::plugin::removal::{
    AfterDisabledFailure, BeforeDisabledFailure, RemoveManagedLocalPluginResult as RemovalResult,
};
use crate::plugin::PluginRegistry;
use crate::storage::local_plugin_package::{
    CleanupOutcome, KnownCleanupShape, QuarantineRenameOutcome, RemovalStorageFailure,
};
use crate::storage::safe_plugin_document::{
    outcome_satisfies_destructive_barrier, persist_outcome_committed, PersistResult,
};

use super::{runtime_error, PluginRuntime};

impl PluginRuntime {
    pub(crate) async fn remove_managed_local_plugin(
        self: &Arc<Self>,
        id: &str,
        expected_catalog_generation: &str,
    ) -> AppResult<RemovalResult> {
        let reservation = self.reserve_operation();
        let runtime = Arc::clone(self);
        let id = id.to_owned();
        let generation = expected_catalog_generation.to_owned();
        // Reserving and spawning have no suspension gap. Caller cancellation
        // cannot abandon an accepted waiter, a barrier, or a committed rename.
        tokio::spawn(async move {
            AssertUnwindSafe(async move {
                let _operation = reservation.enter().await;
                tokio::task::spawn_blocking(move || run_removal(&runtime, &id, &generation))
                    .await
                    .map_err(|_| runtime_error())?
            })
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(runtime_error()))
        })
        .await
        .map_err(|_| runtime_error())?
    }
}

fn barrier_succeeded(result: &PersistResult) -> bool {
    result
        .as_ref()
        .is_ok_and(|outcome| outcome_satisfies_destructive_barrier(*outcome))
}

fn run_removal(
    runtime: &PluginRuntime,
    raw_id: &str,
    generation: &str,
) -> AppResult<RemovalResult> {
    let id = PluginId::parse(raw_id).map_err(|_| AppError::Plugin {
        code: "plugin_invalid_id",
        message: "插件标识无效",
        diagnostic: None,
    })?;
    // Frozen full publication and task-owned document candidate are different
    // objects. No registry guard crosses any external I/O below.
    let baseline = runtime.registry.blocking_read().clone();
    let preflight = match baseline.preflight_removal(&id, generation) {
        Ok(preflight) => preflight,
        Err(reason) => {
            return Ok(RemovalResult::not_removed(
                reason,
                baseline.catalog_snapshot(),
            ))
        }
    };
    let mut owned = match runtime
        .removal_storage
        .prepare(&preflight.locator, &preflight.entry)
    {
        Ok(owned) => owned,
        Err(reason) => {
            return Ok(RemovalResult::not_removed(
                before_storage_failure(reason),
                baseline.catalog_snapshot(),
            ))
        }
    };
    let mut candidate = baseline.clone();
    let disabled = candidate.persist_disabled_before_removal(&preflight.record);
    runtime
        .registry
        .blocking_write()
        .adopt_removal_documents(&candidate);
    if !barrier_succeeded(&disabled) {
        let snapshot = if persist_outcome_committed(persistence_outcome(&disabled)) {
            candidate.build_reconciled_candidate(baseline.removal_discovery_baseline());
            runtime
                .registry
                .blocking_write()
                .publish_candidate_once(candidate, &baseline)?
        } else {
            baseline.catalog_snapshot()
        };
        return Ok(RemovalResult::not_removed(
            BeforeDisabledFailure::StatePersistFailed,
            snapshot,
        ));
    }

    let removing = candidate.persist_removing(preflight.entry.receipt_id(), owned.removal_slot());
    runtime
        .registry
        .blocking_write()
        .adopt_removal_documents(&candidate);
    if !barrier_succeeded(&removing) {
        return rollback(
            runtime,
            candidate,
            &baseline,
            &preflight,
            AfterDisabledFailure::OwnershipPersistFailed,
        );
    }
    if owned.reverify_before_rename().is_err() {
        return rollback(
            runtime,
            candidate,
            &baseline,
            &preflight,
            AfterDisabledFailure::IdentityChanged,
        );
    }
    let rename = owned.quarantine_once();
    if let QuarantineRenameOutcome::ProvenNotCommitted(failure) = rename {
        let reason = if failure == RemovalStorageFailure::IdentityChanged {
            AfterDisabledFailure::IdentityChanged
        } else {
            AfterDisabledFailure::RemoveWriteFailed
        };
        return rollback(runtime, candidate, &baseline, &preflight, reason);
    }
    // Permanent result boundary: no code below may retry rename, roll back the
    // index, or construct NotRemoved, including after ambiguous native failure.
    let removing_entry = preflight.entry.begin_removal(owned.removal_slot().clone());
    let rename_uncertain = match rename {
        QuarantineRenameOutcome::Committed(evidence) => {
            if *evidence.entry != removing_entry || &evidence.removal_slot != owned.removal_slot() {
                return Err(runtime_error());
            }
            false
        }
        QuarantineRenameOutcome::CommitUnconfirmed => true,
        QuarantineRenameOutcome::ProvenNotCommitted(_) => {
            unreachable!("handled before commit boundary")
        }
    };
    let verified = owned.verify_quarantine().is_ok_and(|evidence| {
        *evidence.entry == removing_entry && &evidence.removal_slot == owned.removal_slot()
    });
    let mut outcome = std::panic::catch_unwind(AssertUnwindSafe(|| runtime.discovery.discover()))
        .unwrap_or_else(|_| LocalDiscoveryOutcome::unavailable());
    let scan_available = outcome.summary.status != LocalDiscoveryStatus::Unavailable
        && outcome.removals.status != LocalDiscoveryStatus::Unavailable;
    let scan_proves_conflict = outcome
        .occupied_slots
        .contains(preflight.entry.package_slot())
        || outcome.removals.observations.iter().any(|target| {
            &target.removal_slot == owned.removal_slot()
                && (target.directory_identity != preflight.entry.directory_identity
                    || matches!(
                        target.shape,
                        RemovalObservationShape::ManifestOnly | RemovalObservationShape::Unknown
                    )
                    || target.receipt.as_ref().is_some_and(|receipt| {
                        receipt.file_identity != preflight.entry.receipt_identity
                            || !preflight
                                .entry
                                .matches_receipt(&receipt.model, receipt.canonical_sha256)
                    })
                    || target.manifest.as_ref().is_some_and(|(record, identity)| {
                        *identity != preflight.entry.manifest_identity
                            || record != &preflight.record
                    }))
        });
    let cleanup = owned.cleanup_once();
    let physically_clean = matches!(
        cleanup,
        CleanupOutcome::Removed | CleanupOutcome::Pending(KnownCleanupShape::BothAbsent)
    );
    let deletion =
        physically_clean.then(|| candidate.delete_removed_entry(preflight.entry.receipt_id()));
    runtime
        .registry
        .blocking_write()
        .adopt_removal_documents(&candidate);
    let index_clean = deletion
        .as_ref()
        .is_some_and(|result| persist_outcome_committed(persistence_outcome(result)));
    let index_committed_error = deletion.as_ref().is_some_and(|result| {
        result.is_err() && persist_outcome_committed(persistence_outcome(result))
    });
    let normalized_id = id.as_str().to_owned();
    // Unavailable/ambiguous physical evidence never replaces the last complete
    // catalog, but document adoption and one exact cleanup still happen above.
    let source_remains = outcome
        .plugins
        .iter()
        .any(|package| preflight.entry.matches_package(package));
    if !scan_available
        || source_remains
        || (matches!(cleanup, CleanupOutcome::Conflict) && !scan_proves_conflict)
    {
        return Ok(RemovalResult::removed_catalog_unconfirmed(
            normalized_id,
            baseline.catalog_snapshot(),
        ));
    }
    update_target_observation(&mut outcome, &preflight, owned.removal_slot(), cleanup);
    candidate.build_reconciled_candidate(outcome);
    let reconciled = candidate.catalog_snapshot();
    let conflicted = reconciled.managed_ownership.conflicting_entry_count > 0
        || matches!(cleanup, CleanupOutcome::Conflict);
    let pending = reconciled.managed_ownership.cleanup_pending_count > 0;
    let snapshot = runtime
        .registry
        .blocking_write()
        .publish_candidate_once(candidate, &baseline)?;
    if rename_uncertain || !verified || index_committed_error || conflicted {
        Ok(RemovalResult::removed_catalog_unconfirmed(
            normalized_id,
            snapshot,
        ))
    } else if physically_clean && index_clean {
        Ok(RemovalResult::removed(normalized_id, snapshot))
    } else if pending {
        Ok(RemovalResult::removed_cleanup_pending(
            normalized_id,
            snapshot,
        ))
    } else {
        Ok(RemovalResult::removed_catalog_unconfirmed(
            normalized_id,
            snapshot,
        ))
    }
}

fn rollback(
    runtime: &PluginRuntime,
    mut candidate: PluginRegistry,
    baseline: &PluginRegistry,
    preflight: &RemovalPreflight,
    failure: AfterDisabledFailure,
) -> AppResult<RemovalResult> {
    // A rollback authorizes no further destructive action. Even a committed
    // error is adopted and is a successful rollback; never write it twice.
    let _result = candidate.rollback_removing(preflight.entry.receipt_id());
    runtime
        .registry
        .blocking_write()
        .adopt_removal_documents(&candidate);
    candidate.build_reconciled_candidate(baseline.removal_discovery_baseline());
    let snapshot = runtime
        .registry
        .blocking_write()
        .publish_candidate_once(candidate, baseline)?;
    Ok(RemovalResult::not_removed_after_disabled(failure, snapshot))
}

fn update_target_observation(
    outcome: &mut LocalDiscoveryOutcome,
    preflight: &RemovalPreflight,
    slot: &crate::plugin::ownership::RemovalSlot,
    cleanup: CleanupOutcome,
) {
    let shape = match cleanup {
        CleanupOutcome::Conflict => return,
        CleanupOutcome::Removed | CleanupOutcome::Pending(KnownCleanupShape::BothAbsent) => None,
        CleanupOutcome::Pending(KnownCleanupShape::Full) => Some(RemovalObservationShape::Full),
        CleanupOutcome::Pending(KnownCleanupShape::ReceiptOnly) => {
            Some(RemovalObservationShape::ReceiptOnly)
        }
        CleanupOutcome::Pending(KnownCleanupShape::EmptyDirectory) => {
            Some(RemovalObservationShape::EmptyDirectory)
        }
    };
    outcome
        .removals
        .observations
        .retain(|object| &object.removal_slot != slot);
    outcome
        .removals
        .occupied_slots
        .retain(|occupied| occupied != slot);
    if let Some(shape) = shape {
        outcome.removals.occupied_slots.push(slot.clone());
        outcome.removals.observations.push(RemovalObservation {
            removal_slot: slot.clone(),
            directory_identity: preflight.entry.directory_identity,
            manifest: (shape == RemovalObservationShape::Full)
                .then(|| (preflight.record.clone(), preflight.entry.manifest_identity)),
            receipt: (shape != RemovalObservationShape::EmptyDirectory)
                .then(|| preflight.locator.receipt.clone())
                .flatten(),
            shape,
        });
    }
}

fn before_storage_failure(failure: RemovalStorageFailure) -> BeforeDisabledFailure {
    match failure {
        RemovalStorageFailure::IdentityChanged => BeforeDisabledFailure::IdentityChanged,
        RemovalStorageFailure::StagingCapacityExceeded => {
            BeforeDisabledFailure::StagingCapacityExceeded
        }
        RemovalStorageFailure::Unavailable | RemovalStorageFailure::ProvenWriteFailure => {
            BeforeDisabledFailure::StorageUnavailable
        }
    }
}

#[cfg(test)]
mod tests;
