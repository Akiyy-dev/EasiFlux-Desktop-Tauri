mod backoff;
mod blocking;
mod control;
mod error;
mod facade;
mod poller;
pub(crate) mod ports;
mod shutdown;
mod status;

use std::sync::{Arc, Mutex};

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::api::news_client::NewsPageFetcher;
use crate::models::news::{NewsStatusKind, NewsStatusSnapshot};
use crate::storage::NewsTokenStore;
use control::ManualSignal;
pub use error::NewsServiceError;
#[cfg(test)]
pub use error::NewsServiceErrorKind;
use ports::{NewsEventSink, NewsRepository, NewsRepositoryFactory, NewsTokenStoreFactory};
pub(crate) use shutdown::NewsShutdownOutcome;
use shutdown::ShutdownState;

pub struct NewsService {
    repository: Mutex<Option<Arc<dyn NewsRepository>>>,
    repository_factory: Option<Arc<NewsRepositoryFactory>>,
    client: Option<Arc<dyn NewsPageFetcher>>,
    token_store: Option<Arc<dyn NewsTokenStore>>,
    token_store_factory: Option<Arc<NewsTokenStoreFactory>>,
    event_sink: Arc<dyn NewsEventSink>,
    source_fingerprint: Option<String>,
    status: Mutex<NewsStatusSnapshot>,
    task: Mutex<Option<JoinHandle<()>>>,
    cancellation: watch::Sender<bool>,
    manual: watch::Sender<ManualSignal>,
    shutdown: ShutdownState,
}

impl NewsService {
    #[cfg(test)]
    pub(crate) fn new(
        source_fingerprint: String,
        repository: Arc<dyn NewsRepository>,
        client: Arc<dyn NewsPageFetcher>,
        token_store: Arc<dyn NewsTokenStore>,
        event_sink: Arc<dyn NewsEventSink>,
    ) -> Self {
        Self::build(
            Some(source_fingerprint),
            Some(repository),
            None,
            Some(client),
            Some(token_store),
            None,
            event_sink,
            NewsStatusKind::InitialSync,
        )
    }

    pub(crate) fn recoverable(
        source_fingerprint: Option<String>,
        repository_factory: Arc<NewsRepositoryFactory>,
        client: Option<Arc<dyn NewsPageFetcher>>,
        token_store_factory: Option<Arc<NewsTokenStoreFactory>>,
        event_sink: Arc<dyn NewsEventSink>,
    ) -> Self {
        let kind = if source_fingerprint.is_some() && client.is_some() {
            NewsStatusKind::InitialSync
        } else {
            NewsStatusKind::DeploymentMisconfigured
        };
        Self::build(
            source_fingerprint,
            None,
            Some(repository_factory),
            client,
            None,
            token_store_factory,
            event_sink,
            kind,
        )
    }

    #[cfg(test)]
    pub(crate) fn deployment_misconfigured(event_sink: Arc<dyn NewsEventSink>) -> Self {
        Self::build(
            None,
            None,
            None,
            None,
            None,
            None,
            event_sink,
            NewsStatusKind::DeploymentMisconfigured,
        )
    }

    #[cfg(test)]
    pub(crate) fn deployment_misconfigured_with_repository(
        repository: Arc<dyn NewsRepository>,
        event_sink: Arc<dyn NewsEventSink>,
    ) -> Self {
        Self::build(
            None,
            Some(repository),
            None,
            None,
            None,
            None,
            event_sink,
            NewsStatusKind::DeploymentMisconfigured,
        )
    }

    #[cfg(test)]
    pub(crate) fn storage_error(event_sink: Arc<dyn NewsEventSink>) -> Self {
        Self::build(
            None,
            None,
            None,
            None,
            None,
            None,
            event_sink,
            NewsStatusKind::StorageError,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the private constructor maps each optional runtime dependency explicitly"
    )]
    fn build(
        source_fingerprint: Option<String>,
        repository: Option<Arc<dyn NewsRepository>>,
        repository_factory: Option<Arc<NewsRepositoryFactory>>,
        client: Option<Arc<dyn NewsPageFetcher>>,
        token_store: Option<Arc<dyn NewsTokenStore>>,
        token_store_factory: Option<Arc<NewsTokenStoreFactory>>,
        event_sink: Arc<dyn NewsEventSink>,
        kind: NewsStatusKind,
    ) -> Self {
        let (cancellation, _) = watch::channel(false);
        let (manual, _) = watch::channel(ManualSignal::default());
        Self {
            repository: Mutex::new(repository),
            repository_factory,
            client,
            token_store,
            token_store_factory,
            event_sink,
            source_fingerprint,
            status: Mutex::new(status::empty_snapshot(kind)),
            task: Mutex::new(None),
            cancellation,
            manual,
            shutdown: ShutdownState::new(),
        }
    }
}

#[cfg(test)]
#[path = "news/tests/mod.rs"]
mod tests;
