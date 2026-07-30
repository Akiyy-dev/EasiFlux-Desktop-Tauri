use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tauri::Manager;
use url::{Host, Url};

use crate::api::news_client::{NewsPageFetcher, TgForwarderNewsClient};
use crate::services::news::ports::{
    NewsEventSink, NewsRepository, NewsRepositoryFactory, NewsTokenStoreFactory,
};
use crate::services::NewsService;
use crate::storage::{
    FallbackNewsTokenStore, KeyringNewsTokenStore, NewsApiToken, NewsDatabase, NewsTokenStore,
    NewsTokenStoreError,
};

const NEWS_MESSAGES_PATH: &str = "api/public/v1/messages";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewsSource {
    pub(crate) base_url: Url,
    pub(crate) fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NewsSourceConfigError;

pub(crate) fn parse_news_source(
    base_url: Option<&str>,
    source_epoch: Option<&str>,
) -> Result<NewsSource, NewsSourceConfigError> {
    let (Some(base_url), Some(source_epoch)) = (base_url, source_epoch) else {
        return Err(NewsSourceConfigError);
    };
    if !valid_epoch(source_epoch) {
        return Err(NewsSourceConfigError);
    }
    let mut base_url = Url::parse(base_url).map_err(|_| NewsSourceConfigError)?;
    if base_url.username() != ""
        || base_url.password().is_some()
        || base_url.query().is_some()
        || base_url.fragment().is_some()
        || base_url.host().is_none()
        || !valid_scheme(&base_url)
    {
        return Err(NewsSourceConfigError);
    }
    if !base_url.path().ends_with('/') {
        base_url.set_path(&format!("{}/", base_url.path()));
    }
    let endpoint = base_url
        .join(NEWS_MESSAGES_PATH)
        .map_err(|_| NewsSourceConfigError)?;
    let mut digest = Sha256::new();
    digest.update(endpoint.as_str().as_bytes());
    digest.update([0]);
    digest.update(source_epoch.as_bytes());
    Ok(NewsSource {
        base_url,
        fingerprint: hex::encode(digest.finalize()),
    })
}

fn valid_epoch(epoch: &str) -> bool {
    (1..=64).contains(&epoch.len())
        && epoch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_scheme(url: &Url) -> bool {
    url.scheme() == "https"
        || (url.scheme() == "http"
            && match url.host() {
                Some(Host::Domain(host)) => host == "localhost",
                Some(Host::Ipv4(address)) => address == Ipv4Addr::new(127, 0, 0, 1),
                Some(Host::Ipv6(address)) => address.is_loopback(),
                None => false,
            })
}

pub(crate) fn news_database_path(app_local_data: &Path) -> PathBuf {
    app_local_data.join("news").join("news.sqlite3")
}

struct UnavailableNewsTokenStore;

impl NewsTokenStore for UnavailableNewsTokenStore {
    fn load(&self) -> Result<Option<crate::storage::NewsApiToken>, NewsTokenStoreError> {
        Err(NewsTokenStoreError::Keyring)
    }

    fn set(&self, _token: &crate::storage::NewsApiToken) -> Result<(), NewsTokenStoreError> {
        Err(NewsTokenStoreError::Keyring)
    }

    fn delete(&self) -> Result<(), NewsTokenStoreError> {
        Err(NewsTokenStoreError::Keyring)
    }
}

pub(crate) fn assemble_recoverable_news_with<OpenDatabase, BuildTokenStore, BuildClient>(
    source: Result<NewsSource, NewsSourceConfigError>,
    event_sink: Arc<dyn NewsEventSink>,
    open_database: OpenDatabase,
    build_token_store: BuildTokenStore,
    build_client: BuildClient,
) -> Arc<NewsService>
where
    OpenDatabase: Fn() -> Result<Arc<dyn NewsRepository>, crate::storage::NewsStorageError>
        + Send
        + Sync
        + 'static,
    BuildTokenStore:
        Fn() -> Result<Arc<dyn NewsTokenStore>, NewsTokenStoreError> + Send + Sync + 'static,
    BuildClient: FnOnce(Url) -> Result<Arc<dyn NewsPageFetcher>, ()>,
{
    let (source_fingerprint, client) = match source {
        Ok(source) => match build_client(source.base_url) {
            Ok(client) => (Some(source.fingerprint), Some(client)),
            Err(()) => (None, None),
        },
        Err(_) => (None, None),
    };
    let token_store_factory = client.as_ref().map(|_| {
        Arc::new(move || {
            build_token_store()
                .unwrap_or_else(|_| Arc::new(UnavailableNewsTokenStore) as Arc<dyn NewsTokenStore>)
        }) as Arc<NewsTokenStoreFactory>
    });
    Arc::new(NewsService::recoverable(
        source_fingerprint,
        Arc::new(open_database) as Arc<NewsRepositoryFactory>,
        client,
        token_store_factory,
        event_sink,
    ))
}

pub(crate) fn assemble_recoverable_news_with_fallback<OpenDatabase, BuildTokenStore, BuildClient>(
    source: Result<NewsSource, NewsSourceConfigError>,
    embedded_token: Option<&'static str>,
    event_sink: Arc<dyn NewsEventSink>,
    open_database: OpenDatabase,
    build_token_store: BuildTokenStore,
    build_client: BuildClient,
) -> Arc<NewsService>
where
    OpenDatabase: Fn() -> Result<Arc<dyn NewsRepository>, crate::storage::NewsStorageError>
        + Send
        + Sync
        + 'static,
    BuildTokenStore:
        Fn() -> Result<Arc<dyn NewsTokenStore>, NewsTokenStoreError> + Send + Sync + 'static,
    BuildClient: FnOnce(Url) -> Result<Arc<dyn NewsPageFetcher>, ()>,
{
    let embedded_token =
        embedded_token.filter(|value| NewsApiToken::parse(value.as_bytes()).is_ok());
    let source = if embedded_token.is_some() {
        source
    } else {
        Err(NewsSourceConfigError)
    };

    assemble_recoverable_news_with(
        source,
        event_sink,
        open_database,
        move || {
            let fallback = embedded_token
                .and_then(|value| NewsApiToken::parse(value.as_bytes()).ok())
                .ok_or(NewsTokenStoreError::Keyring)?;
            let keyring = build_token_store().ok();
            Ok(Arc::new(FallbackNewsTokenStore::new(keyring, fallback)) as Arc<dyn NewsTokenStore>)
        },
        build_client,
    )
}

pub(crate) fn build_news_service(
    app: &tauri::AppHandle,
    event_sink: Arc<dyn NewsEventSink>,
) -> Arc<NewsService> {
    let source = parse_news_source(
        option_env!("EASIFLUX_NEWS_API_BASE_URL"),
        option_env!("EASIFLUX_NEWS_SOURCE_EPOCH"),
    );
    let app = app.clone();
    assemble_recoverable_news_with_fallback(
        source,
        option_env!("EASIFLUX_NEWS_API_TOKEN"),
        event_sink,
        move || {
            let directory = app
                .path()
                .app_local_data_dir()
                .map_err(|_| crate::storage::NewsStorageError::database())?;
            NewsDatabase::open(news_database_path(&directory))
                .map(|database| Arc::new(database) as Arc<dyn NewsRepository>)
        },
        || KeyringNewsTokenStore::new().map(|store| Arc::new(store) as Arc<dyn NewsTokenStore>),
        |base_url| {
            TgForwarderNewsClient::new(base_url)
                .map(|client| Arc::new(client) as Arc<dyn NewsPageFetcher>)
                .map_err(|_| ())
        },
    )
}

#[cfg(test)]
#[path = "news_runtime_tests.rs"]
mod tests;
