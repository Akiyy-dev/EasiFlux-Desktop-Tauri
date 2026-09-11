use std::panic::AssertUnwindSafe;
use std::sync::atomic::Ordering;
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
use crate::storage::local_plugin_import::{OwnedImportStage, Promotion};

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
    let current_generation = runtime.registry.read().await.catalog_generation();
    if !matches_generation(
        &expected_catalog_generation,
        lease.generation(),
        current_generation,
    ) {
        return Ok(
            snapshot_after_failure(&runtime, ImportCommitFailure::CatalogStale, false).await,
        );
    }

    let prescan = scan_for_import(&runtime).await;
    let usage = prescan.usage;
    if let Err(error) = publish_import_outcome(&runtime, prescan).await {
        if matches!(
            error,
            AppError::Plugin {
                code: "plugin_catalog_generation_exhausted",
                ..
            }
        ) {
            return Ok(snapshot_after_failure(
                &runtime,
                ImportCommitFailure::CatalogGenerationExhausted,
                false,
            )
            .await);
        }
        tracing::error!("plugin import prescan publication failed");
        return Err(runtime_error());
    }
    let current_generation = runtime.registry.read().await.catalog_generation();
    if !matches_generation(
        &expected_catalog_generation,
        lease.generation(),
        current_generation,
    ) {
        return Ok(
            snapshot_after_failure(&runtime, ImportCommitFailure::CatalogStale, false).await,
        );
    }

    let validation = match usage {
        Some(usage) => runtime
            .registry
            .read()
            .await
            .validate_import(lease.content().record(), usage),
        None => Err(ImportCommitFailure::DiscoveryUnavailable),
    };
    if let Err(reason) = validation {
        return Ok(snapshot_after_failure(&runtime, reason, false).await);
    }

    let storage = Arc::clone(&runtime.storage);
    let bytes = lease.content().bytes().to_vec();
    let mut stage = match tokio::task::spawn_blocking(move || storage.prepare_stage(&bytes)).await {
        Ok(Ok(stage)) => stage,
        Ok(Err(reason)) => {
            return Ok(snapshot_after_failure(&runtime, reason, false).await);
        }
        Err(_) => {
            tracing::error!("plugin manifest staging worker unavailable");
            return Err(runtime_error());
        }
    };

    let record = lease.content().record().clone();
    let state_runtime = Arc::clone(&runtime);
    let persist = tokio::task::spawn_blocking(move || {
        state_runtime
            .registry
            .blocking_write()
            .persist_import_disabled(&record)
    })
    .await;
    match persist {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            cleanup_owned_stage(stage).await;
            return Ok(snapshot_after_failure(&runtime, import_state_failure(&error), false).await);
        }
        Err(_) => {
            cleanup_owned_stage(stage).await;
            tracing::error!("plugin import state worker unavailable");
            return Err(runtime_error());
        }
    }

    let promoted = tokio::task::spawn_blocking(move || {
        let result = stage.promote();
        (stage, result)
    })
    .await;
    let promotion = match promoted {
        Ok((_, Ok(promotion))) => promotion,
        Ok((stage, Err(_))) => {
            cleanup_owned_stage(stage).await;
            return Ok(
                snapshot_after_failure(&runtime, ImportCommitFailure::WriteFailed, true).await,
            );
        }
        Err(_) => {
            // Promotion is the commit point. A worker panic cannot prove which
            // side of the rename it reached, so this is deliberately unknown.
            tracing::error!("plugin manifest promotion worker unavailable");
            return Err(runtime_error());
        }
    };

    let postscan = scan_for_import(&runtime).await;
    let snapshot = match publish_import_outcome(&runtime, postscan).await {
        Ok(snapshot) => snapshot,
        Err(_) => {
            let snapshot = runtime.registry.read().await.catalog_snapshot();
            return Ok(CommitImportResult::imported_not_visible(
                lease.content().record().manifest().id.to_string(),
                snapshot,
            ));
        }
    };
    Ok(classify_import_result(
        lease.content().record(),
        promotion,
        snapshot,
    ))
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

async fn scan_for_import(runtime: &Arc<PluginRuntime>) -> LocalDiscoveryOutcome {
    let discovery = Arc::clone(&runtime.discovery);
    tokio::task::spawn_blocking(move || discovery.discover())
        .await
        .unwrap_or_else(|_| {
            tracing::warn!("local plugin import discovery worker unavailable");
            LocalDiscoveryOutcome::unavailable()
        })
}

async fn snapshot_after_failure(
    runtime: &Arc<PluginRuntime>,
    failure: ImportCommitFailure,
    disabled_decision_saved: bool,
) -> CommitImportResult {
    let snapshot = runtime.registry.read().await.catalog_snapshot();
    CommitImportResult::not_imported(failure, disabled_decision_saved, snapshot)
}

async fn publish_import_outcome(
    runtime: &Arc<PluginRuntime>,
    outcome: LocalDiscoveryOutcome,
) -> AppResult<PluginCatalogSnapshot> {
    let publication = {
        let mut registry = runtime.registry.write().await;
        let publication = registry.apply_local_discovery(outcome);
        runtime
            .initial_discovery_attempted
            .store(true, Ordering::Release);
        publication.map(|_| registry.catalog_snapshot())
    };
    publication
}

fn classify_import_result(
    record: &PluginRecord,
    promotion: Promotion,
    snapshot: PluginCatalogSnapshot,
) -> CommitImportResult {
    let plugin_id = record.manifest().id.to_string();
    let expected = PluginCatalogItem::disabled(record.manifest().clone(), record.source());
    if promotion.object_identity_verified
        && snapshot.availability == PluginAvailability::Available
        && snapshot.plugins.iter().any(|item| item == &expected)
    {
        CommitImportResult::imported(plugin_id, snapshot)
    } else {
        CommitImportResult::imported_not_visible(plugin_id, snapshot)
    }
}

async fn cleanup_owned_stage(mut stage: Box<dyn OwnedImportStage>) {
    let _ = tokio::task::spawn_blocking(move || stage.cleanup()).await;
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
