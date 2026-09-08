use std::cell::Cell;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Instant;

use futures_util::FutureExt;

use crate::error::{AppError, AppResult};
use crate::plugin::discovery::LocalDiscoveryOutcome;
use crate::plugin::import::{
    CancelImportResult, CommitImportResult, CommitLease, ImportCommitFailure, ImportPreview,
    LocalManifestSelector, PrepareImportResult, PrepareLease, SelectedManifestSource,
};
use crate::plugin::manifest::{
    LocalDiscoveryStatus, PluginAvailability, PluginAvailabilityReason, PluginCatalogItem,
    PluginCatalogSnapshot,
};
use crate::plugin::record::PluginRecord;
use crate::storage::local_plugin_import::{ImportPromotionState, OwnedImportStage};

use super::{runtime_error, OperationReservation, PluginRuntime};

impl PluginRuntime {
    pub(crate) async fn prepare_import(
        self: &Arc<Self>,
        selector: Arc<dyn LocalManifestSelector>,
    ) -> AppResult<PrepareImportResult> {
        let lease = self.import_sessions.reserve_prepare(Instant::now())?;
        let runtime = Arc::clone(self);
        // Admission is reserved before the owned task is spawned. There is no
        // suspension where a second picker could overtake this request.
        tokio::spawn(async move {
            AssertUnwindSafe(run_prepare(runtime, selector, lease))
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    tracing::error!("plugin manifest prepare request failed");
                    Err(runtime_error())
                })
        })
        .await
        .map_err(|_| {
            tracing::error!("plugin manifest prepare request worker unavailable");
            runtime_error()
        })?
    }

    pub(crate) fn cancel_import(&self, token: &str) -> AppResult<CancelImportResult> {
        self.import_sessions.cancel(token, Instant::now())?;
        Ok(CancelImportResult::new())
    }

    pub(crate) async fn commit_import(
        self: &Arc<Self>,
        token: &str,
        expected_catalog_generation: &str,
    ) -> AppResult<CommitImportResult> {
        let lease = self.import_sessions.claim_commit(token, Instant::now())?;
        let reservation = self.reserve_operation();
        let runtime = Arc::clone(self);
        let expected_catalog_generation = expected_catalog_generation.to_owned();
        // The one-shot token and FIFO reservation are both owned before the
        // first await, so dropping the command caller cannot replay or cancel it.
        tokio::spawn(async move {
            AssertUnwindSafe(run_commit(
                runtime,
                lease,
                reservation,
                expected_catalog_generation,
            ))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                tracing::error!("plugin manifest commit request failed");
                Err(runtime_error())
            })
        })
        .await
        .map_err(|_| {
            tracing::error!("plugin manifest commit request worker unavailable");
            runtime_error()
        })?
    }
}

async fn run_prepare(
    runtime: Arc<PluginRuntime>,
    selector: Arc<dyn LocalManifestSelector>,
    lease: PrepareLease,
) -> AppResult<PrepareImportResult> {
    let initial = runtime.get_catalog().await?;
    ensure_import_available(&initial)?;
    let selected = selector.select().await?;
    let SelectedManifestSource::Selected(path) = selected else {
        return Ok(PrepareImportResult::cancelled());
    };
    let reader = Arc::clone(&runtime.reader);
    let content = tokio::task::spawn_blocking(move || reader.read(&path))
        .await
        .map_err(|_| {
            tracing::error!("plugin manifest source worker unavailable");
            runtime_error()
        })??;
    let (snapshot, generation) = {
        let registry = runtime.registry.read().await;
        (registry.catalog_snapshot(), registry.catalog_generation())
    };
    ensure_import_available(&snapshot)?;
    let preview: ImportPreview = lease.publish(content, generation, Instant::now())?;
    Ok(PrepareImportResult::Ready(preview))
}

async fn run_commit(
    runtime: Arc<PluginRuntime>,
    lease: CommitLease,
    reservation: OperationReservation,
    expected_catalog_generation: String,
) -> AppResult<CommitImportResult> {
    let _operation = reservation.enter().await;
    // The owned FIFO task retains admission for the complete blocking lifecycle.
    // All filesystem work uses a private registry; no live registry guard crosses it.
    tokio::task::spawn_blocking(move || {
        let progress = CommitProgress::default();
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            run_commit_blocking(&runtime, &lease, &expected_catalog_generation, &progress)
        }))
        .unwrap_or_else(|_| {
            if progress.promotion_started.get() {
                if !progress.postscan_started.get() {
                    let _ = scan_for_import(&runtime);
                }
                Ok(unconfirmed_import(&runtime, lease.content().record()))
            } else {
                Err(runtime_error())
            }
        })
    })
    .await
    .map_err(|_| runtime_error())?
}

