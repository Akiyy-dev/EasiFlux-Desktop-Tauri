use super::{store::*, *};
use crate::{
    error::AppResult,
    plugin::{
        registry::strategy::CapturedStrategy,
        workflow::{self, AccountAuthority, AuthorityRequest, WorkflowHost},
        PluginRuntime,
    },
};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::Duration,
};
use tokio::{sync::Notify, time::Instant};

pub(super) struct Ticket {
    request: AuthorityRequest,
    captured: CapturedStrategy,
    authority: AccountAuthority,
    scope: String,
    epoch: u64,
    admission: u64,
    deadline: Instant,
}
pub(super) struct Live {
    pub(super) request: AuthorityRequest,
    pub(super) captured: CapturedStrategy,
    pub(super) authority: AccountAuthority,
    pub(super) epoch: u64,
    pub(super) intent: AtomicU8,
    pub(super) cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub(super) signal: Notify,
    pub(super) deadline: Instant,
    pub(super) last_wall: AtomicU64,
    clock_anchor: Instant,
    wall_anchor_ms: u64,
}
impl Live {
    pub(super) fn revoke(&self, stop: bool) {
        self.intent
            .fetch_max(if stop { 2 } else { 1 }, Ordering::AcqRel);
        if let Ok(c) = self.cancel.lock() {
            if let Some(c) = &*c {
                c.store(true, Ordering::Release);
            }
        }
        self.signal.notify_one();
    }
    pub(super) fn admitted(&self) -> bool {
        self.intent.load(Ordering::Acquire) == 0
    }
}
pub(super) struct Inner {
    pub(super) document: StrategyDocument,
    tickets: BTreeMap<String, Ticket>,
    pub(super) live: BTreeMap<String, Arc<Live>>,
    pub(super) unavailable: bool,
}
pub(crate) struct StrategySupervisor {
    pub(super) runtime: Arc<PluginRuntime>,
    pub(super) host: Arc<dyn WorkflowHost>,
    store: Arc<dyn StrategyStore>,
    pub(super) wake: Arc<Notify>,
    inner: Mutex<Inner>,
    // Never hold this short-lived map lock during persistence or an await.
    controls: Mutex<BTreeMap<String, Arc<Live>>>,
    admission: AtomicU64,
    closed: AtomicBool,
    workers: AtomicUsize,
    drained: Notify,
}
impl StrategySupervisor {
    pub(crate) fn new(
        runtime: Arc<PluginRuntime>,
        host: Arc<dyn WorkflowHost>,
        store: Arc<dyn StrategyStore>,
        wake: Arc<Notify>,
    ) -> Arc<Self> {
        let loaded = store.load().and_then(|d| {
            d.validate()?;
            Ok(d)
        });
        let unavailable = loaded.is_err();
        let mut document = loaded.unwrap_or_default();
        for run in &mut document.runs {
            if run.pending.is_some() {
                run.view.status = StrategyStatus::RecoveryRequired;
                run.view.reason = Some("plugin_strategy_recovery_required".into());
            } else if matches!(
                run.view.status,
                StrategyStatus::Running
                    | StrategyStatus::Stopping
                    | StrategyStatus::RecoveryRequired
            ) {
                run.view.status = StrategyStatus::Paused;
                run.view.reason = Some("plugin_strategy_restarted".into());
            }
        }
        Arc::new(Self {
            runtime,
            host,
            store,
            wake,
            inner: Mutex::new(Inner {
                document,
                tickets: BTreeMap::new(),
                live: BTreeMap::new(),
                unavailable,
            }),
            controls: Mutex::new(BTreeMap::new()),
            admission: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            workers: AtomicUsize::new(0),
            drained: Notify::new(),
        })
    }
    pub(super) fn lock(&self) -> AppResult<MutexGuard<'_, Inner>> {
        self.inner
            .lock()
            .map_err(|_| error("plugin_strategy_unavailable"))
    }
    pub(super) fn ready(inner: &Inner) -> AppResult<()> {
        if inner.unavailable {
            Err(error("plugin_strategy_storage_unavailable"))
        } else {
            Ok(())
        }
    }
    fn admission_epoch(&self) -> AppResult<u64> {
        let epoch = self.admission.load(Ordering::Acquire);
        if self.closed.load(Ordering::Acquire) || epoch == u64::MAX {
            Err(error("plugin_strategy_unavailable"))
        } else {
            Ok(epoch)
        }
    }
    pub(super) fn save(&self, inner: &mut Inner, mut next: StrategyDocument) -> AppResult<()> {
        let wall = self.host.now_ms();
        for run in &mut next.runs {
            if let Some(live) = inner.live.get(&run.view.run_id) {
                // Persist elapsed monotonic lifetime as a wall-time floor so a
                // pause/restart cannot renew it when the system clock stalls.
                let elapsed =
                    u64::try_from(live.clock_anchor.elapsed().as_millis()).unwrap_or(u64::MAX);
                let floor = live.wall_anchor_ms.saturating_add(elapsed);
                run.last_observed_at_ms = workflow::counter(&run.last_observed_at_ms)?
                    .max(wall)
                    .max(floor)
                    .to_string();
            }
        }
        next.revision = workflow::counter(&inner.document.revision)?
            .checked_add(1)
            .ok_or_else(|| error("plugin_strategy_storage_unavailable"))?
            .to_string();
        if inner.unavailable || self.store.save(&next).is_err() {
            inner.unavailable = true;
            inner.tickets.clear();
            for live in inner.live.values() {
                live.revoke(false);
            }
            for run in &mut next.runs {
                run.view.status = if run.pending.is_some() {
                    StrategyStatus::RecoveryRequired
                } else {
                    StrategyStatus::Faulted
                };
                run.view.reason = Some("plugin_strategy_storage_unavailable".into());
            }
            inner.document = next;
            return Err(error("plugin_strategy_storage_unavailable"));
        }
        inner.document = next;
        Ok(())
    }
    pub(crate) fn list(&self) -> AppResult<StrategyList> {
        let inner = self.lock()?;
        if inner.unavailable && inner.document.runs.is_empty() {
            return Err(error("plugin_strategy_storage_unavailable"));
        }
        Ok(StrategyList {
            schema_version: 1,
            runs: inner.document.runs.iter().map(|r| r.view.clone()).collect(),
        })
    }
    pub(crate) async fn access(&self, request: AuthorityRequest) -> AppResult<StrategyAccess> {
        request
            .validate()
            .map_err(|_| error("plugin_strategy_invalid_request"))?;
        let admission = {
            let inner = self.lock()?;
            Self::ready(&inner)?;
            self.admission_epoch()?
        };
        let _op = self.runtime.strategy_gate().await;
        let _account = self.host.lifecycle().read_guard().await;
        let captured = self.runtime.capture_strategy(&request).await?;
        let epoch = self.runtime.strategy_epoch()?;
        let authority = self
            .host
            .authority_locked()
            .await
            .map_err(|_| error("plugin_strategy_stale"))?;
        let scope = self
            .host
            .strategy_scope_locked()
            .await
            .map_err(|_| error("plugin_strategy_unavailable"))?;
        let now = self.host.now_ms();
        let token = uuid::Uuid::new_v4().to_string();
        let result = StrategyAccess {
            schema_version: 1,
            plugin_id: request.plugin_id.clone(),
            contribution_id: request.contribution_id.clone(),
            catalog_generation: request.expected_catalog_generation.clone(),
            revision: request.expected_revision.clone(),
            account: authority.account.clone(),
            requested_capabilities: captured.requested.clone(),
            authorization_token: token.clone(),
            expires_at_ms: now
                .checked_add(60000)
                .ok_or_else(|| error("plugin_strategy_unavailable"))?
                .to_string(),
        };
        let mut inner = self.lock()?;
        Self::ready(&inner)?;
        if self.admission_epoch()? != admission {
            return Err(error("plugin_strategy_stale"));
        }
        inner.tickets.retain(|_, t| t.deadline > Instant::now());
        if inner.tickets.len() >= 32 {
            return Err(error("plugin_strategy_capacity"));
        }
        inner.tickets.insert(
            token,
            Ticket {
                request,
                captured,
                authority,
                scope,
                epoch,
                admission,
                deadline: Instant::now() + Duration::from_secs(60),
            },
        );
        Ok(result)
    }
    pub(crate) async fn start(
        self: &Arc<Self>,
        request: StrategyStartRequest,
    ) -> AppResult<StrategyRunView> {
        request.validate()?;
        let ticket = {
            let mut inner = self.lock()?;
            Self::ready(&inner)?;
            inner
                .tickets
                .remove(&request.authorization_token)
                .ok_or_else(|| error("plugin_strategy_token_invalid"))?
        };
        let owner = self.clone();
        tokio::spawn(async move { owner.start_owned(ticket, request).await })
            .await
            .map_err(|_| error("plugin_strategy_unavailable"))?
    }
    async fn start_owned(
        self: Arc<Self>,
        ticket: Ticket,
        request: StrategyStartRequest,
    ) -> AppResult<StrategyRunView> {
        let _op = self.runtime.strategy_gate().await;
        let _account = self.host.lifecycle().read_guard().await;
        let captured = self.runtime.capture_strategy(&ticket.request).await?;
        let authority = self
            .host
            .authority_locked()
            .await
            .map_err(|_| error("plugin_strategy_stale"))?;
        let scope = self
            .host
            .strategy_scope_locked()
            .await
            .map_err(|_| error("plugin_strategy_stale"))?;
        if ticket.deadline <= Instant::now()
            || ticket.epoch != self.runtime.strategy_epoch()?
            || captured.identity != ticket.captured.identity
            || authority != ticket.authority
            || scope != ticket.scope
            || request
                .capabilities
                .iter()
                .any(|c| !captured.requested.contains(c))
        {
            return Err(error("plugin_strategy_stale"));
        }
        let now = self.host.now_ms();
        let mut inner = self.lock()?;
        Self::ready(&inner)?;
        if self.admission_epoch()? != ticket.admission
            || inner
                .document
                .runs
                .iter()
                .any(|r| r.view.request_id == request.request_id)
            || inner.live.len() >= 4
        {
            return Err(error("plugin_strategy_stale"));
        }
        if inner.document.runs.iter().any(|r| {
            (inner.live.contains_key(&r.view.run_id)
                && r.view.account.account_id == authority.account.account_id)
                || (r.pending.is_some()
                    && r.view.account.account_id == authority.account.account_id
                    && r.view.account.environment == authority.account.environment)
        }) {
            return Err(error("plugin_strategy_recovery_required"));
        }
        let mut next = inner.document.clone();
        let resume = request.resume_run_id.is_some();
        let index = if let Some(id) = &request.resume_run_id {
            let index = next
                .runs
                .iter()
                .position(|r| &r.view.run_id == id)
                .ok_or_else(|| error("plugin_strategy_invalid_request"))?;
            let r = &next.runs[index];
            if !matches!(
                r.view.status,
                StrategyStatus::Paused | StrategyStatus::Stopped | StrategyStatus::Faulted
            ) || r.pending.is_some()
                || r.fingerprint != captured.identity.approval_fingerprint
                || r.scope != scope
                || r.view.plugin_id != ticket.request.plugin_id
                || r.view.contribution_id != ticket.request.contribution_id
                || r.view.account.account_id != authority.account.account_id
                || r.view.account.environment != authority.account.environment
                || r.view.symbol != request.symbol
                || r.view.input_json != request.input_json
                || r.view.capabilities != request.capabilities
                || r.view.policy != request.policy
                || now < workflow::counter(&r.last_observed_at_ms)?
                || now >= workflow::counter(&r.view.expires_at_ms)?
                || r.view.actions_submitted >= r.view.policy.max_actions
            {
                return Err(error("plugin_strategy_stale"));
            }
            index
        } else {
            if next.runs.len() >= 32 {
                return Err(error("plugin_strategy_capacity"));
            }
            next.runs.push(RunRecord {
                view: StrategyRunView {
                    schema_version: 1,
                    run_id: uuid::Uuid::new_v4().to_string(),
                    request_id: request.request_id.clone(),
                    plugin_id: ticket.request.plugin_id.clone(),
                    contribution_id: ticket.request.contribution_id.clone(),
                    account: authority.account.clone(),
                    symbol: request.symbol.clone(),
                    status: StrategyStatus::Running,
                    reason: None,
                    policy: request.policy.clone(),
                    capabilities: request.capabilities.clone(),
                    input_json: request.input_json.clone(),
                    started_at_ms: now.to_string(),
                    expires_at_ms: now
                        .checked_add(request.policy.max_run_seconds * 1000)
                        .ok_or_else(|| error("plugin_strategy_invalid_request"))?
                        .to_string(),
                    sequence: "0".into(),
                    actions_submitted: 0,
                    total_submitted_qty: "0".into(),
                    last_message: String::new(),
                    last_receipt: None,
                },
                fingerprint: captured.identity.approval_fingerprint.clone(),
                scope,
                state: serde_json::json!({}),
                owned: BTreeMap::new(),
                pending: None,
                last_observed_at_ms: now.to_string(),
            });
            next.runs.len() - 1
        };
        let view = &mut next.runs[index].view;
        view.status = StrategyStatus::Running;
        view.reason = None;
        view.request_id = request.request_id;
        view.account = authority.account.clone();
        let view = view.clone();
        let clock_anchor = Instant::now();
        let live = Arc::new(Live {
            request: ticket.request,
            captured,
            authority,
            epoch: ticket.epoch,
            intent: AtomicU8::new(0),
            cancel: Mutex::new(None),
            signal: Notify::new(),
            deadline: clock_anchor
                + Duration::from_millis(workflow::counter(&view.expires_at_ms)? - now),
            last_wall: AtomicU64::new(now),
            clock_anchor,
            wall_anchor_ms: now,
        });
        self.save(&mut inner, next)?;
        let mut controls = self
            .controls
            .lock()
            .map_err(|_| error("plugin_strategy_unavailable"))?;
        if self.admission_epoch().ok() != Some(ticket.admission) {
            drop(controls);
            let mut next = inner.document.clone();
            next.runs[index].view.status = StrategyStatus::Stopped;
            next.runs[index].view.reason = Some("plugin_strategy_stopped".into());
            self.save(&mut inner, next)?;
            return Err(error("plugin_strategy_stale"));
        }
        controls.insert(view.run_id.clone(), live.clone());
        inner.live.insert(view.run_id.clone(), live.clone());
        self.workers.fetch_add(1, Ordering::AcqRel);
        drop(controls);
        drop(inner);
        let owner = self.clone();
        let id = view.run_id.clone();
        tokio::spawn(async move {
            owner.worker(id, live, resume).await;
        });
        Ok(view)
    }
    fn revoke_intent(&self, stop: bool, id: Option<&str>) -> AppResult<()> {
        self.admission
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| v.checked_add(1))
            .map_err(|_| error("plugin_strategy_unavailable"))?;
        let controls = self
            .controls
            .lock()
            .map_err(|_| error("plugin_strategy_unavailable"))?;
        for (run_id, live) in controls.iter() {
            if id.is_none_or(|id| id == run_id) {
                live.revoke(stop);
            }
        }
        Ok(())
    }
    pub(crate) async fn control(
        &self,
        request: StrategyControlRequest,
    ) -> AppResult<StrategyRunView> {
        if uuid::Uuid::parse_str(&request.run_id).is_err() {
            return Err(error("plugin_strategy_invalid_request"));
        }
        let stop = request.action == ControlAction::Stop;
        self.revoke_intent(stop, Some(&request.run_id))?;
        let mut inner = self.lock()?;
        inner.tickets.clear();
        let index = inner
            .document
            .runs
            .iter()
            .position(|r| r.view.run_id == request.run_id)
            .ok_or_else(|| error("plugin_strategy_invalid_request"))?;
        let mut next = inner.document.clone();
        let r = &mut next.runs[index];
        if inner.live.contains_key(&request.run_id) {
            r.view.status = StrategyStatus::Stopping;
            r.view.reason = Some(
                if stop {
                    "plugin_strategy_stopped"
                } else {
                    "plugin_strategy_paused"
                }
                .into(),
            );
        } else if r.pending.is_some() {
            r.view.status = StrategyStatus::RecoveryRequired;
            r.view.reason = Some("plugin_strategy_recovery_required".into());
        } else if r.view.status != StrategyStatus::Completed {
            r.view.status = if stop {
                StrategyStatus::Stopped
            } else {
                StrategyStatus::Paused
            };
            r.view.reason = Some(
                if stop {
                    "plugin_strategy_stopped"
                } else {
                    "plugin_strategy_paused"
                }
                .into(),
            );
        }
        self.save(&mut inner, next)?;
        Ok(inner.document.runs[index].view.clone())
    }
    pub(crate) async fn stop_all(&self) -> AppResult<StrategyList> {
        self.revoke_intent(true, None)?;
        let mut inner = self.lock()?;
        inner.tickets.clear();
        let mut next = inner.document.clone();
        for r in &mut next.runs {
            if inner.live.contains_key(&r.view.run_id) {
                r.view.status = StrategyStatus::Stopping;
                r.view.reason = Some("plugin_strategy_stopped".into());
            } else if r.pending.is_some() {
                r.view.status = StrategyStatus::RecoveryRequired;
                r.view.reason = Some("plugin_strategy_recovery_required".into());
            } else if !matches!(
                r.view.status,
                StrategyStatus::Completed | StrategyStatus::Stopped
            ) {
                r.view.status = StrategyStatus::Stopped;
                r.view.reason = Some("plugin_strategy_stopped".into());
            }
        }
        if !next.runs.is_empty() {
            self.save(&mut inner, next)?;
        }
        drop(inner);
        self.list()
    }
    pub(crate) fn revoke_for_exit(&self) {
        self.closed.store(true, Ordering::Release);
        let _ = self.revoke_intent(true, None);
    }
    pub(crate) async fn drain(&self) {
        loop {
            let notified = self.drained.notified();
            // No blocking durable-state mutex here: the caller's shutdown timeout
            // must remain pollable even while a worker is fsyncing its receipt.
            if self.workers.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }
    pub(super) fn finish(&self, id: &str, live: &Arc<Live>) {
        if let Ok(mut inner) = self.lock() {
            if !inner.live.get(id).is_some_and(|v| Arc::ptr_eq(v, live)) {
                return;
            }
            let mut next = inner.document.clone();
            if let Some(r) = next.runs.iter_mut().find(|r| r.view.run_id == id) {
                if r.pending.is_some() {
                    r.view.status = StrategyStatus::RecoveryRequired;
                    if r.view.reason.is_none()
                        || r.view.reason.as_deref() == Some("plugin_strategy_stopped")
                    {
                        r.view.reason = Some("plugin_strategy_recovery_required".into());
                    }
                } else if live.intent.load(Ordering::Acquire) > 0
                    && !matches!(
                        r.view.status,
                        StrategyStatus::Faulted | StrategyStatus::Completed
                    )
                {
                    let stop = live.intent.load(Ordering::Acquire) == 2;
                    r.view.status = if stop {
                        StrategyStatus::Stopped
                    } else {
                        StrategyStatus::Paused
                    };
                    r.view.reason = Some(
                        if stop {
                            "plugin_strategy_stopped"
                        } else {
                            "plugin_strategy_paused"
                        }
                        .into(),
                    );
                }
            }
            if !inner.unavailable {
                let _ = self.save(&mut inner, next);
            }
            inner.live.remove(id);
            if let Ok(mut controls) = self.controls.lock() {
                controls.remove(id);
            }
            drop(inner);
            self.workers.fetch_sub(1, Ordering::AcqRel);
            self.drained.notify_waiters();
        }
    }
    pub(super) fn state(&self, id: &str) -> AppResult<RunRecord> {
        self.lock()?
            .document
            .runs
            .iter()
            .find(|r| r.view.run_id == id)
            .cloned()
            .ok_or_else(|| error("plugin_strategy_stale"))
    }
    pub(super) fn mark(&self, id: &str, status: StrategyStatus, reason: &'static str) {
        if let Ok(mut inner) = self.lock() {
            let mut next = inner.document.clone();
            if let Some(r) = next.runs.iter_mut().find(|r| r.view.run_id == id) {
                r.view.status = status;
                r.view.reason = Some(reason.into());
                let _ = self.save(&mut inner, next);
            }
        }
    }
}
