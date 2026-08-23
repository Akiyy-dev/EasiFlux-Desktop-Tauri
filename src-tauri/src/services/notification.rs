use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::models::notification::{
    ListNotificationsRequest, NotificationChange, NotificationChangedEvent, NotificationChannel,
    NotificationEnvironment, NotificationFilter, NotificationInput, NotificationKind,
    NotificationMutationResult, NotificationPage, NotificationRecord, NotificationScope,
    NotificationSummary, NotificationToastCandidate, MAX_JAVASCRIPT_SAFE_INTEGER,
};
use crate::storage::notification_store::{
    NotificationFileV1, NotificationLoadStatus, NotificationPartition, NotificationPersistence,
    NotificationSourceEventIndexEntry, NotificationStore,
};

pub mod policy;

pub use policy::NotificationPolicy;

const NOTIFICATION_STORAGE_UNAVAILABLE: &str = "NOTIFICATION_STORAGE_UNAVAILABLE";
const NOTIFICATION_SCOPE_MISMATCH: &str = "NOTIFICATION_SCOPE_MISMATCH";
const NOTIFICATION_NOT_FOUND: &str = "NOTIFICATION_NOT_FOUND";
const INVALID_NOTIFICATION_CURSOR: &str = "INVALID_NOTIFICATION_CURSOR";
const MAINTENANCE_INTERVAL_MS: u64 = 24 * 60 * 60 * 1_000;

/// Synchronous, non-reentrant commit callback.
///
/// The adapter may not call back into `NotificationService`: it runs after the
/// disk save and in-memory swap, while the service's serialization gate is held.
/// Its error is diagnostic-only and never rolls back the committed mutation.
pub type NotificationEmitter =
    Arc<dyn Fn(&NotificationChangedEvent) -> Result<(), String> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationError {
    code: &'static str,
    message: &'static str,
}

impl NotificationError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl Display for NotificationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message)
    }
}

impl Error for NotificationError {}

