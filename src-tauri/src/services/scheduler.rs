use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard};
use std::time::Duration;

use tokio::sync::{oneshot, watch, Mutex, Notify, RwLock};

use crate::api::diagnostic::{warn_if_parse_empty, warn_if_raw_parsed_mismatch};
use crate::api::endpoints;
use crate::api::mapper::{
    build_order_query_params, list_envelope_meta, parse_balances, parse_positions,
};
use crate::api::{ApiClient, PublicApi};
use crate::error::{AppError, AppResult};
use crate::events::EventEmitter;
use crate::models::account::AccountSummary;
use crate::models::config::{
    environment_label, normalize_account_id, AppConfig, ConnectionStatus, EnvironmentStatus,
    DEFAULT_BASE_URL, MAX_TICKER_POLL_INTERVAL_SECS, MIN_TICKER_POLL_INTERVAL_SECS,
};
use crate::models::time::{TimeSnapshot, TimeSyncStatus};
use crate::models::trading::PrivatePanelsSnapshot;
use crate::services::notification::NotificationRuntime;
use crate::services::{
    AccountLifecycleCoordinator, ChartWorkspaceService, ConnectionService, DailyPnlService,
    MarketService, TimeService, TradingService,
};
use crate::storage::CredentialStore;
use crate::ws::WsManager;

pub const PUBLIC_STALE_MS: u64 = 5_000;
pub const PRIVATE_STALE_MS: u64 = 5_000;

const INTERVAL_TIME_SYNC: Duration = Duration::from_secs(45);
const INTERVAL_FUNDING: Duration = Duration::from_secs(60);
const INTERVAL_BALANCES: Duration = Duration::from_secs(7);
const INTERVAL_PRIVATE_PANELS: Duration = Duration::from_secs(4);
const INTERVAL_DAILY_PNL: Duration = Duration::from_secs(60);
const INTERVAL_MARKET_FALLBACK: Duration = Duration::from_secs(1);
const INTERVAL_KLINE_FLUSH: Duration = Duration::from_secs(5);
const INTERVAL_NOTIFICATION_MAINTENANCE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskId {
    TimeSync,
    FundingRate,
    Balances,
    PrivatePanels,
    DailyPnl,
    MarketFallback,
    KlineFlush,
    Environment,
    NotificationMaintenance,
}

impl TaskId {
    pub fn all() -> &'static [Self] {
        &[
            Self::TimeSync,
            Self::FundingRate,
            Self::Balances,
            Self::PrivatePanels,
            Self::DailyPnl,
            Self::MarketFallback,
            Self::KlineFlush,
            Self::Environment,
            Self::NotificationMaintenance,
        ]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "time" | "timeSync" => Some(Self::TimeSync),
            "funding" | "fundingRate" => Some(Self::FundingRate),
            "account" | "balances" => Some(Self::Balances),
            "privatePanels" => Some(Self::PrivatePanels),
            "dailyPnl" => Some(Self::DailyPnl),
            "market" | "marketFallback" => Some(Self::MarketFallback),
            "kline" | "klineFlush" => Some(Self::KlineFlush),
            "environment" => Some(Self::Environment),
            _ => None,
        }
    }

    fn interval(self) -> Option<Duration> {
        match self {
            Self::TimeSync => Some(INTERVAL_TIME_SYNC),
            Self::FundingRate => Some(INTERVAL_FUNDING),
            Self::Balances => Some(INTERVAL_BALANCES),
            Self::PrivatePanels => Some(INTERVAL_PRIVATE_PANELS),
            Self::DailyPnl => Some(INTERVAL_DAILY_PNL),
            Self::MarketFallback => Some(INTERVAL_MARKET_FALLBACK),
            Self::KlineFlush => Some(INTERVAL_KLINE_FLUSH),
            Self::Environment => None,
            Self::NotificationMaintenance => Some(INTERVAL_NOTIFICATION_MAINTENANCE),
        }
    }

    fn bootstrap_label(self) -> &'static str {
        match self {
            Self::TimeSync => "时间同步",
            Self::FundingRate => "资金费率",
            Self::Balances => "账户余额",
            Self::PrivatePanels => "订单/持仓",
            Self::DailyPnl => "今日盈亏",
            Self::MarketFallback => "行情快照",
            Self::KlineFlush => "K线持久化",
            Self::Environment => "环境检测",
            Self::NotificationMaintenance => "通知维护",
        }
    }

    fn requires_account_lifecycle(self) -> bool {
        self != Self::KlineFlush
    }
}

fn first_tick_delay(task: TaskId) -> Duration {
    if matches!(task, TaskId::KlineFlush | TaskId::NotificationMaintenance) {
        task.interval().expect("periodic task has an interval")
    } else {
        Duration::ZERO
    }
}

fn periodic_task_ids() -> &'static [TaskId] {
    &[
        TaskId::TimeSync,
        TaskId::FundingRate,
        TaskId::Balances,
        TaskId::PrivatePanels,
        TaskId::DailyPnl,
        TaskId::MarketFallback,
        TaskId::KlineFlush,
        TaskId::NotificationMaintenance,
    ]
}

fn configured_task_interval(task: TaskId, config: &AppConfig) -> Option<Duration> {
    if task == TaskId::MarketFallback {
        let seconds = config.ticker_poll_interval;
        let safe = if seconds.is_finite()
            && (MIN_TICKER_POLL_INTERVAL_SECS..=MAX_TICKER_POLL_INTERVAL_SECS).contains(&seconds)
        {
            seconds
        } else {
            1.0
        };
        Some(Duration::from_secs_f64(safe))
    } else {
        task.interval()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionMode {
    Periodic,
    Bootstrap,
}

async fn run_rest_snapshot_if_needed<F, Fut>(
    mode: ExecutionMode,
    matching_ws_domains_fresh: bool,
    operation: F,
) -> AppResult<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    if mode == ExecutionMode::Periodic && matching_ws_domains_fresh {
        Ok(())
    } else {
        operation().await
    }
}

fn bootstrap_tasks(connected: bool) -> Vec<TaskId> {
    let mut tasks = vec![
        TaskId::TimeSync,
        TaskId::MarketFallback,
        TaskId::FundingRate,
        TaskId::Environment,
    ];
    if connected {
        tasks.extend([TaskId::Balances, TaskId::PrivatePanels, TaskId::DailyPnl]);
    }
    tasks
}

async fn run_bootstrap_tasks<F, Fut>(tasks: &[TaskId], mut run: F) -> Vec<TaskId>
where
    F: FnMut(TaskId) -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let mut failed = Vec::new();
    for task in tasks {
        if run(*task).await.is_err() {
            failed.push(*task);
        }
    }
    failed
}

