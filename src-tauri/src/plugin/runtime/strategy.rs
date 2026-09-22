use super::*;
use crate::plugin::{
    compute::slot::ComputeLease, registry::strategy::CapturedStrategy, strategy::error,
    workflow::AuthorityRequest,
};
impl PluginRuntime {
    pub(crate) async fn strategy_gate(&self) -> OwnedMutexGuard<()> {
        self.operation_gate.clone().lock_owned().await
    }
    pub(crate) fn strategy_try_gate(&self) -> AppResult<OwnedMutexGuard<()>> {
        self.operation_gate
            .clone()
            .try_lock_owned()
            .map_err(|_| error("plugin_strategy_busy"))
    }
    pub(crate) fn strategy_epoch(&self) -> AppResult<u64> {
        let value = self.compute_epoch.load(Ordering::Acquire);
        if value == u64::MAX {
            Err(error("plugin_strategy_stale"))
        } else {
            Ok(value)
        }
    }
    pub(crate) async fn capture_strategy(
        &self,
        r: &AuthorityRequest,
    ) -> AppResult<CapturedStrategy> {
        if !self.initial_discovery_attempted.load(Ordering::Acquire) {
            return Err(error("plugin_strategy_unavailable"));
        }
        self.registry.read().await.capture_strategy(r)
    }
    pub(crate) fn strategy_compute(&self, id: &str) -> AppResult<ComputeLease> {
        self.compute_slot
            .acquire(id)
            .map_err(|_| error("plugin_strategy_busy"))
    }
}