#[derive(Default)]
struct CommitProgress {
    promotion_started: Cell<bool>,
    postscan_started: Cell<bool>,
}

fn run_commit_blocking(
    runtime: &Arc<PluginRuntime>,
    lease: &CommitLease,
    expected_generation: &str,
    progress: &CommitProgress,
) -> AppResult<CommitImportResult> {
    let mut candidate = runtime.registry.blocking_read().clone();
    if !matches_generation(
        expected_generation,
        lease.generation(),
        candidate.catalog_generation(),
    ) {
        return Ok(failed_import(
            runtime,
            ImportCommitFailure::CatalogStale,
            false,
        ));
    }
    let prescan = scan_for_import(runtime);
    let usage = prescan.usage;
    if let Err(error) = candidate.apply_local_discovery(prescan) {
        if matches!(
            error,
            AppError::Plugin {
                code: "plugin_catalog_generation_exhausted",
                ..
            }
        ) {
            return Ok(failed_import(
                runtime,
                ImportCommitFailure::CatalogGenerationExhausted,
                false,
            ));
        }
        return Err(runtime_error());
    }
    if !matches_generation(
        expected_generation,
        lease.generation(),
        candidate.catalog_generation(),
    ) {
        // A changed preflight is itself a complete terminal result, never an
        // intermediate publication on the successful import path.
        runtime
            .registry
            .blocking_write()
            .publish_import_candidate(candidate)?;
        return Ok(failed_import(
            runtime,
            ImportCommitFailure::CatalogStale,
            false,
        ));
    }
    let validation = usage
        .ok_or(ImportCommitFailure::DiscoveryUnavailable)
        .and_then(|usage| {
            candidate.validate_managed_import(lease.content().record(), usage, expected_generation)
        });
    if let Err(reason) = validation {
        return Ok(failed_import(runtime, reason, false));
    }
    let record = lease.content().record();
    let mut stage = match runtime
        .storage
        .prepare_stage(record, lease.content().bytes())
    {
        Ok(stage) => stage,
        Err(reason) => return Ok(failed_import(runtime, reason, false)),
    };
    let persisted = std::panic::catch_unwind(AssertUnwindSafe(|| {
        candidate.persist_import_disabled(record)
    }));
    runtime
        .registry
        .blocking_write()
        .adopt_import_documents(&candidate);
    match persisted {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            cleanup_owned_stage(stage);
            runtime
                .registry
                .blocking_write()
                .publish_import_candidate(candidate)?;
            return Ok(failed_import(runtime, import_state_failure(&error), false));
        }
        Err(_) => {
            cleanup_owned_stage(stage);
            return Err(runtime_error());
        }
    }
    progress.promotion_started.set(true);
    let promoted = std::panic::catch_unwind(AssertUnwindSafe(|| stage.promote()));
    let (promotion, mut uncertain) = match promoted {
        Ok(Ok(promotion)) => (Some(promotion), false),
        Ok(Err(_)) if stage.promotion_state() == ImportPromotionState::NotCommitted => {
            progress.promotion_started.set(false);
            cleanup_owned_stage(stage);
            runtime
                .registry
                .blocking_write()
                .publish_import_candidate(candidate)?;
            return Ok(failed_import(
                runtime,
                ImportCommitFailure::WriteFailed,
                true,
            ));
        }
        // A returned error after a committed or unconfirmed promotion cannot
        // authorize publication any more than a worker panic can.
        Ok(Err(_)) => (None, true),
        Err(_) => (None, true),
    };
    // No path below this point cleans the stage/package or says notImported.
    let mut registered = false;
    if let Some(proof) = &promotion {
        let package = crate::plugin::discovery::DiscoveredLocalPlugin {
            record: record.clone(),
            locator: proof.locator.clone(),
        };
        if proof.entry.proves_managed(&package) {
            let registration = std::panic::catch_unwind(AssertUnwindSafe(|| {
                candidate.register_managed_import(proof.entry.clone())
            }));
            match registration {
                Ok(Ok(_)) => registered = true,
                Ok(Err(failure)) => {
                    // NotCommitted preserves the old index and may be classified
                    // external, but a committed error is publication uncertainty.
                    uncertain = failure.persist_outcome
                        != crate::storage::safe_plugin_document::PersistOutcome::NotCommitted;
                    tracing::warn!(
                        code = failure.failure.as_code(),
                        "plugin import ownership registration failed"
                    );
                }
                Err(_) => uncertain = true,
            }
            runtime
                .registry
                .blocking_write()
                .adopt_import_documents(&candidate);
        }
    }
    progress.postscan_started.set(true);
    let postscan = scan_for_import(runtime);
    if postscan.summary.status == LocalDiscoveryStatus::Unavailable {
        return Ok(unconfirmed_import(runtime, record));
    }
    // Exactly one final scan, and at most one complete public swap. Even when
    // registration is uncertain the read is bounded; it never authorizes replay.
    let publication = std::panic::catch_unwind(AssertUnwindSafe(|| {
        candidate.apply_local_discovery(postscan)
    }));
    if uncertain || !matches!(publication, Ok(Ok(_))) {
        return Ok(unconfirmed_import(runtime, record));
    }
    let locator_confirmed = promotion
        .as_ref()
        .is_some_and(|proof| candidate.confirms_import_locator(record, &proof.locator));
    if runtime
        .registry
        .blocking_write()
        .publish_import_candidate(candidate)
        .is_err()
    {
        return Ok(unconfirmed_import(runtime, record));
    }
    let snapshot = runtime.registry.blocking_read().catalog_snapshot();
    Ok(classify_import_result(
        record,
        registered,
        locator_confirmed,
        snapshot,
    ))
}