fn bootstrap_failure_error(failed: &[TaskId]) -> AppError {
    let labels = failed
        .iter()
        .map(|task| task.bootstrap_label())
        .collect::<Vec<_>>()
        .join("、");
    AppError::Connection(format!("连接初始化未完成: {labels}"))
}

fn time_sync_task_result(snapshot: &TimeSnapshot) -> AppResult<()> {
    if snapshot.sync_status == TimeSyncStatus::Failed {
        Err(AppError::Connection("服务器时间同步失败".into()))
    } else {
        Ok(())
    }
}

async fn run_time_sync_task(time: &TimeService) -> AppResult<()> {
    let snapshot = time.sync().await?;
    time_sync_task_result(&snapshot)
}

fn environment_task_result(status: &EnvironmentStatus) -> AppResult<()> {
    if status.reachable {
        Ok(())
    } else {
        Err(AppError::Connection("环境检测失败".into()))
    }
}

async fn publish_environment_task_status<F>(
    environment_status: &Arc<RwLock<EnvironmentStatus>>,
    status: EnvironmentStatus,
    emit: F,
) -> AppResult<()>
where
    F: FnOnce(&EnvironmentStatus),
{
    *environment_status.write().await = status.clone();
    emit(&status);
    environment_task_result(&status)
}

async fn execute_task_with_account_lifecycle<F, Fut>(
    coordinator: &AccountLifecycleCoordinator,
    task: TaskId,
    operation: F,
) -> AppResult<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    if task.requires_account_lifecycle() {
        crate::services::account_profiles::run_account_public_operation(coordinator, operation)
            .await
    } else {
        operation().await
    }
}

async fn run_serialized_blocking<F, R>(
    gate: Arc<StdMutex<()>>,
    operation: F,
) -> Result<R, tokio::task::JoinError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        // The blocking closure owns the guard so cancelling its async waiter cannot release it early.
        let _guard = gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        operation()
    })
    .await
}

async fn execute_kline_flush(
    service: Arc<ChartWorkspaceService>,
    gate: Arc<StdMutex<()>>,
) -> AppResult<()> {
    let outcomes = run_serialized_blocking(gate, move || service.flush_dirty_klines())
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let failures = outcomes
        .into_iter()
        .filter_map(|(key, result)| {
            result
                .err()
                .map(|error| format!("{}_{}: {error}", key.symbol, key.interval))
        })
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(AppError::Storage(failures.join("; ")))
    }
}

fn lock_unpoisoned<T>(mutex: &StdMutex<T>) -> StdMutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug)]
struct GenerationControl {
    cancelled: AtomicBool,
    cancellation: watch::Sender<bool>,
    active_owners: AtomicUsize,
    drained: Notify,
}

impl GenerationControl {
    fn new() -> Arc<Self> {
        let (cancellation, _) = watch::channel(false);
        Arc::new(Self {
            cancelled: AtomicBool::new(false),
            cancellation,
            active_owners: AtomicUsize::new(0),
            drained: Notify::new(),
        })
    }

    fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::SeqCst) {
            self.cancellation.send_replace(true);
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    fn subscribe(&self) -> watch::Receiver<bool> {
        self.cancellation.subscribe()
    }

    fn owner_started(&self) {
        self.active_owners.fetch_add(1, Ordering::SeqCst);
    }

    fn owner_finished(&self) {
        let previous = self.active_owners.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(previous > 0, "scheduler owner count underflow");
        if previous == 1 {
            self.drained.notify_waiters();
        }
    }

    async fn wait_drained(&self) {
        loop {
            let notified = self.drained.notified();
            if self.active_owners.load(Ordering::SeqCst) == 0 {
                return;
            }
            notified.await;
        }
    }
}

async fn wait_for_generation_cancel(mut cancellation: watch::Receiver<bool>) {
    loop {
        if *cancellation.borrow() {
            return;
        }
        if cancellation.changed().await.is_err() {
            return;
        }
    }
}

#[derive(Debug)]
struct TaskRunState {
    control: Arc<GenerationControl>,
    closed: bool,
    in_flight: bool,
    owner_token: Option<u64>,
    next_owner_token: u64,
    pending_force: bool,
    forced_rerun: bool,
    pending_force_waiters: Vec<oneshot::Sender<AppResult<()>>>,
    active_force_waiters: Vec<oneshot::Sender<AppResult<()>>>,
}

impl TaskRunState {
    fn new(control: Arc<GenerationControl>) -> Self {
        Self {
            control,
            closed: false,
            in_flight: false,
            owner_token: None,
            next_owner_token: 0,
            pending_force: false,
            forced_rerun: false,
            pending_force_waiters: Vec::new(),
            active_force_waiters: Vec::new(),
        }
    }
}

impl Default for TaskRunState {
    fn default() -> Self {
        Self::new(GenerationControl::new())
    }
}

type TaskRunStateRef = Arc<StdMutex<TaskRunState>>;

fn new_task_run_states(control: &Arc<GenerationControl>) -> Arc<HashMap<TaskId, TaskRunStateRef>> {
    Arc::new(
        TaskId::all()
            .iter()
            .copied()
            .map(|task| {
                (
                    task,
                    Arc::new(StdMutex::new(TaskRunState::new(Arc::clone(control)))),
                )
            })
            .collect(),
    )
}

fn cancelled_task_result() -> AppResult<()> {
    Err(AppError::Internal("调度任务在强制重跑完成前被取消".into()))
}

fn cancel_task_run_state(run_state: &TaskRunStateRef) {
    let waiters = {
        let mut state = lock_unpoisoned(run_state);
        state.closed = true;
        state.pending_force = false;
        state.forced_rerun = false;
        let mut waiters = std::mem::take(&mut state.active_force_waiters);
        waiters.extend(std::mem::take(&mut state.pending_force_waiters));
        waiters
    };
    for waiter in waiters {
        let _ = waiter.send(cancelled_task_result());
    }
}

struct TaskRunOwnerGuard {
    run_state: TaskRunStateRef,
    owner_token: u64,
    released: bool,
}

impl TaskRunOwnerGuard {
    fn new(run_state: TaskRunStateRef, owner_token: u64) -> Self {
        Self {
            run_state,
            owner_token,
            released: false,
        }
    }

    fn mark_released(&mut self) {
        self.released = true;
    }
}

impl Drop for TaskRunOwnerGuard {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let (control, waiters, owned) = {
            let mut state = lock_unpoisoned(&self.run_state);
            if state.owner_token != Some(self.owner_token) {
                (Arc::clone(&state.control), Vec::new(), false)
            } else {
                state.owner_token = None;
                state.in_flight = false;
                state.pending_force = false;
                state.forced_rerun = false;
                let mut waiters = std::mem::take(&mut state.active_force_waiters);
                waiters.extend(std::mem::take(&mut state.pending_force_waiters));
                (Arc::clone(&state.control), waiters, true)
            }
        };
        if owned {
            control.owner_finished();
            for waiter in waiters {
                let _ = waiter.send(cancelled_task_result());
            }
        }
    }
}

