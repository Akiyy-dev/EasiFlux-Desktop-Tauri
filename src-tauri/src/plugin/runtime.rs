use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_util::FutureExt;
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock};

use crate::error::{AppError, AppResult};
use crate::storage::local_plugin_import::{
    LocalManifestImportStorage, SystemLocalManifestImportStorage,
};

use super::discovery::{LocalDiscoveryOutcome, LocalPluginDiscovery, SystemLocalPluginDiscovery};
use super::import::{ImportSessions, LocalManifestReader, SystemLocalManifestReader};
use super::manifest::{PluginCatalogMutationResult, PluginCatalogSnapshot};
use super::PluginRegistry;

pub(crate) struct PluginRuntime {
    registry: RwLock<PluginRegistry>,
    discovery: Arc<dyn LocalPluginDiscovery>,
    reader: Arc<dyn LocalManifestReader>,
    storage: Arc<dyn LocalManifestImportStorage>,
    import_sessions: Arc<ImportSessions>,
    operation_gate: Arc<Mutex<()>>,
    initial_discovery_attempted: AtomicBool,
}

type OperationWaiter = Pin<Box<dyn Future<Output = OwnedMutexGuard<()>> + Send>>;

enum OperationReservation {
    Ready(OwnedMutexGuard<()>),
    Waiting(OperationWaiter),
}

impl OperationReservation {
    fn enter(self) -> OperationWaiter {
        match self {
            Self::Ready(guard) => Box::pin(async move { guard }),
            Self::Waiting(waiter) => waiter,
        }
    }
}

impl PluginRuntime {
    pub(crate) fn new() -> Self {
        Self::initialize(PluginRegistry::new(), Arc::new(SystemLocalPluginDiscovery))
    }

    pub(crate) fn initialize(
        registry: PluginRegistry,
        discovery: Arc<dyn LocalPluginDiscovery>,
    ) -> Self {
        Self::with_import_services(
            registry,
            discovery,
            Arc::new(SystemLocalManifestReader),
            Arc::new(SystemLocalManifestImportStorage::new()),
        )
    }

    pub(crate) fn with_import_services(
        registry: PluginRegistry,
        discovery: Arc<dyn LocalPluginDiscovery>,
        reader: Arc<dyn LocalManifestReader>,
        storage: Arc<dyn LocalManifestImportStorage>,
    ) -> Self {
        Self {
            registry: RwLock::new(registry),
            discovery,
            reader,
            storage,
            import_sessions: ImportSessions::new(),
            operation_gate: Arc::new(Mutex::new(())),
            initial_discovery_attempted: AtomicBool::new(false),
        }
    }

    pub(crate) async fn get_catalog(self: &Arc<Self>) -> AppResult<PluginCatalogSnapshot> {
        if !self.initial_discovery_attempted.load(Ordering::Acquire) {
            return self.request_discovery(false).await;
        }
        {
            let registry = self.registry.read().await;
            if !registry.state_requires_retry() {
                return Ok(registry.catalog_snapshot());
            }
        }
        self.request_state_retry().await
    }

    pub(crate) async fn reload_catalog(self: &Arc<Self>) -> AppResult<PluginCatalogSnapshot> {
        self.request_discovery(true).await
    }

    pub(crate) async fn set_enabled(
        self: &Arc<Self>,
        id: &str,
        enabled: bool,
        expected_catalog_generation: &str,
    ) -> AppResult<PluginCatalogMutationResult> {
        let reservation = self.reserve_operation();
        let runtime = Arc::clone(self);
        let id = id.to_owned();
        let expected_catalog_generation = expected_catalog_generation.to_owned();
        tokio::spawn(async move {
            AssertUnwindSafe(async move {
                let _operation = reservation.enter().await;
                let owned_runtime = Arc::clone(&runtime);
                tokio::task::spawn_blocking(move || {
                    owned_runtime.registry.blocking_write().set_enabled(
                        &id,
                        enabled,
                        &expected_catalog_generation,
                    )
                })
                .await
                .map_err(|_| {
                    tracing::error!("plugin state mutation worker unavailable");
                    runtime_error()
                })?
            })
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                tracing::error!("plugin state mutation failed");
                Err(runtime_error())
            })
        })
        .await
        .map_err(|_| {
            tracing::error!("plugin state mutation request worker unavailable");
            runtime_error()
        })?
    }

    fn reserve_operation(&self) -> OperationReservation {
        let mut waiter: OperationWaiter = Box::pin(tokio::task::unconstrained(
            Arc::clone(&self.operation_gate).lock_owned(),
        ));
        let mut context = Context::from_waker(futures_util::task::noop_waker_ref());
        match waiter.as_mut().poll(&mut context) {
            Poll::Ready(guard) => OperationReservation::Ready(guard),
            Poll::Pending => OperationReservation::Waiting(waiter),
        }
    }

    async fn request_state_retry(self: &Arc<Self>) -> AppResult<PluginCatalogSnapshot> {
        let reservation = self.reserve_operation();
        let runtime = Arc::clone(self);
        tokio::spawn(async move {
            AssertUnwindSafe(async move {
                let _operation = reservation.enter().await;
                runtime.retry_state_and_snapshot().await
            })
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                tracing::error!("plugin state recovery request failed");
                Err(runtime_error())
            })
        })
        .await
        .map_err(|_| {
            tracing::error!("plugin state recovery request worker unavailable");
            runtime_error()
        })?
    }

    async fn retry_state_and_snapshot(self: &Arc<Self>) -> AppResult<PluginCatalogSnapshot> {
        {
            let registry = self.registry.read().await;
            if !registry.state_requires_retry() {
                return Ok(registry.catalog_snapshot());
            }
        }
        let runtime = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            let mut registry = runtime.registry.blocking_write();
            if registry.state_requires_retry() {
                registry.retry_state_load();
            }
            registry.catalog_snapshot()
        })
        .await
        .map_err(|_| {
            tracing::error!("plugin state recovery worker unavailable");
            runtime_error()
        })
    }

    async fn request_discovery(self: &Arc<Self>, force: bool) -> AppResult<PluginCatalogSnapshot> {
        let runtime = Arc::clone(self);
        let reservation = self.reserve_operation();
        // No suspension separates reservation and spawn. The owned task retains
        // the waiter/guard through scan + publication even if the caller disappears.
        tokio::spawn(async move {
            AssertUnwindSafe(async move {
                let _operation = reservation.enter().await;
                runtime.discover_and_publish(force).await
            })
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

    async fn discover_and_publish(
        self: &Arc<Self>,
        force: bool,
    ) -> AppResult<PluginCatalogSnapshot> {
        if !force && self.initial_discovery_attempted.load(Ordering::Acquire) {
            return self.retry_state_and_snapshot().await;
        }
        let discovery = Arc::clone(&self.discovery);
        // No registry guard or copied enable state crosses the blocking boundary.
        let outcome = tokio::task::spawn_blocking(move || discovery.discover())
            .await
            .unwrap_or_else(|_| {
                tracing::warn!("local plugin discovery worker unavailable");
                LocalDiscoveryOutcome::unavailable()
            });
        let publication = {
            let mut registry = self.registry.write().await;
            let publication = registry.apply_local_discovery(outcome);
            // Any completed scan counts, even an unavailable result or failed publish.
            self.initial_discovery_attempted
                .store(true, Ordering::Release);
            publication
        };
        publication?;
        self.retry_state_and_snapshot().await
    }
}

fn runtime_error() -> AppError {
    AppError::Internal("插件目录请求暂不可用".into())
}

mod import;

#[cfg(test)]
mod tests;