fn failed_import(
    runtime: &Arc<PluginRuntime>,
    failure: ImportCommitFailure,
    saved: bool,
) -> CommitImportResult {
    CommitImportResult::not_imported(
        failure,
        saved,
        runtime.registry.blocking_read().catalog_snapshot(),
    )
}

fn unconfirmed_import(runtime: &Arc<PluginRuntime>, record: &PluginRecord) -> CommitImportResult {
    CommitImportResult::imported_not_visible(
        record.manifest().id.to_string(),
        runtime.registry.blocking_read().catalog_snapshot(),
    )
}

fn matches_generation(requested: &str, lease: u64, current: u64) -> bool {
    requested.parse::<u64>().ok() == Some(lease)
        && requested == lease.to_string()
        && current == lease
}

fn ensure_import_available(snapshot: &PluginCatalogSnapshot) -> AppResult<()> {
    if snapshot.availability != PluginAvailability::Available {
        return Err(match snapshot.availability_reason_code {
            Some(PluginAvailabilityReason::CatalogInvalid) => {
                plugin_error("plugin_catalog_invalid", "插件目录无效")
            }
            _ => plugin_error("plugin_state_unavailable", "插件状态存储暂不可用"),
        });
    }
    if snapshot.local_discovery.status != LocalDiscoveryStatus::Available {
        return Err(plugin_error(
            "plugin_import_discovery_unavailable",
            "请先修复本地插件发现问题，再导入清单。",
        ));
    }
    Ok(())
}

fn scan_for_import(runtime: &Arc<PluginRuntime>) -> LocalDiscoveryOutcome {
    std::panic::catch_unwind(AssertUnwindSafe(|| runtime.discovery.discover())).unwrap_or_else(
        |_| {
            tracing::warn!("local plugin import discovery worker unavailable");
            LocalDiscoveryOutcome::unavailable()
        },
    )
}

fn classify_import_result(
    record: &PluginRecord,
    registered: bool,
    locator_confirmed: bool,
    snapshot: PluginCatalogSnapshot,
) -> CommitImportResult {
    use crate::plugin::manifest::PluginManagement;
    let plugin_id = record.manifest().id.to_string();
    let expected = PluginCatalogItem::disabled(record.manifest().clone(), record.source());
    if snapshot.availability == PluginAvailability::Available {
        if registered
            && locator_confirmed
            && snapshot
                .plugins
                .iter()
                .any(|item| item == &expected.clone().with_management(PluginManagement::Managed))
        {
            return CommitImportResult::imported(plugin_id, snapshot);
        }
        if !registered && locator_confirmed && snapshot.plugins.iter().any(|item| item == &expected)
        {
            return CommitImportResult::imported_external(plugin_id, snapshot);
        }
    }
    CommitImportResult::imported_not_visible(plugin_id, snapshot)
}

fn cleanup_owned_stage(mut stage: Box<dyn OwnedImportStage>) {
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| stage.cleanup()));
}

fn import_state_failure(error: &AppError) -> ImportCommitFailure {
    match error {
        AppError::Plugin { code, .. } => match *code {
            "plugin_catalog_invalid" => ImportCommitFailure::CatalogInvalid,
            "plugin_state_unavailable" => ImportCommitFailure::StateUnavailable,
            "plugin_state_capacity_exceeded" => ImportCommitFailure::StateCapacityExceeded,
            "plugin_revision_exhausted" => ImportCommitFailure::RevisionExhausted,
            "plugin_state_persist_failed" => ImportCommitFailure::StatePersistFailed,
            _ => ImportCommitFailure::StatePersistFailed,
        },
        _ => ImportCommitFailure::StatePersistFailed,
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