enum TaskRunClaim {
    Owner(u64),
    Skip,
    Wait(oneshot::Receiver<AppResult<()>>),
    Closed,
}

async fn run_scheduled_task<F, Fut>(
    run_state: TaskRunStateRef,
    force_if_busy: bool,
    execute: F,
) -> AppResult<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let claim = {
        let mut state = lock_unpoisoned(&run_state);
        if state.closed || state.control.is_cancelled() {
            TaskRunClaim::Closed
        } else if state.in_flight {
            if force_if_busy {
                let (sender, receiver) = oneshot::channel();
                if state.forced_rerun {
                    state.active_force_waiters.push(sender);
                } else {
                    state.pending_force = true;
                    state.pending_force_waiters.push(sender);
                }
                TaskRunClaim::Wait(receiver)
            } else {
                TaskRunClaim::Skip
            }
        } else {
            let Some(owner_token) = state.next_owner_token.checked_add(1) else {
                state.closed = true;
                return cancelled_task_result();
            };
            state.next_owner_token = owner_token;
            state.in_flight = true;
            state.owner_token = Some(owner_token);
            state.forced_rerun = false;
            state.control.owner_started();
            TaskRunClaim::Owner(owner_token)
        }
    };
    match claim {
        TaskRunClaim::Owner(owner_token) => {
            let mut owner = TaskRunOwnerGuard::new(Arc::clone(&run_state), owner_token);
            let result = drain_scheduled_runs(&run_state, owner_token, execute).await;
            owner.mark_released();
            result
        }
        TaskRunClaim::Skip => Ok(()),
        TaskRunClaim::Wait(receiver) => receiver.await.unwrap_or_else(|_| cancelled_task_result()),
        TaskRunClaim::Closed => cancelled_task_result(),
    }
}

async fn run_reschedulable_periodic<F, Fut>(
    running: Arc<AtomicBool>,
    run_state: TaskRunStateRef,
    mut intervals: tokio::sync::watch::Receiver<Duration>,
    first_delay: Duration,
    mut execute: F,
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let initial = *intervals.borrow();
    let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + first_delay, initial);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            changed = intervals.changed() => {
                if changed.is_err() || !running.load(Ordering::Relaxed) {
                    break;
                }
                let next = *intervals.borrow_and_update();
                ticker = tokio::time::interval_at(tokio::time::Instant::now() + next, next);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            }
            _ = ticker.tick() => {
                if !running.load(Ordering::Relaxed) {
                    break;
                }
                let _ = run_scheduled_task(Arc::clone(&run_state), false, &mut execute).await;
            }
        }
    }
}

async fn run_fixed_periodic<F, Fut>(
    running: Arc<AtomicBool>,
    run_state: TaskRunStateRef,
    first_delay: Duration,
    interval: Duration,
    mut execute: F,
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + first_delay, interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        if !running.load(Ordering::Relaxed) {
            break;
        }
        let _ = run_scheduled_task(Arc::clone(&run_state), false, &mut execute).await;
    }
}

async fn run_notification_maintenance_once(
    notification: &Arc<NotificationRuntime>,
    config: &Arc<RwLock<AppConfig>>,
    now_ms: u64,
) {
    let NotificationRuntime::Available(service) = notification.as_ref() else {
        return;
    };
    let configured_accounts: HashSet<String> = {
        let config = config.read().await;
        crate::services::account_profiles::normalize_account_ids(
            &config.accounts,
            &config.active_account_id,
        )
        .into_iter()
        .collect()
    };
    if let Err(error) = service.prune(&configured_accounts, now_ms).await {
        tracing::warn!(code = error.code(), "notification maintenance failed");
    }
}

async fn drain_scheduled_runs<F, Fut>(
    run_state: &TaskRunStateRef,
    owner_token: u64,
    mut execute: F,
) -> AppResult<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let control = {
        let state = lock_unpoisoned(run_state);
        Arc::clone(&state.control)
    };
    loop {
        let cancellation = wait_for_generation_cancel(control.subscribe());
        tokio::pin!(cancellation);
        let result = tokio::select! {
            biased;
            _ = &mut cancellation => cancelled_task_result(),
            result = execute() => result,
        };
        let (completed_waiters, finished, release_owner, result) = {
            let mut state = lock_unpoisoned(run_state);
            if state.owner_token != Some(owner_token) {
                (Vec::new(), true, false, cancelled_task_result())
            } else if state.closed || control.is_cancelled() {
                state.owner_token = None;
                state.in_flight = false;
                state.pending_force = false;
                state.forced_rerun = false;
                let mut waiters = std::mem::take(&mut state.active_force_waiters);
                waiters.extend(std::mem::take(&mut state.pending_force_waiters));
                (waiters, true, true, cancelled_task_result())
            } else if !state.forced_rerun && state.pending_force {
                state.pending_force = false;
                state.forced_rerun = true;
                let pending = std::mem::take(&mut state.pending_force_waiters);
                state.active_force_waiters.extend(pending);
                (Vec::new(), false, false, result)
            } else {
                state.owner_token = None;
                state.in_flight = false;
                state.forced_rerun = false;
                (
                    std::mem::take(&mut state.active_force_waiters),
                    true,
                    true,
                    result,
                )
            }
        };
        if !finished {
            continue;
        }
        if release_owner {
            control.owner_finished();
        }
        for waiter in completed_waiters {
            let _ = waiter.send(result.clone());
        }
        return result;
    }
}

type SchedulerHandle = tauri::async_runtime::JoinHandle<()>;

#[derive(Default)]
struct SchedulerHandleBatch {
    handles: Vec<(TaskId, SchedulerHandle)>,
}

impl SchedulerHandleBatch {
    fn into_handles(mut self) -> Vec<(TaskId, SchedulerHandle)> {
        std::mem::take(&mut self.handles)
    }
}

impl Drop for SchedulerHandleBatch {
    fn drop(&mut self) {
        for (_, handle) in self.handles.drain(..) {
            handle.abort();
        }
    }
}

impl FromIterator<(TaskId, SchedulerHandle)> for SchedulerHandleBatch {
    fn from_iter<T: IntoIterator<Item = (TaskId, SchedulerHandle)>>(iter: T) -> Self {
        let mut batch = Self::default();
        for handle in iter {
            batch.handles.push(handle);
        }
        batch
    }
}

trait IntoSchedulerHandleBatch {
    fn into_scheduler_handle_batch(self) -> SchedulerHandleBatch;
}

impl IntoSchedulerHandleBatch for SchedulerHandleBatch {
    fn into_scheduler_handle_batch(self) -> SchedulerHandleBatch {
        self
    }
}