#[derive(Debug, Clone, PartialEq)]
pub struct PublishOutcome {
    pub notification: Option<NotificationRecord>,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneOutcome {
    pub affected_count: u64,
    pub affected_scopes: Vec<NotificationScope>,
    pub revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityState {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionObservation {
    pub account_id: String,
    pub session_epoch: u64,
    pub channel: NotificationChannel,
    pub state: AvailabilityState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentObservation {
    pub account_id: String,
    pub session_epoch: u64,
    pub environment: NotificationEnvironment,
    pub state: AvailabilityState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderObservationOrigin {
    Command,
    Realtime,
    Snapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObservedOrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
}

impl ObservedOrderStatus {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Filled | Self::Canceled | Self::Rejected)
    }

    fn name(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::PartiallyFilled => "partially-filled",
            Self::Filled => "filled",
            Self::Canceled => "canceled",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderObservation {
    pub account_id: String,
    pub session_epoch: u64,
    pub order_id: Option<String>,
    pub submission_id: Option<String>,
    pub status: ObservedOrderStatus,
    pub origin: OrderObservationOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewContext {
    account_id: Option<String>,
}

impl ViewContext {
    pub fn global() -> Self {
        Self { account_id: None }
    }

    pub fn account(account_id: impl Into<String>) -> Result<Self, NotificationError> {
        let account_id = account_id.into();
        NotificationScope::Account {
            account_id: account_id.clone(),
        }
        .validate()
        .map_err(|error| NotificationError::new(error.code(), "通知账户范围无效"))?;
        Ok(Self {
            account_id: Some(account_id),
        })
    }

    fn visible(&self, scope: &NotificationScope) -> bool {
        match scope {
            NotificationScope::Global => true,
            NotificationScope::Account { account_id } => {
                self.account_id.as_deref() == Some(account_id.as_str())
            }
        }
    }
}

#[derive(Clone)]
struct ServiceState {
    file: NotificationFileV1,
    seen_source_events: HashSet<(NotificationScope, String)>,
    last_pruned_at_ms: u64,
    active_incidents: HashMap<IncidentKey, ActiveIncident>,
    order_states: HashMap<OrderKey, ObservedOrderState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum IncidentKey {
    Connection(String, NotificationChannel),
    Environment(String, NotificationEnvironment),
}

#[derive(Debug, Clone)]
struct ActiveIncident {
    id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OrderKey {
    account_id: String,
    entity_id: String,
}

#[derive(Debug, Clone, Copy)]
struct ObservedOrderState {
    status: ObservedOrderStatus,
    terminal_notified: bool,
}

pub struct NotificationService {
    state: tokio::sync::Mutex<ServiceState>,
    persistence: Arc<dyn NotificationPersistence>,
    emit_changed: NotificationEmitter,
}

impl NotificationService {
    pub(crate) fn from_snapshot<P>(
        mut file: NotificationFileV1,
        persistence: Arc<P>,
        emit_changed: NotificationEmitter,
        last_pruned_at_ms: u64,
    ) -> Self
    where
        P: NotificationPersistence + 'static,
    {
        ensure_source_event_index(&mut file);
        let seen_source_events = rebuild_seen_source_events(&file);
        let active_incidents = rebuild_active_incidents(&file);
        Self {
            state: tokio::sync::Mutex::new(ServiceState {
                file,
                seen_source_events,
                last_pruned_at_ms,
                active_incidents,
                order_states: HashMap::new(),
            }),
            persistence,
            emit_changed,
        }
    }

    pub fn load(
        store: NotificationStore,
        configured_accounts: &[String],
        now_ms: u64,
        emit_changed: NotificationEmitter,
    ) -> Result<Self, NotificationError> {
        let outcome = store.load().map_err(map_persistence_error)?;
        if let NotificationLoadStatus::UnsupportedSchema { .. } = outcome.status {
            return Err(NotificationError::new(
                "UNSUPPORTED_NOTIFICATION_SCHEMA",
                "通知存储版本暂不支持",
            ));
        }
        let mut file = outcome.file;
        ensure_source_event_index(&mut file);
        let configured: HashSet<_> = configured_accounts.iter().cloned().collect();
        let (affected_count, affected_scopes) = prune_file(&mut file, Some(&configured), now_ms);
        let previous_revision = file.revision;
        if affected_count > 0 {
            file.revision = file.revision.checked_add(1).ok_or_else(|| {
                NotificationError::new("NOTIFICATION_REVISION_EXHAUSTED", "通知修订号已达上限")
            })?;
            store.save(&file).map_err(map_persistence_error)?;
        }
        let service = Self::from_snapshot(file, Arc::new(store), emit_changed, now_ms);
        if affected_count > 0 {
            let event = NotificationChangedEvent {
                previous_revision: previous_revision.to_string(),
                revision: (previous_revision + 1).to_string(),
                change: NotificationChange::Reset,
                affected_scopes,
                notification_id: None,
                toast_candidate: None,
            };
            service.emit_diagnostic_only(&event);
        }
        Ok(service)
    }

    pub async fn revision(&self) -> String {
        self.state.lock().await.file.revision.to_string()
    }

    pub async fn publish(
        &self,
        input: NotificationInput,
        now_ms: u64,
    ) -> Result<PublishOutcome, NotificationError> {
        let mut guard = self.state.lock().await;
        let mut next = guard.clone();
        let mutation = apply_publish(&mut next, input, now_ms)?;
        self.commit_publish(&mut guard, next, mutation)
    }

    pub async fn list(
        &self,
        context: ViewContext,
        request: ListNotificationsRequest,
        now_ms: u64,
    ) -> Result<NotificationPage, NotificationError> {
        request
            .validate()
            .map_err(|error| NotificationError::new(error.code(), "通知查询请求无效"))?;
        if request.account_id != context.account_id {
            return Err(scope_mismatch());
        }
        let fingerprint = query_fingerprint(&context);
        let cursor = request
            .cursor
            .as_deref()
            .map(|cursor| decode_cursor(cursor, &fingerprint, request.filter))
            .transpose()?;
        let guard = self.state.lock().await;
        let mut items: Vec<_> = guard
            .file
            .partitions
            .iter()
            .filter(|partition| context.visible(&partition.scope))
            .flat_map(|partition| partition.items.iter().cloned())
            .filter(|record| !is_expired(record, now_ms))
            .filter(|record| {
                request.filter == NotificationFilter::All || record.read_at_ms.is_none()
            })
            .filter(|record| {
                cursor.as_ref().is_none_or(|cursor| {
                    (record.created_at_ms, record.id.as_str())
                        < (cursor.created_at_ms, cursor.id.as_str())
                })
            })
            .collect();
        items.sort_by(|left, right| {
            right
                .created_at_ms
                .cmp(&left.created_at_ms)
                .then_with(|| right.id.cmp(&left.id))
        });
        let unread_count = visible_unread_count(&guard.file, &context, now_ms);
        let has_more = items.len() > request.limit as usize;
        items.truncate(request.limit as usize);
        let next_cursor = if has_more {
            items.last().map(|record| {
                encode_cursor(CursorV1 {
                    version: 1,
                    fingerprint: fingerprint.clone(),
                    filter: request.filter,
                    created_at_ms: record.created_at_ms,
                    id: record.id.clone(),
                })
            })
        } else {
            None
        };
        Ok(NotificationPage {
            items,
            next_cursor,
            unread_count,
            revision: guard.file.revision.to_string(),
        })
    }

    pub async fn summary(
        &self,
        context: ViewContext,
        now_ms: u64,
    ) -> Result<NotificationSummary, NotificationError> {
        let guard = self.state.lock().await;
        Ok(NotificationSummary {
            unread_count: visible_unread_count(&guard.file, &context, now_ms),
            revision: guard.file.revision.to_string(),
        })
    }

    pub async fn mark_read(
        &self,
        context: ViewContext,
        id: &str,
        now_ms: u64,
    ) -> Result<NotificationMutationResult, NotificationError> {
        let mut guard = self.state.lock().await;
        let mut next = guard.clone();
        let Some((scope, record)) = find_record_mut(&mut next.file, id) else {
            return Err(NotificationError::new(NOTIFICATION_NOT_FOUND, "通知不存在"));
        };
        if !context.visible(&scope) {
            return Err(scope_mismatch());
        }
        if record.read_at_ms.is_some() {
            return Ok(NotificationMutationResult {
                notification: Some(record.clone()),
                affected_count: 0,
                affected_scopes: vec![],
                unread_count: visible_unread_count(&guard.file, &context, now_ms),
                revision: guard.file.revision.to_string(),
            });
        }
        record.read_at_ms = Some(now_ms.max(record.created_at_ms));
        record.updated_at_ms = record.updated_at_ms.max(now_ms);
        let changed = record.clone();
        let previous_revision = commit_file(&self.persistence, &mut guard, next)?;
        let event = NotificationChangedEvent {
            previous_revision: previous_revision.to_string(),
            revision: guard.file.revision.to_string(),
            change: NotificationChange::Updated,
            affected_scopes: vec![scope.clone()],
            notification_id: Some(id.into()),
            toast_candidate: None,
        };
        self.emit_diagnostic_only(&event);
        Ok(NotificationMutationResult {
            notification: Some(changed),
            affected_count: 1,
            affected_scopes: vec![scope],
            unread_count: visible_unread_count(&guard.file, &context, now_ms),
            revision: guard.file.revision.to_string(),
        })
    }

    pub async fn mark_visible_read(
        &self,
        context: ViewContext,
        now_ms: u64,
    ) -> Result<NotificationMutationResult, NotificationError> {
        let mut guard = self.state.lock().await;
        let mut next = guard.clone();
        let mut affected_count = 0_u64;
        let mut affected_scopes = Vec::new();
        for partition in &mut next.file.partitions {
            if !context.visible(&partition.scope) {
                continue;
            }
            let mut partition_changed = false;
            for record in &mut partition.items {
                if record.read_at_ms.is_none() && !is_expired(record, now_ms) {
                    record.read_at_ms = Some(now_ms.max(record.created_at_ms));
                    record.updated_at_ms = record.updated_at_ms.max(now_ms);
                    affected_count += 1;
                    partition_changed = true;
                }
            }
            if partition_changed {
                affected_scopes.push(partition.scope.clone());
            }
        }
        if affected_count == 0 {
            return Ok(NotificationMutationResult {
                notification: None,
                affected_count: 0,
                affected_scopes,
                unread_count: visible_unread_count(&guard.file, &context, now_ms),
                revision: guard.file.revision.to_string(),
            });
        }
        let previous_revision = commit_file(&self.persistence, &mut guard, next)?;
        let event = NotificationChangedEvent {
            previous_revision: previous_revision.to_string(),
            revision: guard.file.revision.to_string(),
            change: NotificationChange::Updated,
            affected_scopes: affected_scopes.clone(),
            notification_id: None,
            toast_candidate: None,
        };
        self.emit_diagnostic_only(&event);
        Ok(NotificationMutationResult {
            notification: None,
            affected_count,
            affected_scopes,
            unread_count: visible_unread_count(&guard.file, &context, now_ms),
            revision: guard.file.revision.to_string(),
        })
    }

    pub async fn delete_visible(
        &self,
        context: ViewContext,
        id: &str,
        now_ms: u64,
    ) -> Result<NotificationMutationResult, NotificationError> {
        let mut guard = self.state.lock().await;
        let mut next = guard.clone();
        let Some((partition_index, item_index)) = find_record_index(&next.file, id) else {
            return Err(NotificationError::new(NOTIFICATION_NOT_FOUND, "通知不存在"));
        };
        let scope = next.file.partitions[partition_index].scope.clone();
        if !context.visible(&scope) {
            return Err(scope_mismatch());
        }
        next.file.partitions[partition_index]
            .items
            .remove(item_index);
        remove_empty_partitions(&mut next.file);
        cleanup_source_event_index(&mut next.file);
        next.seen_source_events = rebuild_seen_source_events(&next.file);
        let previous_revision = commit_file(&self.persistence, &mut guard, next)?;
        let event = NotificationChangedEvent {
            previous_revision: previous_revision.to_string(),
            revision: guard.file.revision.to_string(),
            change: NotificationChange::Removed,
            affected_scopes: vec![scope.clone()],
            notification_id: Some(id.into()),
            toast_candidate: None,
        };
        self.emit_diagnostic_only(&event);
        Ok(NotificationMutationResult {
            notification: None,
            affected_count: 1,
            affected_scopes: vec![scope],
            unread_count: visible_unread_count(&guard.file, &context, now_ms),
            revision: guard.file.revision.to_string(),
        })
    }

    pub async fn clear_account(
        &self,
        active_account_id: &str,
        requested_account_id: &str,
        now_ms: u64,
    ) -> Result<NotificationMutationResult, NotificationError> {
        let context = ViewContext::account(active_account_id)?;
        if active_account_id != requested_account_id {
            return Err(scope_mismatch());
        }
        let scope = NotificationScope::Account {
            account_id: requested_account_id.into(),
        };
        let mut guard = self.state.lock().await;
        let mut next = guard.clone();
        let affected_count = next
            .file
            .partitions
            .iter()
            .find(|partition| partition.scope == scope)
            .map_or(0, |partition| partition.items.len() as u64);
        if affected_count == 0 {
            return Ok(NotificationMutationResult {
                notification: None,
                affected_count: 0,
                affected_scopes: Vec::new(),
                unread_count: visible_unread_count(&guard.file, &context, now_ms),
                revision: guard.file.revision.to_string(),
            });
        }
        next.file
            .partitions
            .retain(|partition| partition.scope != scope);
        cleanup_source_event_index(&mut next.file);
        next.seen_source_events = rebuild_seen_source_events(&next.file);
        let previous_revision = commit_file(&self.persistence, &mut guard, next)?;
        let event = NotificationChangedEvent {
            previous_revision: previous_revision.to_string(),
            revision: guard.file.revision.to_string(),
            change: NotificationChange::Reset,
            affected_scopes: vec![scope.clone()],
            notification_id: None,
            toast_candidate: None,
        };
        self.emit_diagnostic_only(&event);
        Ok(NotificationMutationResult {
            notification: None,
            affected_count,
            affected_scopes: vec![scope],
            unread_count: visible_unread_count(&guard.file, &context, now_ms),
            revision: guard.file.revision.to_string(),
        })
    }

    pub async fn maintenance_due(&self, now_ms: u64) -> bool {
        let guard = self.state.lock().await;
        now_ms.saturating_sub(guard.last_pruned_at_ms) >= MAINTENANCE_INTERVAL_MS
    }

    pub async fn prune(
        &self,
        configured_accounts: &HashSet<String>,
        now_ms: u64,
    ) -> Result<PruneOutcome, NotificationError> {
        let mut guard = self.state.lock().await;
        let mut next = guard.clone();
        next.last_pruned_at_ms = now_ms;
        let (affected_count, affected_scopes) =
            prune_file(&mut next.file, Some(configured_accounts), now_ms);
        next.seen_source_events = rebuild_seen_source_events(&next.file);
        next.active_incidents = rebuild_active_incidents(&next.file);
        next.order_states
            .retain(|key, _| configured_accounts.contains(&key.account_id));
        if affected_count == 0 {
            *guard = next;
            return Ok(PruneOutcome {
                affected_count: 0,
                affected_scopes,
                revision: guard.file.revision.to_string(),
            });
        }
        let previous_revision = commit_file(&self.persistence, &mut guard, next)?;
        let event = NotificationChangedEvent {
            previous_revision: previous_revision.to_string(),
            revision: guard.file.revision.to_string(),
            change: NotificationChange::Reset,
            affected_scopes: affected_scopes.clone(),
            notification_id: None,
            toast_candidate: None,
        };
        self.emit_diagnostic_only(&event);
        Ok(PruneOutcome {
            affected_count,
            affected_scopes,
            revision: guard.file.revision.to_string(),
        })
    }

    pub async fn delete_account_partition(
        &self,
        account_id: &str,
        _now_ms: u64,
    ) -> Result<(), NotificationError> {
        let scope = ViewContext::account(account_id).map(|_| NotificationScope::Account {
            account_id: account_id.into(),
        })?;
        let mut guard = self.state.lock().await;
        let mut next = guard.clone();
        let affected_count = next
            .file
            .partitions
            .iter()
            .find(|partition| partition.scope == scope)
            .map_or(0, |partition| partition.items.len());
        next.file
            .partitions
            .retain(|partition| partition.scope != scope);
        cleanup_source_event_index(&mut next.file);
        next.seen_source_events = rebuild_seen_source_events(&next.file);
        next.active_incidents.retain(|key, _| match key {
            IncidentKey::Connection(owner, _) | IncidentKey::Environment(owner, _) => {
                owner != account_id
            }
        });
        next.order_states
            .retain(|key, _| key.account_id != account_id);
        if affected_count == 0 {
            *guard = next;
            return Ok(());
        }
        let previous_revision = commit_file(&self.persistence, &mut guard, next)?;
        let event = NotificationChangedEvent {
            previous_revision: previous_revision.to_string(),
            revision: guard.file.revision.to_string(),
            change: NotificationChange::Reset,
            affected_scopes: vec![scope],
            notification_id: None,
            toast_candidate: None,
        };
        self.emit_diagnostic_only(&event);
        Ok(())
    }

    pub async fn observe_connection(
        &self,
        observation: ConnectionObservation,
        now_ms: u64,
    ) -> Result<PublishOutcome, NotificationError> {
        ViewContext::account(&observation.account_id)?;
        let key = IncidentKey::Connection(observation.account_id.clone(), observation.channel);
        let mut guard = self.state.lock().await;
        let mut next = guard.clone();
        let policy = NotificationPolicy;
        let input = match observation.state {
            AvailabilityState::Unavailable => {
                if next.active_incidents.contains_key(&key) {
                    *guard = next;
                    return Ok(no_publish_outcome(&guard));
                }
                let incident_id = uuid::Uuid::new_v4().to_string();
                let context = policy::PolicyContext::new(
                    observation.account_id,
                    observation.session_epoch,
                    format!(
                        "connection:{}:{incident_id}:unavailable",
                        channel_tag(observation.channel)
                    ),
                )?;
                let input =
                    policy.connection_unavailable(context, observation.channel, &incident_id)?;
                next.active_incidents
                    .insert(key, ActiveIncident { id: incident_id });
                input
            }
            AvailabilityState::Available => {
                let Some(incident) = next.active_incidents.remove(&key) else {
                    *guard = next;
                    return Ok(no_publish_outcome(&guard));
                };
                let context = policy::PolicyContext::new(
                    observation.account_id,
                    observation.session_epoch,
                    format!(
                        "connection:{}:{}:recovered",
                        channel_tag(observation.channel),
                        incident.id
                    ),
                )?;
                policy.connection_recovered(context, observation.channel, &incident.id)?
            }
        };
        let mutation = apply_publish(&mut next, input, now_ms)?;
        self.commit_publish(&mut guard, next, mutation)
    }

    pub async fn observe_environment(
        &self,
        observation: EnvironmentObservation,
        now_ms: u64,
    ) -> Result<PublishOutcome, NotificationError> {
        ViewContext::account(&observation.account_id)?;
        let key = IncidentKey::Environment(observation.account_id.clone(), observation.environment);
        let mut guard = self.state.lock().await;
        let mut next = guard.clone();
        let policy = NotificationPolicy;
        let input = match observation.state {
            AvailabilityState::Unavailable => {
                if next.active_incidents.contains_key(&key) {
                    *guard = next;
                    return Ok(no_publish_outcome(&guard));
                }
                let incident_id = uuid::Uuid::new_v4().to_string();
                let context = policy::PolicyContext::new(
                    observation.account_id,
                    observation.session_epoch,
                    format!(
                        "environment:{}:{incident_id}:unavailable",
                        environment_tag(observation.environment)
                    ),
                )?;
                let input = policy.environment_unavailable(
                    context,
                    observation.environment,
                    &incident_id,
                )?;
                next.active_incidents
                    .insert(key, ActiveIncident { id: incident_id });
                input
            }
            AvailabilityState::Available => {
                let Some(incident) = next.active_incidents.remove(&key) else {
                    *guard = next;
                    return Ok(no_publish_outcome(&guard));
                };
                let context = policy::PolicyContext::new(
                    observation.account_id,
                    observation.session_epoch,
                    format!(
                        "environment:{}:{}:recovered",
                        environment_tag(observation.environment),
                        incident.id
                    ),
                )?;
                policy.environment_recovered(context, observation.environment, &incident.id)?
            }
        };
        let mutation = apply_publish(&mut next, input, now_ms)?;
        self.commit_publish(&mut guard, next, mutation)
    }

    pub async fn observe_order(
        &self,
        observation: OrderObservation,
        now_ms: u64,
    ) -> Result<PublishOutcome, NotificationError> {
        ViewContext::account(&observation.account_id)?;
        let entity_id = observation
            .order_id
            .as_ref()
            .or(observation.submission_id.as_ref())
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .ok_or_else(|| {
                NotificationError::new("INVALID_NOTIFICATION_CONTENT", "订单观察标识无效")
            })?;
        let key = OrderKey {
            account_id: observation.account_id.clone(),
            entity_id: entity_id.clone(),
        };
        let mut guard = self.state.lock().await;
        let mut next = guard.clone();
        let previous = next.order_states.get(&key).copied();
        if previous.is_some_and(|state| state.status.is_terminal())
            && observation.status.is_terminal()
            && previous.map(|state| state.status) != Some(observation.status)
        {
            tracing::warn!(
                previous_status = previous.expect("terminal status was checked").status.name(),
                observed_status = observation.status.name(),
                "ignored inconsistent terminal order transition"
            );
        }
        let already_notified = previous.is_some_and(|state| state.terminal_notified);
        let should_publish = observation.status.is_terminal()
            && !already_notified
            && match observation.origin {
                OrderObservationOrigin::Snapshot => false,
                OrderObservationOrigin::Command => true,
                OrderObservationOrigin::Realtime => {
                    previous.is_some_and(|state| !state.status.is_terminal())
                }
            };
        next.order_states.insert(
            key,
            ObservedOrderState {
                status: observation.status,
                terminal_notified: already_notified || should_publish,
            },
        );
        if !should_publish {
            *guard = next;
            return Ok(no_publish_outcome(&guard));
        }

        let context = policy::PolicyContext::new(
            observation.account_id,
            observation.session_epoch,
            format!("order:{entity_id}:{}", observation.status.name()),
        )?;
        let policy = NotificationPolicy;
        let input = match observation.status {
            ObservedOrderStatus::Filled => policy.order_filled(
                context,
                observation.order_id.as_deref().ok_or_else(|| {
                    NotificationError::new("INVALID_NOTIFICATION_CONTENT", "成交订单标识无效")
                })?,
            )?,
            ObservedOrderStatus::Canceled => policy.order_canceled(
                context,
                observation.order_id.as_deref().ok_or_else(|| {
                    NotificationError::new("INVALID_NOTIFICATION_CONTENT", "取消订单标识无效")
                })?,
            )?,
            ObservedOrderStatus::Rejected => policy.order_rejected(
                context,
                observation.order_id.as_deref(),
                observation.submission_id.as_deref().unwrap_or(&entity_id),
            )?,
            ObservedOrderStatus::New | ObservedOrderStatus::PartiallyFilled => unreachable!(),
        };
        let mutation = apply_publish(&mut next, input, now_ms)?;
        self.commit_publish(&mut guard, next, mutation)
    }

    fn commit_publish(
        &self,
        guard: &mut ServiceState,
        mut next: ServiceState,
        mutation: PublishMutation,
    ) -> Result<PublishOutcome, NotificationError> {
        let Some(record) = mutation.record else {
            *guard = next;
            return Ok(no_publish_outcome(guard));
        };
        let previous_revision = next.file.revision;
        next.file.revision = previous_revision.checked_add(1).ok_or_else(|| {
            NotificationError::new("NOTIFICATION_REVISION_EXHAUSTED", "通知修订号已达上限")
        })?;
        self.persistence
            .save(&next.file)
            .map_err(map_persistence_error)?;
        *guard = next;
        let event = NotificationChangedEvent {
            previous_revision: previous_revision.to_string(),
            revision: guard.file.revision.to_string(),
            change: mutation.change.expect("committed publish has a change"),
            affected_scopes: mutation.affected_scopes,
            notification_id: Some(record.id.clone()),
            toast_candidate: mutation.toast_candidate,
        };
        self.emit_diagnostic_only(&event);
        Ok(PublishOutcome {
            notification: Some(record),
            revision: guard.file.revision.to_string(),
        })
    }

    fn emit_diagnostic_only(&self, event: &NotificationChangedEvent) {
        if (self.emit_changed)(event).is_err() {
            tracing::warn!(
                revision = %event.revision,
                "notification change callback failed after commit"
            );
        }
    }
}

struct PublishMutation {
    record: Option<NotificationRecord>,
    change: Option<NotificationChange>,
    toast_candidate: Option<NotificationToastCandidate>,
    affected_scopes: Vec<NotificationScope>,
}

fn apply_publish(
    next: &mut ServiceState,
    input: NotificationInput,
    now_ms: u64,
) -> Result<PublishMutation, NotificationError> {
    input
        .validate()
        .map_err(|error| NotificationError::new(error.code(), "通知输入无效"))?;
    if input
        .session_epoch
        .is_some_and(|epoch| epoch > MAX_JAVASCRIPT_SAFE_INTEGER)
    {
        return Err(NotificationError::new(
            "INVALID_NOTIFICATION_CONTENT",
            "通知会话代次无效",
        ));
    }
    let source_key = input
        .source_event_id
        .as_ref()
        .map(|source| (input.scope.clone(), source.clone()));
    if source_key
        .as_ref()
        .is_some_and(|key| next.seen_source_events.contains(key))
    {
        return Ok(PublishMutation {
            record: None,
            change: None,
            toast_candidate: None,
            affected_scopes: Vec::new(),
        });
    }

    let (_, mut retention_scopes) = prune_file(&mut next.file, None, now_ms);
    next.seen_source_events = rebuild_seen_source_events(&next.file);

    let scope = input.scope.clone();
    let session_epoch = input.session_epoch;
    let partition = partition_mut(&mut next.file, &scope);
    let existing = partition
        .items
        .iter_mut()
        .find(|record| record.dedupe_key == input.dedupe_key);
    let (record, change, toast_candidate) = if let Some(record) = existing {
        record.category = input.category;
        record.kind = input.kind;
        record.severity = input.severity;
        record.content = input.content;
        record.entity = input.entity;
        record.action = input.action;
        record.occurrence_count = record.occurrence_count.checked_add(1).ok_or_else(|| {
            NotificationError::new("NOTIFICATION_OCCURRENCE_OVERFLOW", "通知出现次数已达上限")
        })?;
        record.updated_at_ms = now_ms.max(record.updated_at_ms);
        record
            .validate()
            .map_err(|error| NotificationError::new(error.code(), "通知记录无效"))?;
        (record.clone(), NotificationChange::Updated, None)
    } else {
        let record = NotificationRecord {
            id: uuid::Uuid::new_v4().to_string(),
            scope: scope.clone(),
            category: input.category,
            kind: input.kind,
            severity: input.severity,
            content: input.content,
            entity: input.entity,
            action: input.action,
            source_event_id: input.source_event_id.clone(),
            dedupe_key: input.dedupe_key,
            occurrence_count: 1,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            read_at_ms: None,
        };
        record
            .validate()
            .map_err(|error| NotificationError::new(error.code(), "通知记录无效"))?;
        let toast = NotificationToastCandidate {
            id: record.id.clone(),
            scope: record.scope.clone(),
            session_epoch,
            category: record.category,
            severity: record.severity,
            content: record.content.clone(),
            action: record.action.clone(),
        };
        partition.items.push(record.clone());
        (record, NotificationChange::Created, Some(toast))
    };
    if let Some((source_scope, source_event_id)) = source_key {
        next.file
            .source_event_index
            .push(NotificationSourceEventIndexEntry {
                scope: source_scope.clone(),
                source_event_id: source_event_id.clone(),
                notification_id: record.id.clone(),
            });
        next.seen_source_events
            .insert((source_scope, source_event_id));
    }
    if change == NotificationChange::Created {
        let (_, created_retention_scopes) = prune_file(&mut next.file, None, now_ms);
        retention_scopes.extend(created_retention_scopes);
        next.seen_source_events = rebuild_seen_source_events(&next.file);
    }
    let mut affected_scopes = vec![scope];
    for retention_scope in retention_scopes {
        if !affected_scopes.contains(&retention_scope) {
            affected_scopes.push(retention_scope);
        }
    }
    Ok(PublishMutation {
        record: Some(record),
        change: Some(change),
        toast_candidate,
        affected_scopes,
    })
}

fn no_publish_outcome(state: &ServiceState) -> PublishOutcome {
    PublishOutcome {
        notification: None,
        revision: state.file.revision.to_string(),
    }
}

fn prune_file(
    file: &mut NotificationFileV1,
    configured_accounts: Option<&HashSet<String>>,
    now_ms: u64,
) -> (u64, Vec<NotificationScope>) {
    let mut affected_count = 0_u64;
    let mut affected_scopes = Vec::new();
    for partition in &mut file.partitions {
        let original_len = partition.items.len();
        partition.items.retain(|record| !is_expired(record, now_ms));
        if partition.items.len() > 1_000 {
            partition.items.sort_by(|left, right| {
                right
                    .created_at_ms
                    .cmp(&left.created_at_ms)
                    .then_with(|| right.id.cmp(&left.id))
            });
            partition.items.truncate(1_000);
        }
        let removed = original_len - partition.items.len();
        if removed > 0 {
            affected_count += removed as u64;
            affected_scopes.push(partition.scope.clone());
        }
    }
    if let Some(configured_accounts) = configured_accounts {
        file.partitions.retain(|partition| {
            let is_orphan = match &partition.scope {
                NotificationScope::Global => false,
                NotificationScope::Account { account_id } => {
                    !configured_accounts.contains(account_id)
                }
            };
            if is_orphan {
                affected_count += partition.items.len() as u64;
                if !affected_scopes.contains(&partition.scope) {
                    affected_scopes.push(partition.scope.clone());
                }
            }
            !is_orphan
        });
    }
    file.partitions.retain(|partition| {
        !partition.items.is_empty() || !affected_scopes.contains(&partition.scope)
    });
    cleanup_source_event_index(file);
    (affected_count, affected_scopes)
}

fn rebuild_seen_source_events(file: &NotificationFileV1) -> HashSet<(NotificationScope, String)> {
    file.source_event_index
        .iter()
        .map(|entry| (entry.scope.clone(), entry.source_event_id.clone()))
        .collect()
}

fn ensure_source_event_index(file: &mut NotificationFileV1) {
    let mut indexed: HashSet<_> = file
        .source_event_index
        .iter()
        .map(|entry| (entry.scope.clone(), entry.source_event_id.clone()))
        .collect();
    let mut missing = Vec::new();
    for partition in &file.partitions {
        for record in &partition.items {
            let Some(source_event_id) = &record.source_event_id else {
                continue;
            };
            let key = (partition.scope.clone(), source_event_id.clone());
            if indexed.insert(key) {
                missing.push(NotificationSourceEventIndexEntry {
                    scope: partition.scope.clone(),
                    source_event_id: source_event_id.clone(),
                    notification_id: record.id.clone(),
                });
            }
        }
    }
    file.source_event_index.extend(missing);
}

fn cleanup_source_event_index(file: &mut NotificationFileV1) {
    let targets: HashSet<_> = file
        .partitions
        .iter()
        .flat_map(|partition| {
            partition
                .items
                .iter()
                .map(|record| (partition.scope.clone(), record.id.clone()))
        })
        .collect();
    file.source_event_index
        .retain(|entry| targets.contains(&(entry.scope.clone(), entry.notification_id.clone())));
}

fn rebuild_active_incidents(file: &NotificationFileV1) -> HashMap<IncidentKey, ActiveIncident> {
    let mut records: Vec<_> = file
        .partitions
        .iter()
        .flat_map(|partition| partition.items.iter())
        .collect();
    records.sort_by(|left, right| {
        left.created_at_ms
            .cmp(&right.created_at_ms)
            .then_with(|| {
                incident_edge_priority(left.kind).cmp(&incident_edge_priority(right.kind))
            })
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut incidents = HashMap::new();
    for record in records {
        let NotificationScope::Account { account_id } = &record.scope else {
            continue;
        };
        let incident_id = record
            .dedupe_key
            .split(':')
            .nth(2)
            .filter(|value| !value.is_empty());
        let Some(incident_id) = incident_id else {
            continue;
        };
        let key = match record.kind {
            NotificationKind::ConnectionUnavailable | NotificationKind::ConnectionRecovered => {
                let Some(crate::models::notification::NotificationScalar::String(channel)) =
                    record.content.params.get("channel")
                else {
                    continue;
                };
                let channel = match channel.as_str() {
                    "api" => NotificationChannel::Api,
                    "websocket" => NotificationChannel::Websocket,
                    _ => continue,
                };
                IncidentKey::Connection(account_id.clone(), channel)
            }
            NotificationKind::EnvironmentUnavailable | NotificationKind::EnvironmentRecovered => {
                let Some(crate::models::notification::NotificationScalar::String(environment)) =
                    record.content.params.get("environment")
                else {
                    continue;
                };
                let environment = match environment.as_str() {
                    "production" => NotificationEnvironment::Production,
                    "development" => NotificationEnvironment::Development,
                    "unknown" => NotificationEnvironment::Unknown,
                    _ => continue,
                };
                IncidentKey::Environment(account_id.clone(), environment)
            }
            _ => continue,
        };
        match record.kind {
            NotificationKind::ConnectionUnavailable | NotificationKind::EnvironmentUnavailable => {
                incidents.insert(
                    key,
                    ActiveIncident {
                        id: incident_id.into(),
                    },
                );
            }
            NotificationKind::ConnectionRecovered | NotificationKind::EnvironmentRecovered => {
                if incidents
                    .get(&key)
                    .is_some_and(|incident| incident.id == incident_id)
                {
                    incidents.remove(&key);
                }
            }
            _ => {}
        }
    }
    incidents
}

fn channel_tag(channel: NotificationChannel) -> &'static str {
    match channel {
        NotificationChannel::Api => "api",
        NotificationChannel::Websocket => "websocket",
    }
}

fn environment_tag(environment: NotificationEnvironment) -> &'static str {
    match environment {
        NotificationEnvironment::Production => "production",
        NotificationEnvironment::Development => "development",
        NotificationEnvironment::Unknown => "unknown",
    }
}

fn incident_edge_priority(kind: NotificationKind) -> u8 {
    match kind {
        NotificationKind::ConnectionUnavailable | NotificationKind::EnvironmentUnavailable => 0,
        NotificationKind::ConnectionRecovered | NotificationKind::EnvironmentRecovered => 1,
        _ => 2,
    }
}

fn partition_mut<'a>(
    file: &'a mut NotificationFileV1,
    scope: &NotificationScope,
) -> &'a mut NotificationPartition {
    let index = file
        .partitions
        .iter()
        .position(|partition| &partition.scope == scope)
        .unwrap_or_else(|| {
            file.partitions.push(NotificationPartition {
                scope: scope.clone(),
                items: Vec::new(),
            });
            file.partitions.len() - 1
        });
    &mut file.partitions[index]
}

fn find_record_mut<'a>(
    file: &'a mut NotificationFileV1,
    id: &str,
) -> Option<(NotificationScope, &'a mut NotificationRecord)> {
    for partition in &mut file.partitions {
        if let Some(record) = partition.items.iter_mut().find(|record| record.id == id) {
            return Some((partition.scope.clone(), record));
        }
    }
    None
}

fn visible_unread_count(file: &NotificationFileV1, context: &ViewContext, now_ms: u64) -> u64 {
    file.partitions
        .iter()
        .filter(|partition| context.visible(&partition.scope))
        .flat_map(|partition| &partition.items)
        .filter(|record| record.read_at_ms.is_none() && !is_expired(record, now_ms))
        .count() as u64
}

fn commit_file(
    persistence: &Arc<dyn NotificationPersistence>,
    guard: &mut ServiceState,
    mut next: ServiceState,
) -> Result<u64, NotificationError> {
    let previous_revision = next.file.revision;
    next.file.revision = next.file.revision.checked_add(1).ok_or_else(|| {
        NotificationError::new("NOTIFICATION_REVISION_EXHAUSTED", "通知修订号已达上限")
    })?;
    persistence
        .save(&next.file)
        .map_err(map_persistence_error)?;
    *guard = next;
    Ok(previous_revision)
}

fn find_record_index(file: &NotificationFileV1, id: &str) -> Option<(usize, usize)> {
    file.partitions
        .iter()
        .enumerate()
        .find_map(|(partition_index, partition)| {
            partition
                .items
                .iter()
                .position(|record| record.id == id)
                .map(|item_index| (partition_index, item_index))
        })
}

fn remove_empty_partitions(file: &mut NotificationFileV1) {
    file.partitions
        .retain(|partition| !partition.items.is_empty());
}

const RETENTION_MS: u64 = 90 * 24 * 60 * 60 * 1_000;

fn is_expired(record: &NotificationRecord, now_ms: u64) -> bool {
    record.created_at_ms < now_ms.saturating_sub(RETENTION_MS)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CursorV1 {
    version: u8,
    fingerprint: String,
    filter: NotificationFilter,
    created_at_ms: u64,
    id: String,
}

fn query_fingerprint(context: &ViewContext) -> String {
    context.account_id.as_ref().map_or_else(
        || "global".into(),
        |account_id| format!("account:{account_id}|global"),
    )
}

fn encode_cursor(cursor: CursorV1) -> String {
    let bytes = serde_json::to_vec(&cursor).expect("CursorV1 serialization is infallible");
    let mut encoded = String::with_capacity(3 + bytes.len() * 2);
    encoded.push_str("n1.");
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

fn decode_cursor(
    encoded: &str,
    fingerprint: &str,
    filter: NotificationFilter,
) -> Result<CursorV1, NotificationError> {
    let hex = encoded.strip_prefix("n1.").ok_or_else(invalid_cursor)?;
    if hex.is_empty() || !hex.is_ascii() || hex.len() % 2 != 0 {
        return Err(invalid_cursor());
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for index in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[index..index + 2], 16).map_err(|_| invalid_cursor())?;
        bytes.push(byte);
    }
    let cursor: CursorV1 = serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())?;
    if cursor.version != 1
        || cursor.fingerprint != fingerprint
        || cursor.filter != filter
        || cursor.id.is_empty()
    {
        return Err(invalid_cursor());
    }
    Ok(cursor)
}

fn invalid_cursor() -> NotificationError {
    NotificationError::new(INVALID_NOTIFICATION_CURSOR, "通知游标无效")
}

fn map_persistence_error(error: crate::error::AppError) -> NotificationError {
    match error {
        crate::error::AppError::Storage(message) if message == NOTIFICATION_STORAGE_UNAVAILABLE => {
            NotificationError::new(NOTIFICATION_STORAGE_UNAVAILABLE, "通知存储不可用")
        }
        _ => NotificationError::new(NOTIFICATION_STORAGE_UNAVAILABLE, "通知存储不可用"),
    }
}

fn scope_mismatch() -> NotificationError {
    NotificationError::new(NOTIFICATION_SCOPE_MISMATCH, "通知账户范围不匹配")
}

#[cfg(test)]
mod tests;
