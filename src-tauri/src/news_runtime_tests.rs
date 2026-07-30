use std::collections::VecDeque;
use std::future::pending;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::Duration;

use async_trait::async_trait;

use tokio::sync::Notify;

use super::{assemble_recoverable_news_with, news_database_path, parse_news_source, NewsSource};
use crate::api::news_client::{NewsFetchError, NewsFetchErrorKind, NewsPageFetcher};
use crate::models::news::{
    NewsMessageDto, NewsMessagesCommittedEvent, NewsPage, NewsStatusKind, NewsStatusSnapshot,
    NewsUnreadSnapshot, ValidatedNewsPage,
};
use crate::services::news::ports::{NewsEventError, NewsEventSink, NewsRepository};
use crate::services::news::NewsShutdownOutcome;
use crate::storage::{
    NewsApiToken, NewsCommitOutcome, NewsDatabaseState, NewsStorageError, NewsTokenStore,
    NewsTokenStoreError,
};

struct CachedRepository {
    state_calls: Arc<Mutex<Vec<ThreadId>>>,
    fail_state: bool,
}

impl NewsRepository for CachedRepository {
    fn prepare_source(&self, _fingerprint: &str) -> Result<(), NewsStorageError> {
        Ok(())
    }

    fn state_snapshot(&self) -> Result<NewsDatabaseState, NewsStorageError> {
        self.state_calls
            .lock()
            .unwrap()
            .push(std::thread::current().id());
        if self.fail_state {
            return Err(NewsStorageError::database());
        }
        Ok(NewsDatabaseState {
            cursor: 9,
            last_seen_delivery_id: 8,
            initial_sync_complete: true,
            synced_count: 1,
            latest_delivery_id: Some(9),
            unread_count: 1,
        })
    }

    fn commit_page(
        &self,
        _page: &ValidatedNewsPage,
        _received_at_ms: i64,
    ) -> Result<NewsCommitOutcome, NewsStorageError> {
        unreachable!("assembly tests never commit")
    }

    fn list_messages(
        &self,
        _before_delivery_id: Option<i64>,
        _limit: usize,
    ) -> Result<NewsPage, NewsStorageError> {
        Ok(NewsPage {
            items: vec![NewsMessageDto {
                delivery_id: "9".into(),
                created_at: "2026-07-30T00:00:00.000Z".into(),
                text: "cached".into(),
            }],
            has_more: false,
            latest_delivery_id: Some("9".into()),
            unread_count: 1,
        })
    }

    fn mark_seen(&self, _through_delivery_id: i64) -> Result<NewsUnreadSnapshot, NewsStorageError> {
        Ok(NewsUnreadSnapshot {
            latest_delivery_id: Some("9".into()),
            unread_count: 0,
        })
    }
}

#[derive(Default)]
struct NullEvents;

impl NewsEventSink for NullEvents {
    fn emit_messages_committed(
        &self,
        _event: &NewsMessagesCommittedEvent,
    ) -> Result<(), NewsEventError> {
        Ok(())
    }

    fn emit_status_changed(&self, _status: &NewsStatusSnapshot) -> Result<(), NewsEventError> {
        Ok(())
    }
}

fn assemble_news_with<OpenDatabase, BuildTokenStore, BuildClient>(
    database_path: Result<PathBuf, ()>,
    source: Result<NewsSource, super::NewsSourceConfigError>,
    event_sink: Arc<dyn NewsEventSink>,
    open_database: OpenDatabase,
    build_token_store: BuildTokenStore,
    build_client: BuildClient,
) -> Arc<crate::services::NewsService>
where
    OpenDatabase: Fn(&Path) -> Result<Arc<dyn NewsRepository>, ()> + Send + Sync + 'static,
    BuildTokenStore: Fn() -> Result<Arc<dyn NewsTokenStore>, ()> + Send + Sync + 'static,
    BuildClient: FnOnce(url::Url) -> Result<Arc<dyn NewsPageFetcher>, ()>,
{
    assemble_recoverable_news_with(
        source,
        event_sink,
        move || {
            let path = database_path
                .as_ref()
                .map_err(|_| NewsStorageError::database())?;
            open_database(path).map_err(|_| NewsStorageError::database())
        },
        move || build_token_store().map_err(|_| NewsTokenStoreError::Keyring),
        build_client,
    )
}

struct NeverFetcher;

#[async_trait]
impl NewsPageFetcher for NeverFetcher {
    async fn fetch_after(
        &self,
        _token: &NewsApiToken,
        _cursor: i64,
        _limit: usize,
    ) -> Result<ValidatedNewsPage, NewsFetchError> {
        panic!("credential-store construction failure must prevent network access")
    }
}