impl IntoSchedulerHandleBatch for Vec<(TaskId, SchedulerHandle)> {
    fn into_scheduler_handle_batch(self) -> SchedulerHandleBatch {
        self.into_iter().collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchedulerLifecycleState {
    Stopped,
    Starting { generation: u64 },
    Running { generation: u64 },
    Stopping { generation: u64 },
}

struct SchedulerLifecycle {
    state: SchedulerLifecycleState,
    next_generation: u64,
    handles: HashMap<TaskId, SchedulerHandle>,
    run_states: Option<Arc<HashMap<TaskId, TaskRunStateRef>>>,
    generation_running: Option<Arc<AtomicBool>>,
    control: Option<Arc<GenerationControl>>,
    cleanup_completion: Option<watch::Receiver<bool>>,
}

impl SchedulerLifecycle {
    fn new() -> Self {
        Self {
            state: SchedulerLifecycleState::Stopped,
            next_generation: 0,
            handles: HashMap::new(),
            run_states: None,
            generation_running: None,
            control: None,
            cleanup_completion: None,
        }
    }

    fn begin_start(
        &mut self,
    ) -> Option<(
        u64,
        Arc<HashMap<TaskId, TaskRunStateRef>>,
        Arc<AtomicBool>,
        Arc<GenerationControl>,
    )> {
        if self.state != SchedulerLifecycleState::Stopped {
            return None;
        }
        self.next_generation = self.next_generation.checked_add(1)?;
        let generation = self.next_generation;
        let control = GenerationControl::new();
        let run_states = new_task_run_states(&control);
        let generation_running = Arc::new(AtomicBool::new(true));
        self.state = SchedulerLifecycleState::Starting { generation };
        self.run_states = Some(Arc::clone(&run_states));
        self.generation_running = Some(Arc::clone(&generation_running));
        self.control = Some(Arc::clone(&control));
        Some((generation, run_states, generation_running, control))
    }

    fn install_handles(
        &mut self,
        generation: u64,
        handles: Vec<(TaskId, SchedulerHandle)>,
    ) -> Result<(), Vec<(TaskId, SchedulerHandle)>> {
        if self.state != (SchedulerLifecycleState::Starting { generation }) {
            return Err(handles);
        }
        for (task, handle) in handles {
            if let Some(previous) = self.handles.insert(task, handle) {
                previous.abort();
            }
        }
        Ok(())
    }

    fn finish_start(&mut self, generation: u64) -> bool {
        if self.state != (SchedulerLifecycleState::Starting { generation })
            || self
                .control
                .as_ref()
                .is_none_or(|control| control.is_cancelled())
        {
            return false;
        }
        self.state = SchedulerLifecycleState::Running { generation };
        true
    }

    fn active_generation(&self) -> Option<u64> {
        match self.state {
            SchedulerLifecycleState::Starting { generation }
            | SchedulerLifecycleState::Running { generation }
            | SchedulerLifecycleState::Stopping { generation } => Some(generation),
            SchedulerLifecycleState::Stopped => None,
        }
    }

    fn run_state(&self, task: TaskId) -> Option<TaskRunStateRef> {
        if !matches!(self.state, SchedulerLifecycleState::Running { .. }) {
            return None;
        }
        self.run_states
            .as_ref()
            .and_then(|states| states.get(&task))
            .cloned()
    }

    fn is_starting(&self, generation: u64) -> bool {
        self.state == (SchedulerLifecycleState::Starting { generation })
    }

    fn begin_cleanup(&mut self, generation: u64) -> CleanupRequest {
        match self.state {
            SchedulerLifecycleState::Stopping { generation: active } if active == generation => {
                return self
                    .cleanup_completion
                    .as_ref()
                    .cloned()
                    .map(CleanupRequest::Wait)
                    .unwrap_or(CleanupRequest::Inactive);
            }
            SchedulerLifecycleState::Starting { generation: active }
            | SchedulerLifecycleState::Running { generation: active }
                if active == generation => {}
            _ => return CleanupRequest::Inactive,
        }

        let (completion, receiver) = watch::channel(false);
        self.state = SchedulerLifecycleState::Stopping { generation };
        self.cleanup_completion = Some(receiver.clone());
        CleanupRequest::Start(CleanupResources {
            generation,
            handles: std::mem::take(&mut self.handles).into_values().collect(),
            run_states: self.run_states.take(),
            generation_running: self.generation_running.take(),
            control: self.control.take(),
            completion,
            receiver,
        })
    }

    fn finish_cleanup(&mut self, generation: u64) -> bool {
        if self.state != (SchedulerLifecycleState::Stopping { generation }) {
            return false;
        }
        self.state = SchedulerLifecycleState::Stopped;
        self.handles.clear();
        self.run_states = None;
        self.generation_running = None;
        self.control = None;
        self.cleanup_completion = None;
        true
    }

    fn cleanup_receiver(&self) -> Option<watch::Receiver<bool>> {
        if matches!(self.state, SchedulerLifecycleState::Stopping { .. }) {
            self.cleanup_completion.clone()
        } else {
            None
        }
    }

    #[cfg(test)]
    fn running_generation(&self) -> Option<u64> {
        match self.state {
            SchedulerLifecycleState::Running { generation } => Some(generation),
            SchedulerLifecycleState::Stopped
            | SchedulerLifecycleState::Starting { .. }
            | SchedulerLifecycleState::Stopping { .. } => None,
        }
    }

    #[cfg(test)]
    fn handle_count(&self) -> usize {
        self.handles.len()
    }

    #[cfg(test)]
    fn has_handle(&self, task: TaskId) -> bool {
        self.handles.contains_key(&task)
    }
}

struct CleanupResources {
    generation: u64,
    handles: Vec<SchedulerHandle>,
    run_states: Option<Arc<HashMap<TaskId, TaskRunStateRef>>>,
    generation_running: Option<Arc<AtomicBool>>,
    control: Option<Arc<GenerationControl>>,
    completion: watch::Sender<bool>,
    receiver: watch::Receiver<bool>,
}

enum CleanupRequest {
    Start(CleanupResources),
    Wait(watch::Receiver<bool>),
    Inactive,
}

struct CleanupCompletionGuard {
    lifecycle: Arc<StdMutex<SchedulerLifecycle>>,
    running: Arc<AtomicBool>,
    generation: u64,
    completion: Option<watch::Sender<bool>>,
}

impl Drop for CleanupCompletionGuard {
    fn drop(&mut self) {
        let finished = lock_unpoisoned(&self.lifecycle).finish_cleanup(self.generation);
        if finished {
            self.running.store(false, Ordering::SeqCst);
        }
        if let Some(completion) = self.completion.take() {
            completion.send_replace(true);
        }
    }
}

async fn wait_for_cleanup(mut completion: watch::Receiver<bool>) {
    let _ = completion.wait_for(|finished| *finished).await;
}

fn request_scheduler_cleanup(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    running: &Arc<AtomicBool>,
    generation: u64,
) -> Option<watch::Receiver<bool>> {
    let request = lock_unpoisoned(lifecycle).begin_cleanup(generation);
    match request {
        CleanupRequest::Wait(receiver) => Some(receiver),
        CleanupRequest::Inactive => None,
        CleanupRequest::Start(mut resources) => {
            running.store(false, Ordering::SeqCst);
            let receiver = resources.receiver.clone();
            if let Some(generation_running) = &resources.generation_running {
                generation_running.store(false, Ordering::SeqCst);
            }
            if let Some(control) = &resources.control {
                control.cancel();
            }
            if let Some(run_states) = &resources.run_states {
                for run_state in run_states.values() {
                    cancel_task_run_state(run_state);
                }
            }
            let lifecycle = Arc::clone(lifecycle);
            let running = Arc::clone(running);
            tauri::async_runtime::spawn(async move {
                let _guard = CleanupCompletionGuard {
                    lifecycle,
                    running,
                    generation: resources.generation,
                    completion: Some(resources.completion),
                };
                for handle in resources.handles.drain(..) {
                    let _ = handle.await;
                }
                if let Some(control) = resources.control {
                    control.wait_drained().await;
                }
            });
            Some(receiver)
        }
    }
}

async fn supervise_scheduler_handle(
    task: TaskId,
    generation: u64,
    mut handle: SchedulerHandle,
    control: Arc<GenerationControl>,
    lifecycle: Arc<StdMutex<SchedulerLifecycle>>,
    running: Arc<AtomicBool>,
    ready: oneshot::Sender<bool>,
) {
    let completed_before_start = std::future::poll_fn(|context| {
        let completed = match std::future::Future::poll(std::pin::Pin::new(&mut handle), context) {
            std::task::Poll::Ready(_) => true,
            std::task::Poll::Pending => false,
        };
        std::task::Poll::Ready(completed)
    })
    .await;
    let alive = !completed_before_start && !control.is_cancelled();
    let _ = ready.send(alive);
    if !alive {
        if completed_before_start && !control.is_cancelled() {
            control.cancel();
            tracing::warn!(
                ?task,
                generation,
                "scheduler periodic loop exited during start"
            );
            let _ = request_scheduler_cleanup(&lifecycle, &running, generation);
        }
        return;
    }

    let cancellation = wait_for_generation_cancel(control.subscribe());
    tokio::pin!(cancellation);
    let unexpected = tokio::select! {
        biased;
        _ = &mut cancellation => {
            handle.abort();
            let _ = handle.await;
            false
        }
        _ = &mut handle => !control.is_cancelled(),
    };
    if unexpected {
        control.cancel();
        tracing::warn!(
            ?task,
            generation,
            "scheduler periodic loop exited unexpectedly"
        );
        let _ = request_scheduler_cleanup(&lifecycle, &running, generation);
    }
}

struct SchedulerStartGuard {
    lifecycle: Arc<StdMutex<SchedulerLifecycle>>,
    running: Arc<AtomicBool>,
    control: Arc<GenerationControl>,
    generation: u64,
    committed: bool,
    activity_released: bool,
}

impl SchedulerStartGuard {
    fn commit(&mut self) {
        self.committed = true;
        self.release_activity();
    }

    fn release_activity(&mut self) {
        if !self.activity_released {
            self.control.owner_finished();
            self.activity_released = true;
        }
    }
}

impl Drop for SchedulerStartGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let _ = request_scheduler_cleanup(&self.lifecycle, &self.running, self.generation);
        self.release_activity();
    }
}

