use std::sync::Arc;

use crate::models::news::{NewsPage, NewsUnreadSnapshot, ValidatedNewsPage};
use crate::storage::{
    NewsApiToken, NewsCommitOutcome, NewsDatabaseState, NewsStorageError, NewsTokenStore,
    NewsTokenStoreError,
};

use super::ports::{NewsRepository, NewsRepositoryFactory, NewsTokenStoreFactory};

pub(super) async fn open_repository(
    factory: Arc<NewsRepositoryFactory>,
) -> Result<Arc<dyn NewsRepository>, NewsStorageError> {
    tokio::task::spawn_blocking(move || factory())
        .await
        .unwrap_or_else(|_| Err(NewsStorageError::database()))
}

pub(super) async fn build_token_store(
    factory: Arc<NewsTokenStoreFactory>,
) -> Result<Arc<dyn NewsTokenStore>, NewsTokenStoreError> {
    tokio::task::spawn_blocking(move || factory())
        .await
        .map_err(|_| NewsTokenStoreError::Keyring)
}

pub(super) async fn prepare_source(
    repository: Arc<dyn NewsRepository>,
    fingerprint: String,
) -> Result<(), NewsStorageError> {
    tokio::task::spawn_blocking(move || repository.prepare_source(&fingerprint))
        .await
        .unwrap_or_else(|_| Err(NewsStorageError::database()))
}

pub(super) async fn state_snapshot(
    repository: Arc<dyn NewsRepository>,
) -> Result<NewsDatabaseState, NewsStorageError> {
    tokio::task::spawn_blocking(move || repository.state_snapshot())
        .await
        .unwrap_or_else(|_| Err(NewsStorageError::database()))
}

pub(super) async fn commit_page(
    repository: Arc<dyn NewsRepository>,
    page: ValidatedNewsPage,
    received_at_ms: i64,
) -> Result<NewsCommitOutcome, NewsStorageError> {
    tokio::task::spawn_blocking(move || repository.commit_page(&page, received_at_ms))
        .await
        .unwrap_or_else(|_| Err(NewsStorageError::database()))
}

pub(super) async fn list_messages(
    repository: Arc<dyn NewsRepository>,
    before_id: Option<i64>,
    limit: usize,
) -> Result<NewsPage, NewsStorageError> {
    tokio::task::spawn_blocking(move || repository.list_messages(before_id, limit))
        .await
        .unwrap_or_else(|_| Err(NewsStorageError::database()))
}

pub(super) async fn mark_seen(
    repository: Arc<dyn NewsRepository>,
    through_id: i64,
) -> Result<NewsUnreadSnapshot, NewsStorageError> {
    tokio::task::spawn_blocking(move || repository.mark_seen(through_id))
        .await
        .unwrap_or_else(|_| Err(NewsStorageError::database()))
}

pub(super) async fn load_token(
    token_store: Arc<dyn NewsTokenStore>,
) -> Result<Option<NewsApiToken>, NewsTokenStoreError> {
    tokio::task::spawn_blocking(move || token_store.load())
        .await
        .unwrap_or(Err(NewsTokenStoreError::Keyring))
}