struct StaticTokenStore;

impl NewsTokenStore for StaticTokenStore {
    fn load(&self) -> Result<Option<NewsApiToken>, NewsTokenStoreError> {
        Ok(Some(NewsApiToken::parse(b"test-token").unwrap()))
    }

    fn set(&self, _token: &NewsApiToken) -> Result<(), NewsTokenStoreError> {
        unreachable!()
    }

    fn delete(&self) -> Result<(), NewsTokenStoreError> {
        unreachable!()
    }
}

enum FetchStep {
    Pending,
    ContractError,
    Page,
}

struct RecoveryFetcher {
    steps: Mutex<VecDeque<FetchStep>>,
    cursors: Mutex<Vec<i64>>,
    called: Notify,
}

impl RecoveryFetcher {
    fn new(steps: impl IntoIterator<Item = FetchStep>) -> Self {
        Self {
            steps: Mutex::new(steps.into_iter().collect()),
            cursors: Mutex::new(Vec::new()),
            called: Notify::new(),
        }
    }

    async fn wait_for_calls(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let notified = self.called.notified();
                if self.cursors.lock().unwrap().len() >= count {
                    return;
                }
                notified.await;
            }
        })
        .await
        .unwrap();
    }
}

#[async_trait]
impl NewsPageFetcher for RecoveryFetcher {
    async fn fetch_after(
        &self,
        _token: &NewsApiToken,
        cursor: i64,
        _limit: usize,
    ) -> Result<ValidatedNewsPage, NewsFetchError> {
        self.cursors.lock().unwrap().push(cursor);
        self.called.notify_waiters();
        let step = { self.steps.lock().unwrap().pop_front() };
        match step {
            Some(FetchStep::ContractError) => {
                Err(NewsFetchError::new(NewsFetchErrorKind::Contract))
            }
            Some(FetchStep::Page) => Ok(ValidatedNewsPage {
                items: vec![],
                next_cursor: cursor,
                has_more: false,
            }),
            Some(FetchStep::Pending) | None => pending().await,
        }
    }
}

struct RecoveryRepository {
    fail_commit: bool,
}

impl NewsRepository for RecoveryRepository {
    fn prepare_source(&self, _fingerprint: &str) -> Result<(), NewsStorageError> {
        Ok(())
    }

    fn state_snapshot(&self) -> Result<NewsDatabaseState, NewsStorageError> {
        Ok(NewsDatabaseState {
            cursor: 9,
            last_seen_delivery_id: 8,
            initial_sync_complete: true,
            synced_count: 9,
            latest_delivery_id: Some(9),
            unread_count: 1,
        })
    }

    fn commit_page(
        &self,
        _page: &ValidatedNewsPage,
        _received_at_ms: i64,
    ) -> Result<NewsCommitOutcome, NewsStorageError> {
        if self.fail_commit {
            Err(NewsStorageError::database())
        } else {
            Ok(NewsCommitOutcome {
                inserted_count: 0,
                newest_delivery_id: Some(9),
                unread_count: 1,
                initial_sync_complete: true,
            })
        }
    }

    fn list_messages(
        &self,
        _before_delivery_id: Option<i64>,
        _limit: usize,
    ) -> Result<NewsPage, NewsStorageError> {
        Ok(NewsPage {
            items: vec![],
            has_more: false,
            latest_delivery_id: Some("9".into()),
            unread_count: 1,
        })
    }

    fn mark_seen(&self, _through_delivery_id: i64) -> Result<NewsUnreadSnapshot, NewsStorageError> {
        Ok(NewsUnreadSnapshot {
            latest_delivery_id: Some("9".into()),
            unread_count: 0,
        })
    }
}