async fn start_scheduler_lifecycle<Prepare, PrepareFut, Prepared, Install, Handles, FinishFut>(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    running: &Arc<AtomicBool>,
    prepare: Prepare,
    install: Install,
) -> bool
where
    Prepare: FnOnce(u64) -> PrepareFut,
    PrepareFut: std::future::Future<Output = Prepared>,
    Install: FnOnce(
        u64,
        Arc<HashMap<TaskId, TaskRunStateRef>>,
        Arc<AtomicBool>,
        Prepared,
    ) -> (Handles, FinishFut),
    Handles: IntoSchedulerHandleBatch,
    FinishFut: std::future::Future<Output = ()>,
{
    loop {
        let waiting = lock_unpoisoned(lifecycle).cleanup_receiver();
        let Some(waiting) = waiting else {
            break;
        };
        wait_for_cleanup(waiting).await;
    }
    let Some((generation, run_states, generation_running, control)) =
        lock_unpoisoned(lifecycle).begin_start()
    else {
        return false;
    };
    running.store(true, Ordering::SeqCst);
    control.owner_started();
    let mut guard = SchedulerStartGuard {
        lifecycle: Arc::clone(lifecycle),
        running: Arc::clone(running),
        control: Arc::clone(&control),
        generation,
        committed: false,
        activity_released: false,
    };
    let cancellation = wait_for_generation_cancel(control.subscribe());
    let prepared = prepare(generation);
    tokio::pin!(cancellation);
    tokio::pin!(prepared);
    let prepared = tokio::select! {
        biased;
        _ = &mut cancellation => return false,
        prepared = &mut prepared => prepared,
    };
    if !lock_unpoisoned(lifecycle).is_starting(generation) {
        return false;
    }
    let (handles, finish_start) = install(
        generation,
        Arc::clone(&run_states),
        Arc::clone(&generation_running),
        prepared,
    );
    let (supervised, ready_receivers): (Vec<_>, Vec<_>) = handles
        .into_scheduler_handle_batch()
        .into_handles()
        .into_iter()
        .map(|(task, handle)| {
            let (ready, receiver) = oneshot::channel();
            let supervisor = tauri::async_runtime::spawn(supervise_scheduler_handle(
                task,
                generation,
                handle,
                Arc::clone(&control),
                Arc::clone(lifecycle),
                Arc::clone(running),
                ready,
            ));
            ((task, supervisor), receiver)
        })
        .unzip();
    let install_result = {
        let mut lifecycle = lock_unpoisoned(lifecycle);
        lifecycle.install_handles(generation, supervised)
    };
    if let Err(rejected) = install_result {
        control.cancel();
        for (_, handle) in rejected {
            let _ = handle.await;
        }
        return false;
    }
    for ready in ready_receivers {
        let cancellation = wait_for_generation_cancel(control.subscribe());
        tokio::pin!(cancellation);
        let alive = tokio::select! {
            biased;
            _ = &mut cancellation => false,
            alive = ready => alive.unwrap_or(false),
        };
        if !alive {
            return false;
        }
    }
    let cancellation = wait_for_generation_cancel(control.subscribe());
    tokio::pin!(cancellation);
    tokio::pin!(finish_start);
    tokio::select! {
        biased;
        _ = &mut cancellation => return false,
        _ = &mut finish_start => {}
    }
    tokio::task::yield_now().await;
    let committed = lock_unpoisoned(lifecycle).finish_start(generation);
    if committed {
        guard.commit();
    }
    committed
}

