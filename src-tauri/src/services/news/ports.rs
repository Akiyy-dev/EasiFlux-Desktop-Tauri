use std::fmt;
use std::sync::Arc;

use crate::models::news::{
    NewsMessagesCommittedEvent, NewsPage, NewsStatusSnapshot, NewsUnreadSnapshot, ValidatedNewsPage,
};
use crate::storage::{
    NewsCommitOutcome, NewsDatabase, NewsDatabaseState, NewsStorageError, NewsTokenStore,
};

pub(crate) type NewsRepositoryFactory =
    dyn Fn() -> Result<Arc<dyn NewsRepository>, NewsStorageError> + Send + Sync;
pub(crate) type NewsTokenStoreFactory = dyn Fn() -> Arc<dyn NewsTokenStore> + Send + Sync;

pub(crate) trait NewsRepository: Send + Sync {
    fn prepare_source(&self, fingerprint: &str) -> Result<(), NewsStorageError>;
    fn state_snapshot(&self) -> Result<NewsDatabaseState, NewsStorageError>;
    fn commit_page(
        &self,
        page: &ValidatedNewsPage,
        received_at_ms: i64,
    ) -> Result<NewsCommitOutcome, NewsStorageError>;
    fn list_messages(
        &self,
        before_delivery_id: Option<i64>,
        limit: usize,
    ) -> Result<NewsPage, NewsStorageError>;
    fn mark_seen(&self, through_delivery_id: i64) -> Result<NewsUnreadSnapshot, NewsStorageError>;
}

impl NewsRepository for NewsDatabase {
    fn prepare_source(&self, fingerprint: &str) -> Result<(), NewsStorageError> {
        NewsDatabase::prepare_source(self, fingerprint)
    }

    fn state_snapshot(&self) -> Result<NewsDatabaseState, NewsStorageError> {
        NewsDatabase::state_snapshot(self)
    }

    fn commit_page(
        &self,
        page: &ValidatedNewsPage,
        received_at_ms: i64,
    ) -> Result<NewsCommitOutcome, NewsStorageError> {
        NewsDatabase::commit_page(self, page, received_at_ms)
    }

    fn list_messages(
        &self,
        before_delivery_id: Option<i64>,
        limit: usize,
    ) -> Result<NewsPage, NewsStorageError> {
        NewsDatabase::list_messages(self, before_delivery_id, limit)
    }

    fn mark_seen(&self, through_delivery_id: i64) -> Result<NewsUnreadSnapshot, NewsStorageError> {
        NewsDatabase::mark_seen(self, through_delivery_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NewsEventError;

impl fmt::Display for NewsEventError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("news event emission failed")
    }
}

impl std::error::Error for NewsEventError {}

pub(crate) trait NewsEventSink: Send + Sync {
    fn emit_messages_committed(
        &self,
        event: &NewsMessagesCommittedEvent,
    ) -> Result<(), NewsEventError>;
    fn emit_status_changed(&self, status: &NewsStatusSnapshot) -> Result<(), NewsEventError>;
}
