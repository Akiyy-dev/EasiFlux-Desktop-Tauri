use std::collections::{HashSet, VecDeque};
use std::future::pending;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::ThreadId;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Notify;

use crate::api::news_client::{
    NewsFetchError, NewsFetchErrorKind, NewsPageFetcher, ValidatedNewsMessage, ValidatedNewsPage,
};
use crate::models::news::{
    NewsMessageDto, NewsMessagesCommittedEvent, NewsPage, NewsStatusKind, NewsStatusSnapshot,
    NewsUnreadSnapshot,
};
use crate::services::news::ports::{NewsEventError, NewsEventSink, NewsRepository};
use crate::services::news::NewsService;
use crate::storage::{
    NewsApiToken, NewsCommitOutcome, NewsDatabaseState, NewsStorageError, NewsStorageErrorKind,
    NewsTokenStore, NewsTokenStoreError,
};

pub(super) enum FetchAction {
    Page(ValidatedNewsPage),
    Error(NewsFetchErrorKind, Option<Duration>),
    Pending(Arc<AtomicBool>),
}

#[derive(Default)]
pub(super) struct ScriptedFetcher {
    actions: Mutex<VecDeque<FetchAction>>,
    calls: Mutex<Vec<(i64, usize)>>,
    called: Notify,
}

impl ScriptedFetcher {
    pub(super) fn push(&self, action: FetchAction) {
        self.actions.lock().unwrap().push_back(action);
    }

    pub(super) fn calls(&self) -> Vec<(i64, usize)> {
        self.calls.lock().unwrap().clone()
    }
}

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl NewsPageFetcher for ScriptedFetcher {
    async fn fetch_after(
        &self,
        _token: &NewsApiToken,
        cursor: i64,
        limit: usize,
    ) -> Result<ValidatedNewsPage, NewsFetchError> {
        self.calls.lock().unwrap().push((cursor, limit));
        self.called.notify_waiters();
        let action = { self.actions.lock().unwrap().pop_front() };
        match action {
            Some(FetchAction::Page(page)) => Ok(page),
            Some(FetchAction::Error(kind, retry_after)) => {
                Err(NewsFetchError::with_retry_after(kind, retry_after))
            }
            Some(FetchAction::Pending(dropped)) => {
                let _drop_flag = DropFlag(dropped);
                pending().await
            }
            None => pending().await,
        }
    }
}

#[derive(Clone)]
pub(super) enum TokenOutcome {
    Present(Vec<u8>),
    Missing,
    Error,
}

pub(super) struct FakeTokenStore {
    outcome: Mutex<TokenOutcome>,
    loads: Mutex<Vec<ThreadId>>,
}