async fn stop_scheduler_lifecycle(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    running: &Arc<AtomicBool>,
) -> bool {
    let generation = lock_unpoisoned(lifecycle).active_generation();
    let Some(generation) = generation else {
        return false;
    };
    if let Some(completion) = request_scheduler_cleanup(lifecycle, running, generation) {
        wait_for_cleanup(completion).await;
    }
    true
}

pub struct SchedulerService {
    time: Arc<TimeService>,
    market: Arc<MarketService>,
    chart_workspace: Arc<ChartWorkspaceService>,
    trading: Arc<TradingService>,
    daily_pnl: Arc<DailyPnlService>,
    connection: Arc<ConnectionService>,
    ws: Arc<WsManager>,
    config: Arc<RwLock<AppConfig>>,
    notification: Arc<NotificationRuntime>,
    emitter: EventEmitter,
    api: Arc<crate::api::ApiClient>,
    environment_status: Arc<RwLock<EnvironmentStatus>>,
    account_lifecycle: Arc<AccountLifecycleCoordinator>,
    running: Arc<AtomicBool>,
    lifecycle: Arc<StdMutex<SchedulerLifecycle>>,
    kline_flush_gate: Arc<StdMutex<()>>,
    market_fallback_interval_tx: tokio::sync::watch::Sender<Duration>,
    prev_public_connected: Arc<Mutex<bool>>,
    prev_private_connected: Arc<Mutex<bool>>,
}

impl SchedulerService {
    pub fn new(
        time: Arc<TimeService>,
        market: Arc<MarketService>,
        chart_workspace: Arc<ChartWorkspaceService>,
        trading: Arc<TradingService>,
        daily_pnl: Arc<DailyPnlService>,
        connection: Arc<ConnectionService>,
        ws: Arc<WsManager>,
        config: Arc<RwLock<AppConfig>>,
        notification: Arc<NotificationRuntime>,
        emitter: EventEmitter,
        api: Arc<crate::api::ApiClient>,
        environment_status: Arc<RwLock<EnvironmentStatus>>,
        account_lifecycle: Arc<AccountLifecycleCoordinator>,
    ) -> Self {
        let (market_fallback_interval_tx, _) =
            tokio::sync::watch::channel(INTERVAL_MARKET_FALLBACK);
        Self {
            time,
            market,
            chart_workspace,
            trading,
            daily_pnl,
            connection,
            ws,
            config,
            notification,
            emitter,
            api,
            environment_status,
            account_lifecycle,
            running: Arc::new(AtomicBool::new(false)),
            lifecycle: Arc::new(StdMutex::new(SchedulerLifecycle::new())),
            kline_flush_gate: Arc::new(StdMutex::new(())),
            market_fallback_interval_tx,
            prev_public_connected: Arc::new(Mutex::new(false)),
            prev_private_connected: Arc::new(Mutex::new(false)),
        }
    }

    pub async fn start(&self) {
        start_scheduler_lifecycle(
            &self.lifecycle,
            &self.running,
            |_| async {
                let config = self.config.read().await;
                configured_task_interval(TaskId::MarketFallback, &config)
                    .expect("market fallback has an interval")
            },
            |generation, run_states, generation_running, market_fallback_interval| {
                self.market_fallback_interval_tx
                    .send_replace(market_fallback_interval);
                let handles: SchedulerHandleBatch = periodic_task_ids()
                    .iter()
                    .copied()
                    .map(|task| {
                        let run_state = run_states.get(&task).expect("task registered").clone();
                        let handle = if task == TaskId::MarketFallback {
                            self.spawn_market_fallback(run_state, Arc::clone(&generation_running))
                        } else {
                            self.spawn_periodic(
                                task,
                                task.interval().expect("periodic task"),
                                run_state,
                                Arc::clone(&generation_running),
                            )
                        };
                        (task, handle)
                    })
                    .collect();
                let lifecycle = Arc::clone(&self.lifecycle);
                let finish_start = async move {
                    let time_state = run_states
                        .get(&TaskId::TimeSync)
                        .expect("time task registered")
                        .clone();
                    let _ = run_scheduled_task(time_state, true, || self.execute(TaskId::TimeSync))
                        .await;
                    if lock_unpoisoned(&lifecycle).is_starting(generation) {
                        let environment_state = run_states
                            .get(&TaskId::Environment)
                            .expect("environment task registered")
                            .clone();
                        let _ = run_scheduled_task(environment_state, true, || {
                            self.execute(TaskId::Environment)
                        })
                        .await;
                    }
                };
                (handles, finish_start)
            },
        )
        .await;
    }

    pub async fn stop(&self) {
        stop_scheduler_lifecycle(&self.lifecycle, &self.running).await;
    }

    pub fn set_market_fallback_interval(&self, interval: Duration) {
        self.market_fallback_interval_tx.send_replace(interval);
    }

    pub async fn bootstrap_connection(&self) -> AppResult<()> {
        let _guard = self.account_lifecycle.read_guard().await;
        let connected = self.connection.status().await == ConnectionStatus::Connected;
        let tasks = bootstrap_tasks(connected);
        // The lifecycle guard is already held: call the non-locking inner path directly.
        // This also keeps strict snapshots independent from regular task coalescing.
        let failed = run_bootstrap_tasks(&tasks, |task| {
            self.execute_inner(task, ExecutionMode::Bootstrap)
        })
        .await;
        for task in &failed {
            self.emitter
                .emit_error(&format!("连接后{}同步失败", task.bootstrap_label()));
        }
        if failed.is_empty() {
            Ok(())
        } else {
            Err(bootstrap_failure_error(&failed))
        }
    }

    pub async fn run_now(&self, task: TaskId, force: bool) -> AppResult<()> {
        let run_state = lock_unpoisoned(&self.lifecycle)
            .run_state(task)
            .ok_or_else(|| crate::error::AppError::Internal("调度器未运行".into()))?;
        run_scheduled_task(run_state, force, || self.execute(task)).await
    }

    fn spawn_periodic(
        &self,
        task: TaskId,
        interval: Duration,
        run_state: TaskRunStateRef,
        generation_running: Arc<AtomicBool>,
    ) -> SchedulerHandle {
        let scheduler = self.clone_refs();
        tauri::async_runtime::spawn(async move {
            run_fixed_periodic(
                generation_running,
                run_state,
                first_tick_delay(task),
                interval,
                || scheduler.execute(task),
            )
            .await;
        })
    }