async fn wait_for_attempts(attempts: &AtomicUsize, count: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while attempts.load(Ordering::SeqCst) < count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn wait_for_status(service: &Arc<crate::services::NewsService>, kind: NewsStatusKind) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while service.status().kind != kind {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

fn repository_factory(
    opened_path: Arc<Mutex<Option<PathBuf>>>,
    state_calls: Arc<Mutex<Vec<ThreadId>>>,
    fail_state: bool,
) -> impl Fn(&Path) -> Result<Arc<dyn NewsRepository>, ()> {
    move |path| {
        *opened_path.lock().unwrap() = Some(path.to_owned());
        Ok(Arc::new(CachedRepository {
            state_calls: state_calls.clone(),
            fail_state,
        }))
    }
}

async fn wait_for_state_call(state_calls: &Arc<Mutex<Vec<ThreadId>>>) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while state_calls.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn wait_for_synced_count(service: &Arc<crate::services::NewsService>, count: u64) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while service.status().synced_count != count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn source_parser_normalizes_the_base_and_uses_the_full_endpoint_fingerprint() {
    let root = parse_news_source(Some("https://news.example.test"), Some("epoch-v1")).unwrap();
    assert_eq!(root.base_url.as_str(), "https://news.example.test/");
    assert_eq!(
        root.fingerprint,
        "20475d8a9e8c4a3657e754c28df66c051aee86c5eb14e8b5520614e3dd6a6d7b"
    );

    let nested =
        parse_news_source(Some("https://news.example.test/base"), Some("source.2")).unwrap();
    assert_eq!(nested.base_url.as_str(), "https://news.example.test/base/");
    assert_eq!(
        nested.fingerprint,
        "6548a98f1b7fc098cda5e1f54ec2bbc6059297b79da1e7d7fca6b2beccfee0ee"
    );
}

#[test]
fn source_parser_rejects_absent_partial_or_invalid_compile_time_values() {
    for (base_url, epoch) in [
        (None, None),
        (Some("https://news.example.test"), None),
        (None, Some("epoch-v1")),
        (Some("not a URL"), Some("epoch-v1")),
        (Some("https://user@news.example.test"), Some("epoch-v1")),
        (Some("https://news.example.test?q=1"), Some("epoch-v1")),
        (Some("https://news.example.test"), Some("bad epoch")),
    ] {
        assert!(parse_news_source(base_url, epoch).is_err());
    }
}

#[test]
fn database_path_is_scoped_below_app_local_data() {
    assert_eq!(
        news_database_path(Path::new("C:/app-local")),
        PathBuf::from("C:/app-local/news/news.sqlite3")
    );
}

#[tokio::test]
async fn path_or_database_failure_degrades_to_storage_error() {
    let source = parse_news_source(Some("https://news.example.test"), Some("epoch-v1"));
    let events: Arc<dyn NewsEventSink> = Arc::new(NullEvents);
    let path_failure = assemble_news_with(
        Err(()),
        source.clone(),
        events.clone(),
        |_| panic!("database factory must not run after path resolution failure"),
        || Ok(Arc::new(StaticTokenStore) as Arc<dyn NewsTokenStore>),
        |_| Ok(Arc::new(NeverFetcher) as Arc<dyn NewsPageFetcher>),
    );
    path_failure.start();
    wait_for_status(&path_failure, NewsStatusKind::StorageError).await;

    let database_failure = assemble_news_with(
        Ok(PathBuf::from("C:/app-local/news/news.sqlite3")),
        source,
        events,
        |_| Err(()),
        || Ok(Arc::new(StaticTokenStore) as Arc<dyn NewsTokenStore>),
        |_| Ok(Arc::new(NeverFetcher) as Arc<dyn NewsPageFetcher>),
    );
    database_failure.start();
    wait_for_status(&database_failure, NewsStatusKind::StorageError).await;
}

#[tokio::test]
async fn source_misconfiguration_preserves_completed_cached_reads() {
    let runtime_thread = std::thread::current().id();
    let opened_path = Arc::new(Mutex::new(None));
    let state_calls = Arc::new(Mutex::new(Vec::new()));
    let service = assemble_news_with(
        Ok(PathBuf::from("C:/app-local/news/news.sqlite3")),
        parse_news_source(None, None),
        Arc::new(NullEvents),
        repository_factory(opened_path.clone(), state_calls.clone(), false),
        || panic!("misconfigured source must not construct Keyring"),
        |_| panic!("misconfigured source must not construct a client"),
    );

    service.start();
    wait_for_state_call(&state_calls).await;
    wait_for_synced_count(&service, 1).await;
    assert_eq!(
        service.status().kind,
        NewsStatusKind::DeploymentMisconfigured
    );
    assert!(service.status().initial_sync_complete);
    assert_eq!(service.status().synced_count, 1);
    assert_eq!(service.status().latest_delivery_id.as_deref(), Some("9"));
    assert_eq!(service.status().unread_count, 1);
    assert!(state_calls
        .lock()
        .unwrap()
        .iter()
        .all(|thread| *thread != runtime_thread));
    assert_eq!(
        service.list_messages(None, 50).await.unwrap().items[0].text,
        "cached"
    );
    assert_eq!(
        opened_path.lock().unwrap().as_deref(),
        Some(Path::new("C:/app-local/news/news.sqlite3"))
    );
}

#[tokio::test]
async fn client_construction_failure_preserves_complete_cached_status() {
    let state_calls = Arc::new(Mutex::new(Vec::new()));
    let service = assemble_news_with(
        Ok(PathBuf::from("C:/app-local/news/news.sqlite3")),
        parse_news_source(Some("https://news.example.test"), Some("epoch-v1")),
        Arc::new(NullEvents),
        repository_factory(Arc::new(Mutex::new(None)), state_calls.clone(), false),
        || panic!("failed client construction must not construct Keyring"),
        |_| Err(()),
    );

    service.start();
    wait_for_state_call(&state_calls).await;
    wait_for_synced_count(&service, 1).await;
    let status = service.status();
    assert_eq!(status.kind, NewsStatusKind::DeploymentMisconfigured);
    assert!(status.initial_sync_complete);
    assert_eq!(status.synced_count, 1);
    assert_eq!(status.latest_delivery_id.as_deref(), Some("9"));
    assert_eq!(status.unread_count, 1);
}

#[tokio::test]
async fn degraded_cached_state_read_failure_stays_redacted_and_nonfatal() {
    let state_calls = Arc::new(Mutex::new(Vec::new()));
    let service = assemble_news_with(
        Ok(PathBuf::from("C:/private/user/news.sqlite3")),
        parse_news_source(None, None),
        Arc::new(NullEvents),
        repository_factory(Arc::new(Mutex::new(None)), state_calls.clone(), true),
        || panic!("misconfigured source must not construct Keyring"),
        |_| panic!("misconfigured source must not construct a client"),
    );

    service.start();
    wait_for_state_call(&state_calls).await;
    let status = service.status();
    assert_eq!(status.kind, NewsStatusKind::DeploymentMisconfigured);
    assert_eq!(status.synced_count, 0);
    let serialized = serde_json::to_string(&status).unwrap();
    assert!(!serialized.contains("private"));
    assert!(!serialized.contains("sqlite"));
    assert!(!serialized.contains("database"));
}

#[tokio::test]
async fn token_factory_failure_keeps_cache_and_publishes_credential_store_unavailable() {
    let opened_path = Arc::new(Mutex::new(None));
    let state_calls = Arc::new(Mutex::new(Vec::new()));
    let service = assemble_news_with(
        Ok(PathBuf::from("C:/app-local/news/news.sqlite3")),
        parse_news_source(Some("https://news.example.test"), Some("epoch-v1")),
        Arc::new(NullEvents),
        repository_factory(opened_path, state_calls, false),
        || Err(()),
        |_| Ok(Arc::new(NeverFetcher) as Arc<dyn NewsPageFetcher>),
    );

    service.start();
    tokio::time::timeout(Duration::from_secs(2), async {
        while service.status().kind != NewsStatusKind::CredentialStoreUnavailable {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        service.list_messages(None, 50).await.unwrap().items[0].text,
        "cached"
    );
    assert_eq!(
        service.stop_and_join(Duration::from_secs(1)).await,
        NewsShutdownOutcome::Stopped
    );
}

#[tokio::test]
async fn initial_open_failure_recovers_once_after_retry_without_resetting_cursor() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let repaired = Arc::new(AtomicBool::new(false));
    let fetcher = Arc::new(RecoveryFetcher::new([FetchStep::Pending]));
    let service = assemble_recoverable_news_with(
        parse_news_source(Some("https://news.example.test"), Some("epoch-v1")),
        Arc::new(NullEvents),
        {
            let attempts = attempts.clone();
            let repaired = repaired.clone();
            move || {
                attempts.fetch_add(1, Ordering::SeqCst);
                if repaired.load(Ordering::SeqCst) {
                    Ok(Arc::new(RecoveryRepository { fail_commit: false })
                        as Arc<dyn NewsRepository>)
                } else {
                    Err(NewsStorageError::database())
                }
            }
        },
        || Ok(Arc::new(StaticTokenStore) as Arc<dyn NewsTokenStore>),
        {
            let fetcher = fetcher.clone();
            move |_| Ok(fetcher as Arc<dyn NewsPageFetcher>)
        },
    );

    assert_eq!(attempts.load(Ordering::SeqCst), 0);
    service.start();
    service.start();
    service.start();
    wait_for_attempts(&attempts, 1).await;
    wait_for_status(&service, NewsStatusKind::StorageError).await;
    assert!(fetcher.cursors.lock().unwrap().is_empty());
    tokio::task::yield_now().await;
    assert_eq!(attempts.load(Ordering::SeqCst), 1);

    repaired.store(true, Ordering::SeqCst);
    service.retry_sync();
    fetcher.wait_for_calls(1).await;
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(*fetcher.cursors.lock().unwrap(), [9]);
    assert_eq!(service.status().synced_count, 9);
    assert_eq!(service.status().latest_delivery_id.as_deref(), Some("9"));
    assert_eq!(
        service.stop_and_join(Duration::from_secs(1)).await,
        NewsShutdownOutcome::Stopped
    );
}

#[tokio::test]
async fn failed_storage_recovery_can_be_retried_and_each_signal_opens_once() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let fetcher = Arc::new(RecoveryFetcher::new([FetchStep::Pending]));
    let service = assemble_recoverable_news_with(
        parse_news_source(Some("https://news.example.test"), Some("epoch-v1")),
        Arc::new(NullEvents),
        {
            let attempts = attempts.clone();
            move || {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                if attempt < 2 {
                    Err(NewsStorageError::database())
                } else {
                    Ok(Arc::new(RecoveryRepository { fail_commit: false })
                        as Arc<dyn NewsRepository>)
                }
            }
        },
        || Ok(Arc::new(StaticTokenStore) as Arc<dyn NewsTokenStore>),
        {
            let fetcher = fetcher.clone();
            move |_| Ok(fetcher as Arc<dyn NewsPageFetcher>)
        },
    );

    service.start();
    wait_for_attempts(&attempts, 1).await;
    wait_for_status(&service, NewsStatusKind::StorageError).await;
    service.retry_sync();
    wait_for_attempts(&attempts, 2).await;
    assert_eq!(service.status().kind, NewsStatusKind::StorageError);
    assert!(fetcher.cursors.lock().unwrap().is_empty());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    service.retry_sync();
    fetcher.wait_for_calls(1).await;
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    assert_eq!(*fetcher.cursors.lock().unwrap(), [9]);
    assert_eq!(
        service.stop_and_join(Duration::from_secs(1)).await,
        NewsShutdownOutcome::Stopped
    );
}

#[tokio::test]
async fn runtime_storage_retry_reopens_but_contract_retry_reuses_repository() {
    let storage_attempts = Arc::new(AtomicUsize::new(0));
    let storage_fetcher = Arc::new(RecoveryFetcher::new([FetchStep::Page, FetchStep::Pending]));
    let storage = assemble_recoverable_news_with(
        parse_news_source(Some("https://news.example.test"), Some("epoch-v1")),
        Arc::new(NullEvents),
        {
            let attempts = storage_attempts.clone();
            move || {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(RecoveryRepository {
                    fail_commit: attempt == 0,
                }) as Arc<dyn NewsRepository>)
            }
        },
        || Ok(Arc::new(StaticTokenStore) as Arc<dyn NewsTokenStore>),
        {
            let fetcher = storage_fetcher.clone();
            move |_| Ok(fetcher as Arc<dyn NewsPageFetcher>)
        },
    );
    storage.start();
    wait_for_status(&storage, NewsStatusKind::StorageError).await;
    storage.retry_sync();
    storage.recheck_credentials();
    storage_fetcher.wait_for_calls(2).await;
    assert_eq!(storage_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(*storage_fetcher.cursors.lock().unwrap(), [9, 9]);

    let contract_attempts = Arc::new(AtomicUsize::new(0));
    let contract_fetcher = Arc::new(RecoveryFetcher::new([
        FetchStep::ContractError,
        FetchStep::Pending,
    ]));
    let contract = assemble_recoverable_news_with(
        parse_news_source(Some("https://news.example.test"), Some("epoch-v1")),
        Arc::new(NullEvents),
        {
            let attempts = contract_attempts.clone();
            move || {
                attempts.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(RecoveryRepository { fail_commit: false }) as Arc<dyn NewsRepository>)
            }
        },
        || Ok(Arc::new(StaticTokenStore) as Arc<dyn NewsTokenStore>),
        {
            let fetcher = contract_fetcher.clone();
            move |_| Ok(fetcher as Arc<dyn NewsPageFetcher>)
        },
    );
    contract.start();
    wait_for_status(&contract, NewsStatusKind::ContractError).await;
    contract.retry_sync();
    contract.recheck_credentials();
    contract_fetcher.wait_for_calls(2).await;
    assert_eq!(contract_attempts.load(Ordering::SeqCst), 1);

    assert_eq!(
        storage.stop_and_join(Duration::from_secs(1)).await,
        NewsShutdownOutcome::Stopped
    );
    assert_eq!(
        contract.stop_and_join(Duration::from_secs(1)).await,
        NewsShutdownOutcome::Stopped
    );
}
