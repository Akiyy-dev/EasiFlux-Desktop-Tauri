use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::FutureExt;
use tokio::sync::{Mutex, RwLock};

use crate::error::{AppError, AppResult};

use super::discovery::{LocalDiscoveryOutcome, LocalPluginDiscovery, SystemLocalPluginDiscovery};
use super::manifest::{PluginCatalogMutationResult, PluginCatalogSnapshot};
use super::PluginRegistry;

pub(crate) struct PluginRuntime {
    registry: RwLock<PluginRegistry>,
    discovery: Arc<dyn LocalPluginDiscovery>,
    reload_gate: Mutex<()>,
    initial_discovery_attempted: AtomicBool,
}

impl PluginRuntime {
    pub(crate) fn new() -> Self {
        Self::initialize(PluginRegistry::new(), Arc::new(SystemLocalPluginDiscovery))
    }

    pub(crate) fn initialize(
        registry: PluginRegistry,
        discovery: Arc<dyn LocalPluginDiscovery>,
    ) -> Self {
        Self {
            registry: RwLock::new(registry),
            discovery,
            reload_gate: Mutex::new(()),
            initial_discovery_attempted: AtomicBool::new(false),
        }
    }

    pub(crate) async fn get_catalog(self: &Arc<Self>) -> AppResult<PluginCatalogSnapshot> {
        if self.initial_discovery_attempted.load(Ordering::Acquire) {
            return Ok(self.recover_and_snapshot().await);
        }
        self.request_discovery(false).await
    }

    pub(crate) async fn reload_catalog(self: &Arc<Self>) -> AppResult<PluginCatalogSnapshot> {
        self.request_discovery(true).await
    }

    pub(crate) async fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
        expected_catalog_generation: &str,
    ) -> AppResult<PluginCatalogMutationResult> {
        self.registry
            .write()
            .await
            .set_enabled(id, enabled, expected_catalog_generation)
    }

    async fn recover_and_snapshot(&self) -> PluginCatalogSnapshot {
        let mut registry = self.registry.write().await;
        registry.retry_state_load();
        registry.catalog_snapshot()
    }

    async fn request_discovery(self: &Arc<Self>, force: bool) -> AppResult<PluginCatalogSnapshot> {
        let runtime = Arc::clone(self);
        // The owned task retains the gate through scan + publication even if a
        // command caller disappears. Catch/report panics even for detached callers.
        tokio::spawn(async move {
            AssertUnwindSafe(runtime.discover_and_publish(force))
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    tracing::error!("plugin catalog request failed");
                    Err(runtime_error())
                })
        })
        .await
        .map_err(|_| {
            tracing::error!("plugin catalog request worker unavailable");
            runtime_error()
        })?
    }

    async fn discover_and_publish(&self, force: bool) -> AppResult<PluginCatalogSnapshot> {
        let _gate = self.reload_gate.lock().await;
        if !force && self.initial_discovery_attempted.load(Ordering::Acquire) {
            return Ok(self.recover_and_snapshot().await);
        }
        let discovery = Arc::clone(&self.discovery);
        // No registry guard or copied enable state crosses the blocking boundary.
        let outcome = tokio::task::spawn_blocking(move || discovery.discover())
            .await
            .unwrap_or_else(|_| {
                tracing::warn!("local plugin discovery worker unavailable");
                LocalDiscoveryOutcome::unavailable()
            });
        let mut registry = self.registry.write().await;
        let publication = registry.apply_local_discovery(outcome);
        // Any completed scan counts, even an unavailable result or failed publish.
        self.initial_discovery_attempted
            .store(true, Ordering::Release);
        publication?;
        registry.retry_state_load();
        Ok(registry.catalog_snapshot())
    }
}

fn runtime_error() -> AppError {
    AppError::Internal("插件目录请求暂不可用".into())
}

#[cfg(test)]
mod tests;
