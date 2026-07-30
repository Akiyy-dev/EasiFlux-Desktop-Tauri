use std::cmp::Ordering;
use std::sync::Arc;

use crate::models::news::{NewsPage, NewsStatusKind, NewsStatusSnapshot, NewsUnreadSnapshot};
use crate::storage::NewsTokenStore;

use super::control::ManualAction;
use super::ports::NewsRepository;
use super::{blocking, poller, status, NewsService, NewsServiceError};

fn latest_delivery_id(value: Option<&str>) -> Option<i64> {
    value.and_then(|id| id.parse::<i64>().ok())
}

fn merge_progress(current: &NewsStatusSnapshot, incoming: &mut NewsStatusSnapshot) {
    incoming.initial_sync_complete |= current.initial_sync_complete;
    incoming.synced_count = incoming.synced_count.max(current.synced_count);
    match latest_delivery_id(incoming.latest_delivery_id.as_deref())
        .cmp(&latest_delivery_id(current.latest_delivery_id.as_deref()))
    {
        Ordering::Less => {
            incoming.latest_delivery_id = current.latest_delivery_id.clone();
            incoming.unread_count = current.unread_count.min(100);
        }
        Ordering::Equal => {
            incoming.unread_count = incoming.unread_count.min(current.unread_count).min(100);
        }
        Ordering::Greater => incoming.unread_count = incoming.unread_count.min(100),
    }
}

impl NewsService {
    pub fn start(self: &Arc<Self>) {
        if *self.cancellation.borrow() {
            return;
        }
        if self.repository().is_none() && self.repository_factory.is_none() {
            return;
        }
        let mut task = self.task.lock().unwrap();
        if task.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return;
        }
        *task = Some(tokio::spawn(poller::run(self.clone())));
    }

    pub fn status(&self) -> NewsStatusSnapshot {
        self.status.lock().unwrap().clone()
    }

    pub async fn list_messages(
        &self,
        before_id: Option<i64>,
        limit: usize,
    ) -> Result<NewsPage, NewsServiceError> {
        let repository = self
            .repository()
            .ok_or_else(NewsServiceError::unavailable)?;
        blocking::list_messages(repository, before_id, limit)
            .await
            .map_err(|_| NewsServiceError::repository())
    }

    pub async fn mark_seen(&self, through_id: i64) -> Result<NewsUnreadSnapshot, NewsServiceError> {
        let repository = self
            .repository()
            .ok_or_else(NewsServiceError::unavailable)?;
        let unread = blocking::mark_seen(repository, through_id)
            .await
            .map_err(|_| NewsServiceError::repository())?;
        let mut current = self.status.lock().unwrap();
        let response_latest = latest_delivery_id(unread.latest_delivery_id.as_deref());
        let current_latest = latest_delivery_id(current.latest_delivery_id.as_deref());
        if response_latest >= current_latest {
            current.latest_delivery_id = unread.latest_delivery_id.clone();
            current.unread_count = unread.unread_count.min(100);
        }
        drop(current);
        Ok(unread)
    }

    pub fn recheck_credentials(&self) -> NewsStatusSnapshot {
        self.signal(ManualAction::RecheckCredentials)
    }

    pub fn retry_sync(&self) -> NewsStatusSnapshot {
        self.signal(ManualAction::RetrySync)
    }

    fn signal(&self, action: ManualAction) -> NewsStatusSnapshot {
        self.manual.send_modify(|signal| signal.advance(action));
        self.status()
    }

    pub(super) fn publish(&self, mut snapshot: NewsStatusSnapshot) {
        let changed = {
            let mut current = self.status.lock().unwrap();
            merge_progress(&current, &mut snapshot);
            if *current == snapshot {
                false
            } else {
                *current = snapshot.clone();
                true
            }
        };
        if changed {
            let _ = self.event_sink.emit_status_changed(&snapshot);
        }
    }

    pub(super) fn publish_from_current(&self, kind: NewsStatusKind, retry_at: Option<String>) {
        let current = self.status();
        self.publish(status::from_snapshot(kind, &current, retry_at));
    }

    pub(super) fn repository(&self) -> Option<Arc<dyn NewsRepository>> {
        self.repository.lock().unwrap().clone()
    }

    pub(super) async fn open_repository(&self) -> Result<Arc<dyn NewsRepository>, ()> {
        if let Some(repository) = self.repository() {
            return Ok(repository);
        }
        let factory = self.repository_factory.clone().ok_or(())?;
        let repository = blocking::open_repository(factory).await.map_err(|_| ())?;
        let mut current = self.repository.lock().unwrap();
        Ok(current.get_or_insert_with(|| repository.clone()).clone())
    }

    pub(super) fn invalidate_repository(&self) {
        if self.repository_factory.is_some() {
            *self.repository.lock().unwrap() = None;
        }
    }

    pub(super) async fn token_store(&self) -> Result<Arc<dyn NewsTokenStore>, ()> {
        if let Some(token_store) = &self.token_store {
            return Ok(token_store.clone());
        }
        let factory = self.token_store_factory.clone().ok_or(())?;
        blocking::build_token_store(factory).await.map_err(|_| ())
    }
}