    fn spawn_market_fallback(
        &self,
        run_state: TaskRunStateRef,
        generation_running: Arc<AtomicBool>,
    ) -> SchedulerHandle {
        let scheduler = self.clone_refs();
        let receiver = self.market_fallback_interval_tx.subscribe();
        tauri::async_runtime::spawn(run_reschedulable_periodic(
            generation_running,
            run_state,
            receiver,
            Duration::ZERO,
            move || {
                let scheduler = scheduler.clone();
                async move { scheduler.execute(TaskId::MarketFallback).await }
            },
        ))
    }

    fn clone_refs(&self) -> SchedulerRefs {
        SchedulerRefs {
            time: self.time.clone(),
            market: self.market.clone(),
            chart_workspace: self.chart_workspace.clone(),
            trading: self.trading.clone(),
            daily_pnl: self.daily_pnl.clone(),
            connection: self.connection.clone(),
            ws: self.ws.clone(),
            config: self.config.clone(),
            notification: self.notification.clone(),
            emitter: self.emitter.clone(),
            api: self.api.clone(),
            environment_status: self.environment_status.clone(),
            account_lifecycle: self.account_lifecycle.clone(),
            kline_flush_gate: self.kline_flush_gate.clone(),
            prev_public_connected: self.prev_public_connected.clone(),
            prev_private_connected: self.prev_private_connected.clone(),
        }
    }

    async fn execute(&self, task: TaskId) -> AppResult<()> {
        execute_task_with_account_lifecycle(self.account_lifecycle.as_ref(), task, || {
            self.execute_inner(task, ExecutionMode::Periodic)
        })
        .await
    }

    async fn execute_inner(&self, task: TaskId, mode: ExecutionMode) -> AppResult<()> {
        match task {
            TaskId::TimeSync => run_time_sync_task(&self.time).await,
            TaskId::FundingRate => self.run_funding_rate().await,
            TaskId::Balances => self.run_balances(mode).await,
            TaskId::PrivatePanels => self.run_private_panels(mode).await,
            TaskId::DailyPnl => self.daily_pnl.refresh().await.map(|_| ()),
            TaskId::MarketFallback => self.run_market_fallback(mode).await,
            TaskId::KlineFlush => {
                execute_kline_flush(self.chart_workspace.clone(), self.kline_flush_gate.clone())
                    .await
            }
            TaskId::Environment => self.run_environment().await,
            TaskId::NotificationMaintenance => {
                run_notification_maintenance_once(
                    &self.notification,
                    &self.config,
                    self.time.local_now_ms(),
                )
                .await;
                Ok(())
            }
        }
    }

    async fn run_funding_rate(&self) -> AppResult<()> {
        let symbol = self.market.active_symbol().await;
        self.market.refresh_funding_rate(&symbol).await
    }

    async fn run_balances(&self, mode: ExecutionMode) -> AppResult<()> {
        if self.connection.status().await != ConnectionStatus::Connected {
            return Ok(());
        }
        run_rest_snapshot_if_needed(
            mode,
            self.ws.is_balance_healthy(PRIVATE_STALE_MS),
            || async {
                let account_id = {
                    let cfg = self.config.read().await;
                    normalize_account_id(&cfg.active_account_id)
                };
                let params = build_order_query_params(
                    None, None, None, None, None, None, None, None, None, None,
                );
                let payload = self.api.private_get(endpoints::BALANCES, params).await?;
                let balances = parse_balances(&payload);
                warn_if_parse_empty(&self.emitter, "account/balance", &payload, balances.len());
                let total_equity = balances
                    .iter()
                    .map(|b| b.total.parse::<f64>().unwrap_or(0.0))
                    .sum::<f64>()
                    .to_string();
                self.emitter.emit_account_snapshot(AccountSummary {
                    account_id,
                    balances,
                    total_equity,
                });
                Ok(())
            },
        )
        .await
    }

    async fn run_private_panels(&self, mode: ExecutionMode) -> AppResult<()> {
        if self.connection.status().await != ConnectionStatus::Connected {
            return Ok(());
        }
        run_rest_snapshot_if_needed(
            mode,
            self.ws.is_private_panels_healthy(PRIVATE_STALE_MS),
            || async {
                let symbol = self.market.active_symbol().await;
                let sym = Some(symbol.as_str());
                let open_orders = self.trading.fetch_open_orders(sym).await?;
                let order_history = self.trading.fetch_order_history(sym, Some(50)).await?;
                let params = build_order_query_params(
                    sym, None, None, None, None, None, None, None, None, None,
                );
                let payload = self.api.private_get(endpoints::POSITIONS, params).await?;
                let meta = list_envelope_meta(&payload);
                let positions = parse_positions(&payload);
                warn_if_parse_empty(&self.emitter, "position/list", &payload, positions.len());
                warn_if_raw_parsed_mismatch(&self.emitter, "position/list", &meta, positions.len());
                self.emitter
                    .emit_private_panels_snapshot(PrivatePanelsSnapshot {
                        open_orders,
                        order_history,
                        positions,
                    });
                Ok(())
            },
        )
        .await
    }

    async fn run_market_fallback(&self, mode: ExecutionMode) -> AppResult<()> {
        let symbol = self.market.active_symbol().await;
        let public_connected = self.ws.is_public_connected();
        let private_connected = self.ws.is_private_connected();
        {
            let mut prev = self.prev_public_connected.lock().await;
            if public_connected && !*prev {
                let interval = self.market.kline_interval().await;
                self.market.schedule_kline_backfill(&symbol, &interval);
            }
            *prev = public_connected;
        }
        {
            let mut prev = self.prev_private_connected.lock().await;
            *prev = private_connected;
        }

        run_rest_snapshot_if_needed(mode, self.ws.is_market_healthy(PUBLIC_STALE_MS), || async {
            let mut failures = Vec::new();
            if let Err(error) = self.market.refresh_ticker_depth(&symbol).await {
                failures.push(error.to_string());
            }
            if let Err(error) = self.market.refresh_klines(&symbol).await {
                failures.push(error.to_string());
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(crate::error::AppError::Internal(failures.join("; ")))
            }
        })
        .await
    }

    async fn run_environment(&self) -> AppResult<()> {
        probe_environment(
            &self.config,
            &self.time,
            &self.emitter,
            &self.environment_status,
        )
        .await
    }
}

