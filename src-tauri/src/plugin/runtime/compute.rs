use super::*;
use crate::plugin::compute::{
    error, execute_guest, validate_request_id, CancelComputeResult, ComputeRequest, ComputeResult,
};

struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl PluginRuntime {
    pub(crate) async fn execute_compute(
        self: &Arc<Self>,
        request: ComputeRequest,
    ) -> AppResult<ComputeResult> {
        request.validate()?;
        let epoch = self.compute_epoch.load(Ordering::Acquire);
        // Do not queue a run across an in-progress/queued lifecycle operation.
        let operation = self
            .operation_gate
            .try_lock()
            .map_err(|_| error("plugin_compute_unavailable"))?;
        if !self.initial_discovery_attempted.load(Ordering::Acquire) {
            return Err(error("plugin_compute_unavailable"));
        }
        let captured = self.registry.read().await.capture_compute(&request)?;
        let lease = self.compute_slot.acquire(&request.request_id)?;
        let cancel_on_drop = CancelOnDrop(lease.cancel.clone());
        if epoch == u64::MAX || self.compute_epoch.load(Ordering::Acquire) != epoch {
            return Err(error("plugin_compute_cancelled"));
        }
        drop(operation);
        // Owned lease lives INSIDE the blocking task. Dropping/aborting this IPC
        // future cancels cooperatively but cannot free the running slot early.
        let worker = tokio::task::spawn_blocking(move || {
            let lease = lease;
            let value = execute_guest(
                &captured.params,
                &request.values,
                request.parameter,
                &lease.cancel,
            )?;
            Ok::<_, AppError>((value, request, captured))
        });
        let (value, request, captured) = worker
            .await
            .map_err(|_| error("plugin_compute_internal"))??;
        // Revalidate after the IPC future resumes, not just inside the worker:
        // a lifecycle transition can happen while its successful output is queued.
        let _operation = self
            .operation_gate
            .try_lock()
            .map_err(|_| error("plugin_compute_unavailable"))?;
        let registry = self.registry.read().await;
        if cancel_on_drop.0.load(Ordering::Acquire)
            || self.compute_epoch.load(Ordering::Acquire) != epoch
        {
            return Err(error("plugin_compute_cancelled"));
        }
        registry.revalidate_compute(&request, &captured)?;
        Ok(ComputeResult {
            schema_version: 1,
            request_id: request.request_id,
            plugin_id: request.plugin_id,
            contribution_id: request.contribution_id,
            catalog_generation: request.expected_catalog_generation,
            revision: request.expected_revision,
            value,
            input_count: request.values.len(),
            parameter: request.parameter,
        })
    }
    pub(crate) fn cancel_compute(&self, request_id: &str) -> AppResult<CancelComputeResult> {
        validate_request_id(request_id)?;
        Ok(CancelComputeResult {
            schema_version: 1,
            request_id: request_id.into(),
            cancelled: self.compute_slot.cancel(request_id)?,
        })
    }
}

#[cfg(test)]
mod tests;