impl FakeTokenStore {
    pub(super) fn new(outcome: TokenOutcome) -> Self {
        Self {
            outcome: Mutex::new(outcome),
            loads: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn set(&self, outcome: TokenOutcome) {
        *self.outcome.lock().unwrap() = outcome;
    }

    pub(super) fn load_threads(&self) -> Vec<ThreadId> {
        self.loads.lock().unwrap().clone()
    }
}

impl NewsTokenStore for FakeTokenStore {
    fn load(&self) -> Result<Option<NewsApiToken>, NewsTokenStoreError> {
        self.loads.lock().unwrap().push(std::thread::current().id());
        match self.outcome.lock().unwrap().clone() {
            TokenOutcome::Present(bytes) => NewsApiToken::parse(&bytes)
                .map(Some)
                .map_err(|_| NewsTokenStoreError::Keyring),
            TokenOutcome::Missing => Ok(None),
            TokenOutcome::Error => Err(NewsTokenStoreError::Keyring),
        }
    }

    fn set(&self, _token: &NewsApiToken) -> Result<(), NewsTokenStoreError> {
        unreachable!("poller never writes credentials")
    }

    fn delete(&self) -> Result<(), NewsTokenStoreError> {
        unreachable!("poller never deletes credentials")
    }
}

#[derive(Default)]
pub(super) struct BlockingGate {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl BlockingGate {
    fn enter_and_wait(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        while !state.1 {
            state = self.changed.wait(state).unwrap();
        }
    }

    pub(super) fn entered(&self) -> bool {
        self.state.lock().unwrap().0
    }

    pub(super) fn wait_until_entered(&self, timeout: Duration) -> bool {
        let state = self.state.lock().unwrap();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, timeout, |state| !state.0)
            .unwrap();
        state.0
    }

    pub(super) fn release(&self) {
        let mut state = self.state.lock().unwrap();
        state.1 = true;
        self.changed.notify_all();
    }
}

pub(super) struct FakeRepository {
    inner: Mutex<RepoInner>,
    calls: Mutex<Vec<(&'static str, ThreadId)>>,
    commit_gate: Mutex<Option<Arc<BlockingGate>>>,
    mark_gate: Mutex<Option<Arc<BlockingGate>>>,
}

struct RepoInner {
    state: NewsDatabaseState,
    ids: HashSet<i64>,
    prepare_error: Option<NewsStorageErrorKind>,
    state_error: bool,
    commit_error: bool,
}

impl Default for FakeRepository {
    fn default() -> Self {
        Self {
            inner: Mutex::new(RepoInner {
                state: empty_state(),
                ids: HashSet::new(),
                prepare_error: None,
                state_error: false,
                commit_error: false,
            }),
            calls: Mutex::new(Vec::new()),
            commit_gate: Mutex::new(None),
            mark_gate: Mutex::new(None),
        }
    }
}

impl FakeRepository {
    pub(super) fn set_prepare_error(&self, kind: Option<NewsStorageErrorKind>) {
        self.inner.lock().unwrap().prepare_error = kind;
    }

    pub(super) fn fail_next_state(&self) {
        self.inner.lock().unwrap().state_error = true;
    }

    pub(super) fn fail_next_commit(&self) {
        self.inner.lock().unwrap().commit_error = true;
    }

    pub(super) fn block_commit(&self, gate: Arc<BlockingGate>) {
        *self.commit_gate.lock().unwrap() = Some(gate);
    }

    pub(super) fn block_mark(&self, gate: Arc<BlockingGate>) {
        *self.mark_gate.lock().unwrap() = Some(gate);
    }

    pub(super) fn snapshot(&self) -> NewsDatabaseState {
        self.inner.lock().unwrap().state.clone()
    }

    pub(super) fn seed_state(&self, state: NewsDatabaseState) {
        self.inner.lock().unwrap().state = state;
    }

    pub(super) fn calls(&self) -> Vec<(&'static str, ThreadId)> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, name: &'static str) {
        self.calls
            .lock()
            .unwrap()
            .push((name, std::thread::current().id()));
    }
}

impl NewsRepository for FakeRepository {
    fn prepare_source(&self, _fingerprint: &str) -> Result<(), NewsStorageError> {
        self.record("prepare");
        match self.inner.lock().unwrap().prepare_error {
            Some(NewsStorageErrorKind::SourceMismatch) => Err(NewsStorageError::source_mismatch()),
            Some(NewsStorageErrorKind::InvalidInput) => Err(NewsStorageError::invalid_input()),
            Some(NewsStorageErrorKind::Database) => Err(NewsStorageError::database()),
            None => Ok(()),
        }
    }

    fn state_snapshot(&self) -> Result<NewsDatabaseState, NewsStorageError> {
        self.record("state");
        let mut inner = self.inner.lock().unwrap();
        if std::mem::take(&mut inner.state_error) {
            Err(NewsStorageError::database())
        } else {
            Ok(inner.state.clone())
        }
    }

    fn commit_page(
        &self,
        page: &ValidatedNewsPage,
        _received_at_ms: i64,
    ) -> Result<NewsCommitOutcome, NewsStorageError> {
        self.record("commit");
        if let Some(gate) = self.commit_gate.lock().unwrap().take() {
            gate.enter_and_wait();
        }
        let mut inner = self.inner.lock().unwrap();
        if std::mem::take(&mut inner.commit_error) {
            return Err(NewsStorageError::database());
        }
        let mut inserted = 0;
        for item in &page.items {
            if inner.ids.insert(item.delivery_id) {
                inserted += 1;
            }
        }
        inner.state.cursor = page.next_cursor;
        inner.state.synced_count = inner.ids.len() as u64;
        inner.state.latest_delivery_id = inner.ids.iter().copied().max();
        if !inner.state.initial_sync_complete && !page.has_more {
            inner.state.initial_sync_complete = true;
            inner.state.last_seen_delivery_id = inner.state.latest_delivery_id.unwrap_or(0);
            inner.state.unread_count = 0;
        } else if inner.state.initial_sync_complete {
            inner.state.unread_count = inner
                .ids
                .iter()
                .filter(|id| **id > inner.state.last_seen_delivery_id)
                .count()
                .min(100) as u32;
        }
        Ok(NewsCommitOutcome {
            inserted_count: inserted,
            newest_delivery_id: inner.state.latest_delivery_id,
            unread_count: inner.state.unread_count,
            initial_sync_complete: inner.state.initial_sync_complete,
        })
    }

    fn list_messages(
        &self,
        _before_delivery_id: Option<i64>,
        _limit: usize,
    ) -> Result<NewsPage, NewsStorageError> {
        self.record("list");
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
        self.record("mark");
        if let Some(gate) = self.mark_gate.lock().unwrap().take() {
            gate.enter_and_wait();
        }
        Ok(NewsUnreadSnapshot {
            latest_delivery_id: Some("9".into()),
            unread_count: 0,
        })
    }
}

#[derive(Default)]
pub(super) struct RecordingEvents {
    pub(super) committed: Mutex<Vec<NewsMessagesCommittedEvent>>,
    pub(super) statuses: Mutex<Vec<NewsStatusSnapshot>>,
    fail: AtomicBool,
    status_changed: Notify,
}

impl RecordingEvents {
    pub(super) fn set_fail(&self, fail: bool) {
        self.fail.store(fail, Ordering::SeqCst);
    }
}

impl NewsEventSink for RecordingEvents {
    fn emit_messages_committed(
        &self,
        event: &NewsMessagesCommittedEvent,
    ) -> Result<(), NewsEventError> {
        self.committed.lock().unwrap().push(event.clone());
        if self.fail.load(Ordering::SeqCst) {
            Err(NewsEventError)
        } else {
            Ok(())
        }
    }

    fn emit_status_changed(&self, status: &NewsStatusSnapshot) -> Result<(), NewsEventError> {
        self.statuses.lock().unwrap().push(status.clone());
        self.status_changed.notify_waiters();
        if self.fail.load(Ordering::SeqCst) {
            Err(NewsEventError)
        } else {
            Ok(())
        }
    }
}

pub(super) struct Rig {
    pub(super) service: Arc<NewsService>,
    pub(super) repository: Arc<FakeRepository>,
    pub(super) fetcher: Arc<ScriptedFetcher>,
    pub(super) tokens: Arc<FakeTokenStore>,
    pub(super) events: Arc<RecordingEvents>,
}

impl Rig {
    pub(super) fn new(token: TokenOutcome) -> Self {
        let repository = Arc::new(FakeRepository::default());
        let fetcher = Arc::new(ScriptedFetcher::default());
        let tokens = Arc::new(FakeTokenStore::new(token));
        let events = Arc::new(RecordingEvents::default());
        let service = Arc::new(NewsService::new(
            "fingerprint-a".into(),
            repository.clone(),
            fetcher.clone(),
            tokens.clone(),
            events.clone(),
        ));
        Self {
            service,
            repository,
            fetcher,
            tokens,
            events,
        }
    }

    pub(super) async fn wait_for_calls(&self, count: usize) {
        for _ in 0..10_000 {
            if self.fetcher.calls().len() >= count {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("fetch call count did not reach {count}");
    }

    pub(super) async fn wait_for_status(&self, kind: NewsStatusKind) {
        loop {
            let notified = self.events.status_changed.notified();
            if self.service.status().kind == kind {
                return;
            }
            notified.await;
        }
    }
}

pub(super) fn page(ids: &[i64], has_more: bool) -> ValidatedNewsPage {
    ValidatedNewsPage {
        items: ids
            .iter()
            .map(|id| ValidatedNewsMessage {
                delivery_id: *id,
                created_at_ms: 1_700_000_000_000 + id,
                text: format!("message-{id}"),
            })
            .collect(),
        next_cursor: ids.last().copied().unwrap_or(0),
        has_more,
    }
}

pub(super) fn empty_page(cursor: i64) -> ValidatedNewsPage {
    ValidatedNewsPage {
        items: Vec::new(),
        next_cursor: cursor,
        has_more: false,
    }
}

fn empty_state() -> NewsDatabaseState {
    NewsDatabaseState {
        cursor: 0,
        last_seen_delivery_id: 0,
        initial_sync_complete: false,
        synced_count: 0,
        latest_delivery_id: None,
        unread_count: 0,
    }
}