#[derive(Clone)]
struct SchedulerRefs {
    time: Arc<TimeService>,
    market: Arc<MarketService>,
    chart_workspace: Arc<ChartWorkspaceService>,
    trading: Arc<TradingService>,
    daily_pnl: Arc<DailyPnlService>,
    connection: Arc<ConnectionService>,
    ws: Arc<WsManager>,
    config: Arc<RwLock<AppConfig>>,
    notification: Arc<NotificationRuntime>,
    emitter: EventEmitter,
    api: Arc<crate::api::ApiClient>,
    environment_status: Arc<RwLock<EnvironmentStatus>>,
    account_lifecycle: Arc<AccountLifecycleCoordinator>,
    kline_flush_gate: Arc<StdMutex<()>>,
    prev_public_connected: Arc<Mutex<bool>>,
    prev_private_connected: Arc<Mutex<bool>>,
}

impl SchedulerRefs {
    async fn execute(&self, task: TaskId) -> AppResult<()> {
        execute_task_with_account_lifecycle(self.account_lifecycle.as_ref(), task, || {
            self.execute_inner(task, ExecutionMode::Periodic)
        })
        .await
    }

    async fn execute_inner(&self, task: TaskId, mode: ExecutionMode) -> AppResult<()> {
        match task {
            TaskId::TimeSync => run_time_sync_task(&self.time).await,
            TaskId::FundingRate => {
                let symbol = self.market.active_symbol().await;
                self.market.refresh_funding_rate(&symbol).await
            }
            TaskId::Balances => self.run_balances(mode).await,
            TaskId::PrivatePanels => self.run_private_panels(mode).await,
            TaskId::DailyPnl => self.daily_pnl.refresh().await.map(|_| ()),
            TaskId::MarketFallback => self.run_market_fallback(mode).await,
            TaskId::KlineFlush => {
                execute_kline_flush(self.chart_workspace.clone(), self.kline_flush_gate.clone())
                    .await
            }
            TaskId::Environment => self.run_environment().await,
            TaskId::NotificationMaintenance => {
                run_notification_maintenance_once(
                    &self.notification,
                    &self.config,
                    self.time.local_now_ms(),
                )
                .await;
                Ok(())
            }
        }
    }

    async fn run_balances(&self, mode: ExecutionMode) -> AppResult<()> {
        if self.connection.status().await != ConnectionStatus::Connected {
            return Ok(());
        }
        run_rest_snapshot_if_needed(
            mode,
            self.ws.is_balance_healthy(PRIVATE_STALE_MS),
            || async {
                let account_id = {
                    let cfg = self.config.read().await;
                    normalize_account_id(&cfg.active_account_id)
                };
                let params = build_order_query_params(
                    None, None, None, None, None, None, None, None, None, None,
                );
                let payload = self.api.private_get(endpoints::BALANCES, params).await?;
                let balances = parse_balances(&payload);
                warn_if_parse_empty(&self.emitter, "account/balance", &payload, balances.len());
                let total_equity = balances
                    .iter()
                    .map(|b| b.total.parse::<f64>().unwrap_or(0.0))
                    .sum::<f64>()
                    .to_string();
                self.emitter.emit_account_snapshot(AccountSummary {
                    account_id,
                    balances,
                    total_equity,
                });
                Ok(())
            },
        )
        .await
    }

    async fn run_private_panels(&self, mode: ExecutionMode) -> AppResult<()> {
        if self.connection.status().await != ConnectionStatus::Connected {
            return Ok(());
        }
        run_rest_snapshot_if_needed(
            mode,
            self.ws.is_private_panels_healthy(PRIVATE_STALE_MS),
            || async {
                let symbol = self.market.active_symbol().await;
                let sym = Some(symbol.as_str());
                let open_orders = self.trading.fetch_open_orders(sym).await?;
                let order_history = self.trading.fetch_order_history(sym, Some(50)).await?;
                let params = build_order_query_params(
                    sym, None, None, None, None, None, None, None, None, None,
                );
                let payload = self.api.private_get(endpoints::POSITIONS, params).await?;
                let meta = list_envelope_meta(&payload);
                let positions = parse_positions(&payload);
                warn_if_parse_empty(&self.emitter, "position/list", &payload, positions.len());
                warn_if_raw_parsed_mismatch(&self.emitter, "position/list", &meta, positions.len());
                self.emitter
                    .emit_private_panels_snapshot(PrivatePanelsSnapshot {
                        open_orders,
                        order_history,
                        positions,
                    });
                Ok(())
            },
        )
        .await
    }

    async fn run_market_fallback(&self, mode: ExecutionMode) -> AppResult<()> {
        let symbol = self.market.active_symbol().await;
        let public_connected = self.ws.is_public_connected();
        {
            let mut prev = self.prev_public_connected.lock().await;
            if public_connected && !*prev {
                let interval = self.market.kline_interval().await;
                self.market.schedule_kline_backfill(&symbol, &interval);
            }
            *prev = public_connected;
        }
        {
            let mut prev = self.prev_private_connected.lock().await;
            *prev = self.ws.is_private_connected();
        }
        run_rest_snapshot_if_needed(mode, self.ws.is_market_healthy(PUBLIC_STALE_MS), || async {
            let mut failures = Vec::new();
            if let Err(error) = self.market.refresh_ticker_depth(&symbol).await {
                failures.push(error.to_string());
            }
            if let Err(error) = self.market.refresh_klines(&symbol).await {
                failures.push(error.to_string());
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(crate::error::AppError::Internal(failures.join("; ")))
            }
        })
        .await
    }

    async fn run_environment(&self) -> AppResult<()> {
        probe_environment(
            &self.config,
            &self.time,
            &self.emitter,
            &self.environment_status,
        )
        .await
    }
}

async fn environment_probe_client(base_url: &str) -> ApiClient {
    let client = ApiClient::new();
    client.set_base_url(base_url).await;
    client
}

async fn probe_environment(
    config: &Arc<RwLock<AppConfig>>,
    time: &Arc<TimeService>,
    emitter: &EventEmitter,
    environment_status: &Arc<RwLock<EnvironmentStatus>>,
) -> AppResult<()> {
    let active_account_id = {
        let config = config.read().await;
        normalize_account_id(&config.active_account_id)
    };
    let selected_base_url = CredentialStore::load(&active_account_id)?
        .map(|credential| credential.base_url)
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    let client = environment_probe_client(&selected_base_url).await;
    let base_url = client.base_url().await;
    let checked_at = time.local_now_ms();
    let status = match PublicApi::server_time(&client).await {
        Ok(_) => EnvironmentStatus {
            label: environment_label(&base_url).to_string(),
            base_url,
            reachable: true,
            checked_at,
            error: None,
        },
        Err(error) => EnvironmentStatus {
            label: environment_label(&base_url).to_string(),
            base_url,
            reachable: false,
            checked_at,
            error: Some(error.user_message()),
        },
    };
    publish_environment_task_status(environment_status, status, |status| {
        emitter.emit_environment_updated(status);
    })
    .await
}

#[cfg(test)]
mod tests;
