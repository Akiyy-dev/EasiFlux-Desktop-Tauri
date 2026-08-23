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
    notification_file_fits_serialized_limit, NotificationFileV1, NotificationLoadStatus,
    NotificationPartition, NotificationPersistence, NotificationSourceEventIndexEntry,
    NotificationStore, MAX_NOTIFICATION_PARTITION_ITEMS, MAX_NOTIFICATION_SOURCE_INDEX_PER_SCOPE,
    MAX_NOTIFICATION_SOURCE_INDEX_TOTAL,
};

pub mod policy;

pub use policy::NotificationPolicy;

const NOTIFICATION_STORAGE_UNAVAILABLE: &str = "NOTIFICATION_STORAGE_UNAVAILABLE";
const NOTIFICATION_SCOPE_MISMATCH: &str = "NOTIFICATION_SCOPE_MISMATCH";
const NOTIFICATION_NOT_FOUND: &str = "NOTIFICATION_NOT_FOUND";
const NOTIFICATION_SOURCE_INDEX_CAPACITY_EXCEEDED: &str =
    "NOTIFICATION_SOURCE_INDEX_CAPACITY_EXCEEDED";
const NOTIFICATION_FILE_CAPACITY_EXCEEDED: &str = "NOTIFICATION_FILE_CAPACITY_EXCEEDED";
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
        let _ = normalize_source_event_index(&mut file);
        let mut normalization = HousekeepingMutation::default();
        enforce_source_index_caps(&mut file, &mut normalization);
        let _ = enforce_serialized_file_cap(&mut file, &mut normalization, false);
        let active_incidents = rebuild_active_incidents(&file);
        let seen_source_events = rebuild_seen_source_events(&file);
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
        let configured: HashSet<_> = configured_accounts.iter().cloned().collect();
        let mut pruning = prune_file(&mut file, Some(&configured), now_ms);
        pruning.merge(normalize_source_event_index(&mut file));
        enforce_source_index_caps(&mut file, &mut pruning);
        enforce_serialized_file_cap(&mut file, &mut pruning, true)?;
        let previous_revision = file.revision;
        if pruning.changed {
            file.revision = file.revision.checked_add(1).ok_or_else(|| {
                NotificationError::new("NOTIFICATION_REVISION_EXHAUSTED", "通知修订号已达上限")
            })?;
            store.save(&file).map_err(map_persistence_error)?;
        }
        let service = Self::from_snapshot(file, Arc::new(store), emit_changed, now_ms);
        if pruning.changed {
            let event = NotificationChangedEvent {
                previous_revision: previous_revision.to_string(),
                revision: (previous_revision + 1).to_string(),
                change: NotificationChange::Reset,
                affected_scopes: pruning.affected_scopes,
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
        let Some(prepared) = self.prepare_publish_locked(&mut guard, &input, now_ms)? else {
            return Ok(no_publish_outcome(&guard));
        };
        let mut next = guard.clone();
        let mutation = apply_prepared_publish(&mut next, prepared)?;
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
        let _ = cleanup_source_event_index(&mut next.file);
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
        let _ = cleanup_source_event_index(&mut next.file);
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
        let mut pruning = prune_file(&mut next.file, Some(configured_accounts), now_ms);
        enforce_serialized_file_cap(&mut next.file, &mut pruning, true)?;
        next.seen_source_events = rebuild_seen_source_events(&next.file);
        next.active_incidents = rebuild_active_incidents(&next.file);
        next.order_states
            .retain(|key, _| configured_accounts.contains(&key.account_id));
        if !pruning.changed {
            *guard = next;
            return Ok(PruneOutcome {
                affected_count: 0,
                affected_scopes: pruning.affected_scopes,
                revision: guard.file.revision.to_string(),
            });
        }
        let previous_revision = commit_file(&self.persistence, &mut guard, next)?;
        let event = NotificationChangedEvent {
            previous_revision: previous_revision.to_string(),
            revision: guard.file.revision.to_string(),
            change: NotificationChange::Reset,
            affected_scopes: pruning.affected_scopes.clone(),
            notification_id: None,
            toast_candidate: None,
        };
        self.emit_diagnostic_only(&event);
        Ok(PruneOutcome {
            affected_count: pruning.affected_count,
            affected_scopes: pruning.affected_scopes,
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
        let partition_existed = next
            .file
            .partitions
            .iter()
            .any(|partition| partition.scope == scope);
        next.file
            .partitions
            .retain(|partition| partition.scope != scope);
        let _ = cleanup_source_event_index(&mut next.file);
        next.seen_source_events = rebuild_seen_source_events(&next.file);
        next.active_incidents.retain(|key, _| match key {
            IncidentKey::Connection(owner, _) | IncidentKey::Environment(owner, _) => {
                owner != account_id
            }
        });
        next.order_states
            .retain(|key, _| key.account_id != account_id);
        if !partition_existed {
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
        let policy = NotificationPolicy;
        let (input, next_incident) = match observation.state {
            AvailabilityState::Unavailable => {
                if guard.active_incidents.contains_key(&key) {
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
                (input, Some(ActiveIncident { id: incident_id }))
            }
            AvailabilityState::Available => {
                let Some(incident) = guard.active_incidents.get(&key).cloned() else {
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
                (
                    policy.connection_recovered(context, observation.channel, &incident.id)?,
                    None,
                )
            }
        };
        let Some(prepared) = self.prepare_publish_locked(&mut guard, &input, now_ms)? else {
            return Ok(no_publish_outcome(&guard));
        };
        let mut next = guard.clone();
        if let Some(incident) = next_incident {
            next.active_incidents.insert(key, incident);
        } else {
            next.active_incidents.remove(&key);
        }
        let mutation = apply_prepared_publish(&mut next, prepared)?;
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
        let policy = NotificationPolicy;
        let (input, next_incident) = match observation.state {
            AvailabilityState::Unavailable => {
                if guard.active_incidents.contains_key(&key) {
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
                (input, Some(ActiveIncident { id: incident_id }))
            }
            AvailabilityState::Available => {
                let Some(incident) = guard.active_incidents.get(&key).cloned() else {
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
                (
                    policy.environment_recovered(context, observation.environment, &incident.id)?,
                    None,
                )
            }
        };
        let Some(prepared) = self.prepare_publish_locked(&mut guard, &input, now_ms)? else {
            return Ok(no_publish_outcome(&guard));
        };
        let mut next = guard.clone();
        if let Some(incident) = next_incident {
            next.active_incidents.insert(key, incident);
        } else {
            next.active_incidents.remove(&key);
        }
        let mutation = apply_prepared_publish(&mut next, prepared)?;
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
        let previous = guard.order_states.get(&key).copied();
        if let Some(previous) = previous.filter(|state| state.status.is_terminal()) {
            if !observation.status.is_terminal() || previous.status != observation.status {
                tracing::warn!(
                    previous_status = previous.status.name(),
                    observed_status = observation.status.name(),
                    "ignored order transition after terminal state"
                );
                return Ok(no_publish_outcome(&guard));
            }
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
        if !should_publish {
            let mut next = guard.clone();
            next.order_states.insert(
                key,
                ObservedOrderState {
                    status: observation.status,
                    terminal_notified: already_notified,
                },
            );
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
        let Some(prepared) = self.prepare_publish_locked(&mut guard, &input, now_ms)? else {
            return Ok(no_publish_outcome(&guard));
        };
        let mut next = guard.clone();
        next.order_states.insert(
            key,
            ObservedOrderState {
                status: observation.status,
                terminal_notified: true,
            },
        );
        let mutation = apply_prepared_publish(&mut next, prepared)?;
        self.commit_publish(&mut guard, next, mutation)
    }

    fn prepare_publish_locked(
        &self,
        guard: &mut ServiceState,
        input: &NotificationInput,
        now_ms: u64,
    ) -> Result<Option<PreparedPublish>, NotificationError> {
        validate_publish_input(input)?;
        if source_key(input)
            .as_ref()
            .is_some_and(|key| guard.seen_source_events.contains(key))
        {
            return Ok(None);
        }

        let mut next = guard.clone();
        let mut housekeeping = prepare_file_for_publish(&mut next.file, input, now_ms)?;
        let prepared = prepare_publish_record(&next.file, input, now_ms)?;
        reserve_serialized_capacity_for_publish(
            &mut next.file,
            &prepared,
            &mut housekeeping.mutation,
        )?;
        if !housekeeping.mutation.changed {
            return Ok(Some(prepared));
        }
        next.seen_source_events = rebuild_seen_source_events(&next.file);
        next.active_incidents = rebuild_active_incidents(&next.file);
        let previous_revision = commit_file(&self.persistence, guard, next)?;
        let event = NotificationChangedEvent {
            previous_revision: previous_revision.to_string(),
            revision: guard.file.revision.to_string(),
            change: NotificationChange::Reset,
            affected_scopes: housekeeping.mutation.affected_scopes,
            notification_id: None,
            toast_candidate: None,
        };
        self.emit_diagnostic_only(&event);
        Ok(Some(prepared))
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

struct PublishHousekeeping {
    mutation: HousekeepingMutation,
}

#[derive(Clone)]
struct PreparedPublish {
    record: NotificationRecord,
    change: NotificationChange,
    toast_candidate: Option<NotificationToastCandidate>,
    source_entry: Option<NotificationSourceEventIndexEntry>,
}

fn prepare_publish_record(
    file: &NotificationFileV1,
    input: &NotificationInput,
    now_ms: u64,
) -> Result<PreparedPublish, NotificationError> {
    let scope = input.scope.clone();
    let session_epoch = input.session_epoch;
    let existing = file
        .partitions
        .iter()
        .find(|partition| partition.scope == scope)
        .and_then(|partition| {
            partition
                .items
                .iter()
                .find(|record| record.dedupe_key == input.dedupe_key)
        });
    let (record, change, toast_candidate) = if let Some(existing) = existing {
        let mut record = existing.clone();
        record.category = input.category;
        record.kind = input.kind;
        record.severity = input.severity;
        record.content = input.content.clone();
        record.entity = input.entity.clone();
        record.action = input.action.clone();
        record.occurrence_count = record.occurrence_count.checked_add(1).ok_or_else(|| {
            NotificationError::new("NOTIFICATION_OCCURRENCE_OVERFLOW", "通知出现次数已达上限")
        })?;
        record.updated_at_ms = now_ms.max(record.updated_at_ms);
        record
            .validate()
            .map_err(|error| NotificationError::new(error.code(), "通知记录无效"))?;
        (record, NotificationChange::Updated, None)
    } else {
        let record = NotificationRecord {
            id: uuid::Uuid::new_v4().to_string(),
            scope: scope.clone(),
            category: input.category,
            kind: input.kind,
            severity: input.severity,
            content: input.content.clone(),
            entity: input.entity.clone(),
            action: input.action.clone(),
            source_event_id: input.source_event_id.clone(),
            dedupe_key: input.dedupe_key.clone(),
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
        (record, NotificationChange::Created, Some(toast))
    };
    let source_entry = source_key(input).map(|(source_scope, source_event_id)| {
        NotificationSourceEventIndexEntry {
            scope: source_scope,
            source_event_id,
            notification_id: record.id.clone(),
        }
    });
    Ok(PreparedPublish {
        record,
        change,
        toast_candidate,
        source_entry,
    })
}

fn apply_prepared_to_file(
    file: &mut NotificationFileV1,
    prepared: &PreparedPublish,
) -> Result<(), NotificationError> {
    match prepared.change {
        NotificationChange::Created => {
            partition_mut(file, &prepared.record.scope)
                .items
                .push(prepared.record.clone());
        }
        NotificationChange::Updated => {
            let record = file
                .partitions
                .iter_mut()
                .find(|partition| partition.scope == prepared.record.scope)
                .and_then(|partition| {
                    partition
                        .items
                        .iter_mut()
                        .find(|record| record.id == prepared.record.id)
                })
                .ok_or_else(|| {
                    NotificationError::new("NOTIFICATION_STATE_CONFLICT", "通知状态已发生冲突")
                })?;
            *record = prepared.record.clone();
        }
        NotificationChange::Removed | NotificationChange::Reset => {
            return Err(NotificationError::new(
                "NOTIFICATION_STATE_CONFLICT",
                "通知状态已发生冲突",
            ));
        }
    }
    if let Some(source_entry) = &prepared.source_entry {
        file.source_event_index.push(source_entry.clone());
    }
    Ok(())
}

fn apply_prepared_publish(
    next: &mut ServiceState,
    prepared: PreparedPublish,
) -> Result<PublishMutation, NotificationError> {
    apply_prepared_to_file(&mut next.file, &prepared)?;
    if let Some(source_entry) = &prepared.source_entry {
        next.seen_source_events.insert((
            source_entry.scope.clone(),
            source_entry.source_event_id.clone(),
        ));
    }
    Ok(PublishMutation {
        record: Some(prepared.record.clone()),
        change: Some(prepared.change),
        toast_candidate: prepared.toast_candidate,
        affected_scopes: vec![prepared.record.scope],
    })
}

fn validate_publish_input(input: &NotificationInput) -> Result<(), NotificationError> {
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
    Ok(())
}

fn source_key(input: &NotificationInput) -> Option<(NotificationScope, String)> {
    input
        .source_event_id
        .as_ref()
        .map(|source| (input.scope.clone(), source.clone()))
}

fn no_publish_outcome(state: &ServiceState) -> PublishOutcome {
    PublishOutcome {
        notification: None,
        revision: state.file.revision.to_string(),
    }
}

#[derive(Default)]
struct HousekeepingMutation {
    affected_count: u64,
    affected_scopes: Vec<NotificationScope>,
    changed: bool,
}

impl HousekeepingMutation {
    fn note_scope(&mut self, scope: NotificationScope) {
        self.changed = true;
        if !self.affected_scopes.contains(&scope) {
            self.affected_scopes.push(scope);
        }
    }

    fn note_removed_record(&mut self, scope: NotificationScope) {
        self.affected_count += 1;
        self.note_scope(scope);
    }

    fn merge(&mut self, other: Self) {
        self.affected_count = self.affected_count.saturating_add(other.affected_count);
        self.changed |= other.changed;
        for scope in other.affected_scopes {
            if !self.affected_scopes.contains(&scope) {
                self.affected_scopes.push(scope);
            }
        }
    }
}

fn prepare_file_for_publish(
    file: &mut NotificationFileV1,
    input: &NotificationInput,
    now_ms: u64,
) -> Result<PublishHousekeeping, NotificationError> {
    let mut mutation = prune_file(file, None, now_ms);
    let semantic_target = file
        .partitions
        .iter()
        .find(|partition| partition.scope == input.scope)
        .and_then(|partition| {
            partition
                .items
                .iter()
                .find(|record| record.dedupe_key == input.dedupe_key)
        })
        .cloned();
    let protected_id = semantic_target.as_ref().map(|record| record.id.as_str());

    if input.source_event_id.is_some() {
        let scope_limit = MAX_NOTIFICATION_SOURCE_INDEX_PER_SCOPE.saturating_sub(1);
        let total_limit = MAX_NOTIFICATION_SOURCE_INDEX_TOTAL.saturating_sub(1);
        if protected_id.is_some_and(|id| {
            !can_reduce_source_index_without_record(file, Some(&input.scope), scope_limit, id)
                || !can_reduce_source_index_without_record(file, None, total_limit, id)
        }) {
            return Err(NotificationError::new(
                NOTIFICATION_SOURCE_INDEX_CAPACITY_EXCEEDED,
                "通知来源索引容量已满",
            ));
        }
        for removed in
            evict_indexed_records_to_limit(file, Some(&input.scope), scope_limit, protected_id)
        {
            debug_assert_ne!(protected_id, Some(removed.id.as_str()));
            mutation.note_removed_record(removed.scope);
        }
        for removed in evict_indexed_records_to_limit(file, None, total_limit, protected_id) {
            debug_assert_ne!(protected_id, Some(removed.id.as_str()));
            mutation.note_removed_record(removed.scope);
        }
    }

    let creates_record = !file
        .partitions
        .iter()
        .find(|partition| partition.scope == input.scope)
        .is_some_and(|partition| {
            partition
                .items
                .iter()
                .any(|record| record.dedupe_key == input.dedupe_key)
        });
    if creates_record {
        while partition_record_count(file, &input.scope) >= MAX_NOTIFICATION_PARTITION_ITEMS {
            let Some(scope) = evict_oldest_record(file, Some(&input.scope)) else {
                break;
            };
            mutation.note_removed_record(scope);
        }
    }

    Ok(PublishHousekeeping { mutation })
}

fn reserve_serialized_capacity_for_publish(
    file: &mut NotificationFileV1,
    prepared: &PreparedPublish,
    mutation: &mut HousekeepingMutation,
) -> Result<(), NotificationError> {
    let initial_revision_increment = if mutation.changed { 2 } else { 1 };
    if prepared_file_fits_after_eviction(file, prepared, &[], initial_revision_increment)? {
        return Ok(());
    }

    let protected_id =
        (prepared.change == NotificationChange::Updated).then_some(prepared.record.id.as_str());
    let mut candidates: Vec<_> = file
        .partitions
        .iter()
        .flat_map(|partition| {
            partition.items.iter().filter_map(|record| {
                (protected_id != Some(record.id.as_str())).then(|| {
                    (
                        record.created_at_ms,
                        record.id.clone(),
                        partition.scope.clone(),
                    )
                })
            })
        })
        .collect();
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    if candidates.is_empty() || !prepared_file_fits_after_eviction(file, prepared, &candidates, 2)?
    {
        return Err(NotificationError::new(
            NOTIFICATION_FILE_CAPACITY_EXCEEDED,
            "通知存储容量已满",
        ));
    }

    let mut lower = 0_usize;
    let mut upper = candidates.len();
    while lower + 1 < upper {
        let middle = lower + (upper - lower) / 2;
        if prepared_file_fits_after_eviction(file, prepared, &candidates[..middle], 2)? {
            upper = middle;
        } else {
            lower = middle;
        }
    }
    let selected = &candidates[..upper];
    remove_record_candidates(file, selected);
    for (_, _, scope) in selected {
        mutation.note_removed_record(scope.clone());
    }
    Ok(())
}

fn prepared_file_fits_after_eviction(
    original: &NotificationFileV1,
    prepared: &PreparedPublish,
    candidates: &[(u64, String, NotificationScope)],
    revision_increment: u64,
) -> Result<bool, NotificationError> {
    let mut trial = original.clone();
    remove_record_candidates(&mut trial, candidates);
    apply_prepared_to_file(&mut trial, prepared)?;
    trial.revision = trial
        .revision
        .checked_add(revision_increment)
        .ok_or_else(|| {
            NotificationError::new("NOTIFICATION_REVISION_EXHAUSTED", "通知修订号已达上限")
        })?;
    Ok(notification_file_fits_serialized_limit(&trial))
}

fn can_reduce_source_index_without_record(
    file: &NotificationFileV1,
    required_scope: Option<&NotificationScope>,
    limit: usize,
    protected_id: &str,
) -> bool {
    let mut current_count = 0_usize;
    let mut protected_count = 0_usize;
    for entry in &file.source_event_index {
        if required_scope.is_some_and(|scope| scope != &entry.scope) {
            continue;
        }
        current_count += 1;
        if entry.notification_id == protected_id {
            protected_count += 1;
        }
    }
    current_count <= limit || protected_count <= limit
}

fn prune_file(
    file: &mut NotificationFileV1,
    configured_accounts: Option<&HashSet<String>>,
    now_ms: u64,
) -> HousekeepingMutation {
    let mut mutation = HousekeepingMutation::default();
    for partition in &mut file.partitions {
        let original_len = partition.items.len();
        partition.items.retain(|record| !is_expired(record, now_ms));
        if partition.items.len() > MAX_NOTIFICATION_PARTITION_ITEMS {
            partition.items.sort_by(|left, right| {
                right
                    .created_at_ms
                    .cmp(&left.created_at_ms)
                    .then_with(|| right.id.cmp(&left.id))
            });
            partition.items.truncate(MAX_NOTIFICATION_PARTITION_ITEMS);
        }
        let removed = original_len - partition.items.len();
        for _ in 0..removed {
            mutation.note_removed_record(partition.scope.clone());
        }
    }
    if let Some(configured_accounts) = configured_accounts {
        let mut partition_index = 0;
        while partition_index < file.partitions.len() {
            let partition = &file.partitions[partition_index];
            let is_orphan = match &partition.scope {
                NotificationScope::Global => false,
                NotificationScope::Account { account_id } => {
                    !configured_accounts.contains(account_id)
                }
            };
            if is_orphan {
                let partition = file.partitions.remove(partition_index);
                if partition.items.is_empty() {
                    mutation.note_scope(partition.scope);
                } else {
                    for _ in &partition.items {
                        mutation.note_removed_record(partition.scope.clone());
                    }
                }
            } else {
                partition_index += 1;
            }
        }
    }
    file.partitions.retain(|partition| {
        !partition.items.is_empty() || !mutation.affected_scopes.contains(&partition.scope)
    });
    for scope in cleanup_source_event_index(file) {
        mutation.note_scope(scope);
    }

    enforce_source_index_caps(file, &mut mutation);
    mutation
}

fn partition_record_count(file: &NotificationFileV1, scope: &NotificationScope) -> usize {
    file.partitions
        .iter()
        .find(|partition| &partition.scope == scope)
        .map_or(0, |partition| partition.items.len())
}

fn enforce_source_index_caps(file: &mut NotificationFileV1, mutation: &mut HousekeepingMutation) {
    let mut per_scope = HashMap::<NotificationScope, usize>::new();
    for entry in &file.source_event_index {
        *per_scope.entry(entry.scope.clone()).or_default() += 1;
    }
    let mut over_limit_scopes: Vec<_> = per_scope
        .into_iter()
        .filter_map(|(scope, count)| {
            (count > MAX_NOTIFICATION_SOURCE_INDEX_PER_SCOPE).then_some(scope)
        })
        .collect();
    over_limit_scopes.sort_by(|left, right| scope_sort_key(left).cmp(&scope_sort_key(right)));
    for scope in over_limit_scopes {
        for removed in evict_indexed_records_to_limit(
            file,
            Some(&scope),
            MAX_NOTIFICATION_SOURCE_INDEX_PER_SCOPE,
            None,
        ) {
            mutation.note_removed_record(removed.scope);
        }
    }
    if file.source_event_index.len() > MAX_NOTIFICATION_SOURCE_INDEX_TOTAL {
        for removed in
            evict_indexed_records_to_limit(file, None, MAX_NOTIFICATION_SOURCE_INDEX_TOTAL, None)
        {
            mutation.note_removed_record(removed.scope);
        }
    }
}

fn enforce_serialized_file_cap(
    file: &mut NotificationFileV1,
    mutation: &mut HousekeepingMutation,
    persists_revision: bool,
) -> Result<usize, NotificationError> {
    let mut probes = 1_usize;
    let existing_increment = u64::from(persists_revision && mutation.changed);
    if serialized_file_fits_after_eviction(file, &[], existing_increment)? {
        return Ok(probes);
    }
    let mut candidates: Vec<_> = file
        .partitions
        .iter()
        .flat_map(|partition| {
            partition.items.iter().map(|record| {
                (
                    record.created_at_ms,
                    record.id.clone(),
                    partition.scope.clone(),
                )
            })
        })
        .collect();
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    if candidates.is_empty() {
        return Err(NotificationError::new(
            NOTIFICATION_FILE_CAPACITY_EXCEEDED,
            "通知存储容量已满",
        ));
    }

    let original = file.clone();
    let revision_increment = u64::from(persists_revision);
    let mut lower = 0_usize;
    let mut upper = 1_usize;
    loop {
        probes += 1;
        if serialized_file_fits_after_eviction(&original, &candidates[..upper], revision_increment)?
        {
            break;
        }
        lower = upper;
        if upper == candidates.len() {
            return Err(NotificationError::new(
                NOTIFICATION_FILE_CAPACITY_EXCEEDED,
                "通知存储容量已满",
            ));
        }
        upper = upper.saturating_mul(2).min(candidates.len());
    }
    while lower + 1 < upper {
        let middle = lower + (upper - lower) / 2;
        probes += 1;
        if serialized_file_fits_after_eviction(
            &original,
            &candidates[..middle],
            revision_increment,
        )? {
            upper = middle;
        } else {
            lower = middle;
        }
    }

    let selected = &candidates[..upper];
    remove_record_candidates(file, selected);
    for (_, _, scope) in selected {
        mutation.note_removed_record(scope.clone());
    }
    Ok(probes)
}

#[cfg(test)]
pub(super) fn enforce_serialized_file_cap_for_test(file: &mut NotificationFileV1) -> usize {
    let mut mutation = HousekeepingMutation::default();
    enforce_serialized_file_cap(file, &mut mutation, false).unwrap()
}

fn serialized_file_fits_after_eviction(
    original: &NotificationFileV1,
    candidates: &[(u64, String, NotificationScope)],
    revision_increment: u64,
) -> Result<bool, NotificationError> {
    let mut trial = original.clone();
    remove_record_candidates(&mut trial, candidates);
    trial.revision = trial
        .revision
        .checked_add(revision_increment)
        .ok_or_else(|| {
            NotificationError::new("NOTIFICATION_REVISION_EXHAUSTED", "通知修订号已达上限")
        })?;
    Ok(notification_file_fits_serialized_limit(&trial))
}

fn remove_record_candidates(
    file: &mut NotificationFileV1,
    candidates: &[(u64, String, NotificationScope)],
) {
    let targets: HashSet<_> = candidates
        .iter()
        .map(|(_, id, scope)| (scope.clone(), id.clone()))
        .collect();
    for partition in &mut file.partitions {
        partition
            .items
            .retain(|record| !targets.contains(&(partition.scope.clone(), record.id.clone())));
    }
    file.partitions
        .retain(|partition| !partition.items.is_empty());
    file.source_event_index
        .retain(|entry| !targets.contains(&(entry.scope.clone(), entry.notification_id.clone())));
}

fn evict_indexed_records_to_limit(
    file: &mut NotificationFileV1,
    required_scope: Option<&NotificationScope>,
    limit: usize,
    protected_id: Option<&str>,
) -> Vec<NotificationRecord> {
    let mut index_counts = HashMap::<(NotificationScope, String), usize>::new();
    let mut current_count = 0_usize;
    for entry in &file.source_event_index {
        if required_scope.is_some_and(|scope| scope != &entry.scope) {
            continue;
        }
        current_count += 1;
        *index_counts
            .entry((entry.scope.clone(), entry.notification_id.clone()))
            .or_default() += 1;
    }
    if current_count <= limit {
        return Vec::new();
    }

    let mut candidates: Vec<_> = file
        .partitions
        .iter()
        .filter(|partition| required_scope.is_none_or(|scope| scope == &partition.scope))
        .flat_map(|partition| {
            partition.items.iter().filter_map(|record| {
                index_counts
                    .get(&(partition.scope.clone(), record.id.clone()))
                    .copied()
                    .map(|count| (record.clone(), count))
            })
        })
        .collect();
    candidates.sort_by(|(left, _), (right, _)| {
        let left_protected = protected_id == Some(left.id.as_str());
        let right_protected = protected_id == Some(right.id.as_str());
        left_protected
            .cmp(&right_protected)
            .then_with(|| left.created_at_ms.cmp(&right.created_at_ms))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut selected = Vec::new();
    for (record, count) in candidates {
        current_count = current_count.saturating_sub(count);
        selected.push(record);
        if current_count <= limit {
            break;
        }
    }
    let targets: HashSet<_> = selected
        .iter()
        .map(|record| (record.scope.clone(), record.id.clone()))
        .collect();
    for partition in &mut file.partitions {
        partition
            .items
            .retain(|record| !targets.contains(&(partition.scope.clone(), record.id.clone())));
    }
    let affected_scopes: HashSet<_> = selected.iter().map(|record| record.scope.clone()).collect();
    file.partitions.retain(|partition| {
        !partition.items.is_empty() || !affected_scopes.contains(&partition.scope)
    });
    file.source_event_index
        .retain(|entry| !targets.contains(&(entry.scope.clone(), entry.notification_id.clone())));
    selected
}

fn evict_oldest_record(
    file: &mut NotificationFileV1,
    required_scope: Option<&NotificationScope>,
) -> Option<NotificationScope> {
    let mut oldest: Option<(usize, usize, u64, String)> = None;
    for (partition_index, partition) in file.partitions.iter().enumerate() {
        if required_scope.is_some_and(|scope| scope != &partition.scope) {
            continue;
        }
        for (record_index, record) in partition.items.iter().enumerate() {
            let candidate = (
                partition_index,
                record_index,
                record.created_at_ms,
                record.id.clone(),
            );
            if oldest.as_ref().is_none_or(|oldest| {
                (candidate.2, candidate.3.as_str()) < (oldest.2, oldest.3.as_str())
            }) {
                oldest = Some(candidate);
            }
        }
    }
    let (partition_index, record_index, _, _) = oldest?;
    let scope = file.partitions[partition_index].scope.clone();
    file.partitions[partition_index].items.remove(record_index);
    if file.partitions[partition_index].items.is_empty() {
        file.partitions.remove(partition_index);
    }
    let _ = cleanup_source_event_index(file);
    Some(scope)
}

fn rebuild_seen_source_events(file: &NotificationFileV1) -> HashSet<(NotificationScope, String)> {
    file.source_event_index
        .iter()
        .map(|entry| (entry.scope.clone(), entry.source_event_id.clone()))
        .collect()
}

fn normalize_source_event_index(file: &mut NotificationFileV1) -> HousekeepingMutation {
    let before = file.source_event_index.clone();
    let _ = canonicalize_partial_incident_creation_order(file);
    let mut indexed: HashSet<_> = file
        .source_event_index
        .iter()
        .map(|entry| (entry.scope.clone(), entry.source_event_id.clone()))
        .collect();
    let mut candidates: Vec<_> = file
        .partitions
        .iter()
        .flat_map(|partition| {
            partition.items.iter().filter_map(|record| {
                record.source_event_id.as_ref().map(|source_event_id| {
                    (
                        record.created_at_ms,
                        incident_edge_priority(record.kind),
                        record.id.clone(),
                        partition.scope.clone(),
                        source_event_id.clone(),
                    )
                })
            })
        })
        .collect();
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| scope_sort_key(&left.3).cmp(&scope_sort_key(&right.3)))
    });
    let mut missing = Vec::new();
    for (_, _, notification_id, scope, source_event_id) in candidates {
        let key = (scope.clone(), source_event_id.clone());
        if indexed.insert(key) {
            missing.push(NotificationSourceEventIndexEntry {
                scope,
                source_event_id,
                notification_id,
            });
        }
    }
    file.source_event_index.extend(missing);
    source_index_mutation_since(&before, &file.source_event_index)
}

fn canonicalize_partial_incident_creation_order(
    file: &mut NotificationFileV1,
) -> HousekeepingMutation {
    let before = file.source_event_index.clone();
    let creation_positions = incident_creation_positions(file);
    let edges: Vec<_> = file
        .partitions
        .iter()
        .flat_map(|partition| partition.items.iter())
        .filter_map(|record| {
            incident_history_edge(record, creation_positions.get(record.id.as_str()).copied())
        })
        .collect();
    let fallback_keys: HashSet<_> = edges
        .iter()
        .filter(|edge| edge.creation_position.is_none())
        .map(|edge| edge.key.clone())
        .collect();
    if fallback_keys.is_empty() {
        return HousekeepingMutation::default();
    }

    let creation_entries: HashMap<_, _> = file
        .partitions
        .iter()
        .flat_map(|partition| {
            partition.items.iter().filter_map(|record| {
                record.source_event_id.as_ref().map(|source_event_id| {
                    (
                        record.id.clone(),
                        NotificationSourceEventIndexEntry {
                            scope: partition.scope.clone(),
                            source_event_id: source_event_id.clone(),
                            notification_id: record.id.clone(),
                        },
                    )
                })
            })
        })
        .collect();
    let fallback_record_ids: HashSet<_> = edges
        .iter()
        .filter(|edge| fallback_keys.contains(&edge.key))
        .map(|edge| edge.record_id.clone())
        .collect();
    file.source_event_index.retain(|entry| {
        !fallback_record_ids.contains(&entry.notification_id)
            || creation_entries
                .get(&entry.notification_id)
                .is_none_or(|creation| {
                    entry.scope != creation.scope
                        || entry.source_event_id != creation.source_event_id
                })
    });

    let mut keys: Vec<_> = fallback_keys.into_iter().collect();
    keys.sort_by(|left, right| incident_key_sort_key(left).cmp(&incident_key_sort_key(right)));
    let mut appended = HashSet::new();
    for key in keys {
        let key_edges: Vec<_> = edges.iter().filter(|edge| edge.key == key).collect();
        let unavailable_incidents: HashSet<_> = key_edges
            .iter()
            .filter(|edge| edge.unavailable)
            .map(|edge| edge.incident_id.as_str())
            .collect();
        let recovered_incidents: HashSet<_> = key_edges
            .iter()
            .filter(|edge| !edge.unavailable)
            .map(|edge| edge.incident_id.as_str())
            .collect();

        let mut orphan_recoveries: Vec<_> = key_edges
            .iter()
            .copied()
            .filter(|edge| {
                !edge.unavailable && !unavailable_incidents.contains(edge.incident_id.as_str())
            })
            .collect();
        sort_legacy_incident_edges(&mut orphan_recoveries);
        append_incident_creation_entries(
            &orphan_recoveries,
            &creation_entries,
            &mut appended,
            &mut file.source_event_index,
        );

        let mut closed_incidents: Vec<_> = unavailable_incidents
            .intersection(&recovered_incidents)
            .copied()
            .collect();
        closed_incidents.sort_unstable();
        for incident_id in closed_incidents {
            let mut unavailable: Vec<_> = key_edges
                .iter()
                .copied()
                .filter(|edge| edge.unavailable && edge.incident_id == incident_id)
                .collect();
            let mut recovered: Vec<_> = key_edges
                .iter()
                .copied()
                .filter(|edge| !edge.unavailable && edge.incident_id == incident_id)
                .collect();
            sort_legacy_incident_edges(&mut unavailable);
            sort_legacy_incident_edges(&mut recovered);
            append_incident_creation_entries(
                &unavailable,
                &creation_entries,
                &mut appended,
                &mut file.source_event_index,
            );
            append_incident_creation_entries(
                &recovered,
                &creation_entries,
                &mut appended,
                &mut file.source_event_index,
            );
        }

        let mut unclosed: Vec<_> = key_edges
            .iter()
            .copied()
            .filter(|edge| {
                edge.unavailable && !recovered_incidents.contains(edge.incident_id.as_str())
            })
            .collect();
        sort_legacy_incident_edges(&mut unclosed);
        append_incident_creation_entries(
            &unclosed,
            &creation_entries,
            &mut appended,
            &mut file.source_event_index,
        );
    }
    source_index_mutation_since(&before, &file.source_event_index)
}

fn source_index_mutation_since(
    before: &[NotificationSourceEventIndexEntry],
    after: &[NotificationSourceEventIndexEntry],
) -> HousekeepingMutation {
    if before == after {
        return HousekeepingMutation::default();
    }
    let mut scopes: Vec<_> = before
        .iter()
        .chain(after)
        .map(|entry| entry.scope.clone())
        .collect();
    scopes.sort_by(|left, right| scope_sort_key(left).cmp(&scope_sort_key(right)));
    scopes.dedup();
    let mut mutation = HousekeepingMutation::default();
    for scope in scopes {
        let before_scope: Vec<_> = before.iter().filter(|entry| entry.scope == scope).collect();
        let after_scope: Vec<_> = after.iter().filter(|entry| entry.scope == scope).collect();
        if before_scope != after_scope {
            mutation.note_scope(scope);
        }
    }
    if !mutation.changed {
        for entry in before.iter().chain(after) {
            mutation.note_scope(entry.scope.clone());
        }
    }
    mutation
}

fn incident_creation_positions(file: &NotificationFileV1) -> HashMap<String, usize> {
    let records_by_id: HashMap<_, _> = file
        .partitions
        .iter()
        .flat_map(|partition| {
            partition
                .items
                .iter()
                .map(|record| (record.id.as_str(), record))
        })
        .collect();
    let mut positions = HashMap::new();
    for (position, entry) in file.source_event_index.iter().enumerate() {
        let Some(record) = records_by_id.get(entry.notification_id.as_str()) else {
            continue;
        };
        if record.scope == entry.scope
            && record.source_event_id.as_deref() == Some(entry.source_event_id.as_str())
        {
            positions.entry(record.id.clone()).or_insert(position);
        }
    }
    positions
}

fn incident_key_sort_key(key: &IncidentKey) -> (&str, u8, &str) {
    match key {
        IncidentKey::Connection(account_id, channel) => {
            (account_id.as_str(), 0, channel_tag(*channel))
        }
        IncidentKey::Environment(account_id, environment) => {
            (account_id.as_str(), 1, environment_tag(*environment))
        }
    }
}

fn sort_legacy_incident_edges(edges: &mut Vec<&IncidentHistoryEdge>) {
    edges.sort_by(|left, right| {
        left.created_at_ms
            .cmp(&right.created_at_ms)
            .then_with(|| left.record_id.cmp(&right.record_id))
            .then_with(|| left.incident_id.cmp(&right.incident_id))
    });
}

fn append_incident_creation_entries(
    edges: &[&IncidentHistoryEdge],
    entries: &HashMap<String, NotificationSourceEventIndexEntry>,
    appended: &mut HashSet<String>,
    index: &mut Vec<NotificationSourceEventIndexEntry>,
) {
    for edge in edges {
        if appended.insert(edge.record_id.clone()) {
            if let Some(entry) = entries.get(&edge.record_id) {
                index.push(entry.clone());
            }
        }
    }
}

fn cleanup_source_event_index(file: &mut NotificationFileV1) -> Vec<NotificationScope> {
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
    let mut affected_scopes = Vec::new();
    file.source_event_index.retain(|entry| {
        let keep = targets.contains(&(entry.scope.clone(), entry.notification_id.clone()));
        if !keep && !affected_scopes.contains(&entry.scope) {
            affected_scopes.push(entry.scope.clone());
        }
        keep
    });
    affected_scopes
}

fn scope_sort_key(scope: &NotificationScope) -> (&str, &str) {
    match scope {
        NotificationScope::Global => ("0", ""),
        NotificationScope::Account { account_id } => ("1", account_id.as_str()),
    }
}

fn rebuild_active_incidents(file: &NotificationFileV1) -> HashMap<IncidentKey, ActiveIncident> {
    let creation_positions = incident_creation_positions(file);
    let mut edges: Vec<_> = file
        .partitions
        .iter()
        .flat_map(|partition| partition.items.iter())
        .filter_map(|record| {
            incident_history_edge(record, creation_positions.get(record.id.as_str()).copied())
        })
        .collect();
    let fallback_keys: HashSet<_> = edges
        .iter()
        .filter(|edge| edge.creation_position.is_none())
        .map(|edge| edge.key.clone())
        .collect();
    edges.sort_by_key(|edge| edge.creation_position.unwrap_or(usize::MAX));
    let mut incidents = HashMap::new();
    for edge in edges
        .iter()
        .filter(|edge| !fallback_keys.contains(&edge.key))
    {
        if edge.unavailable {
            incidents.insert(
                edge.key.clone(),
                ActiveIncident {
                    id: edge.incident_id.clone(),
                },
            );
        } else if incidents
            .get(&edge.key)
            .is_some_and(|incident| incident.id == edge.incident_id)
        {
            incidents.remove(&edge.key);
        }
    }

    for key in fallback_keys {
        let recovered: HashSet<_> = edges
            .iter()
            .filter(|edge| edge.key == key && !edge.unavailable)
            .map(|edge| edge.incident_id.as_str())
            .collect();
        let latest_unclosed = edges
            .iter()
            .filter(|edge| {
                edge.key == key
                    && edge.unavailable
                    && !recovered.contains(edge.incident_id.as_str())
            })
            .max_by(|left, right| {
                left.created_at_ms
                    .cmp(&right.created_at_ms)
                    .then_with(|| left.record_id.cmp(&right.record_id))
                    .then_with(|| left.incident_id.cmp(&right.incident_id))
            });
        if let Some(edge) = latest_unclosed {
            incidents.insert(
                key,
                ActiveIncident {
                    id: edge.incident_id.clone(),
                },
            );
        }
    }
    incidents
}

struct IncidentHistoryEdge {
    key: IncidentKey,
    incident_id: String,
    unavailable: bool,
    created_at_ms: u64,
    record_id: String,
    creation_position: Option<usize>,
}

fn incident_history_edge(
    record: &NotificationRecord,
    creation_position: Option<usize>,
) -> Option<IncidentHistoryEdge> {
    let NotificationScope::Account { account_id } = &record.scope else {
        return None;
    };
    let incident_id = record
        .dedupe_key
        .split(':')
        .nth(2)
        .filter(|value| !value.is_empty())?;
    let key = match record.kind {
        NotificationKind::ConnectionUnavailable | NotificationKind::ConnectionRecovered => {
            let Some(crate::models::notification::NotificationScalar::String(channel)) =
                record.content.params.get("channel")
            else {
                return None;
            };
            let channel = match channel.as_str() {
                "api" => NotificationChannel::Api,
                "websocket" => NotificationChannel::Websocket,
                _ => return None,
            };
            IncidentKey::Connection(account_id.clone(), channel)
        }
        NotificationKind::EnvironmentUnavailable | NotificationKind::EnvironmentRecovered => {
            let Some(crate::models::notification::NotificationScalar::String(environment)) =
                record.content.params.get("environment")
            else {
                return None;
            };
            let environment = match environment.as_str() {
                "production" => NotificationEnvironment::Production,
                "development" => NotificationEnvironment::Development,
                "unknown" => NotificationEnvironment::Unknown,
                _ => return None,
            };
            IncidentKey::Environment(account_id.clone(), environment)
        }
        _ => return None,
    };
    Some(IncidentHistoryEdge {
        key,
        incident_id: incident_id.into(),
        unavailable: matches!(
            record.kind,
            NotificationKind::ConnectionUnavailable | NotificationKind::EnvironmentUnavailable
        ),
        created_at_ms: record.created_at_ms,
        record_id: record.id.clone(),
        creation_position,
    })
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
