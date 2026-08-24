use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard};
use std::time::Duration;

use sha2::{Digest, Sha256};
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
    canonical_api_base_url, normalize_account_id, AppConfig, ConnectionStatus, EnvironmentStatus,
    DEFAULT_BASE_URL, MAX_TICKER_POLL_INTERVAL_SECS, MIN_TICKER_POLL_INTERVAL_SECS,
};
use crate::models::notification::NotificationEnvironment;
use crate::models::time::{TimeSnapshot, TimeSyncStatus};
use crate::models::trading::{PrivatePanelsSnapshot, SessionContext};
use crate::services::connection::SessionNotificationObserver;
#[cfg(test)]
use crate::services::market::run_generation_owned_kline_storage;
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
const RECOVERY_BACKOFF_BASE: Duration = Duration::from_millis(100);
const RECOVERY_BACKOFF_CAP: Duration = Duration::from_secs(5);

fn recovery_backoff(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(16);
    let multiplier = 1_u32 << exponent;
    RECOVERY_BACKOFF_BASE
        .checked_mul(multiplier)
        .unwrap_or(RECOVERY_BACKOFF_CAP)
        .min(RECOVERY_BACKOFF_CAP)
}

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
        !matches!(self, Self::KlineFlush | Self::Environment)
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

async fn run_market_reconnect_backfill<F, Fut>(
    previous_connected: Arc<Mutex<bool>>,
    connected: bool,
    operation: F,
) -> AppResult<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let mut previous = previous_connected.lock().await;
    if !connected {
        *previous = false;
        return Ok(());
    }
    if *previous {
        return Ok(());
    }

    // Keep the transition lock for the awaited operation. Cancellation drops
    // the lock without publishing the edge, so a replacement generation will
    // retry; normal success or failure consumes the edge exactly once.
    let result = operation().await;
    *previous = true;
    result
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
    run_bootstrap_tasks_with_errors(tasks, &mut run)
        .await
        .into_iter()
        .map(|(task, _)| task)
        .collect()
}

async fn run_bootstrap_tasks_with_errors<F, Fut>(
    tasks: &[TaskId],
    mut run: F,
) -> Vec<(TaskId, AppError)>
where
    F: FnMut(TaskId) -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let mut failed = Vec::new();
    for task in tasks {
        if let Err(error) = run(*task).await {
            failed.push((*task, error));
        }
    }
    failed
}

async fn run_bootstrap_account_phase<C, CFut, F, Fut>(
    coordinator: &AccountLifecycleCoordinator,
    connected: C,
    run: F,
) -> (Vec<TaskId>, Vec<(TaskId, AppError)>)
where
    C: FnOnce() -> CFut,
    CFut: std::future::Future<Output = bool>,
    F: FnMut(TaskId) -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let _guard = coordinator.read_guard().await;
    let tasks = bootstrap_tasks(connected().await);
    let account_tasks = tasks
        .iter()
        .copied()
        .filter(|task| *task != TaskId::Environment)
        .collect::<Vec<_>>();
    let failed = run_bootstrap_tasks_with_errors(&account_tasks, run).await;
    (tasks, failed)
}

fn bootstrap_failure_needs_generic_error(error: &AppError) -> bool {
    !matches!(
        error,
        AppError::Notified { .. }
            | AppError::Observed(_)
            | AppError::AuthFailure(crate::api::response::AuthFailureKind::SessionExpired)
    )
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

async fn capture_session_context(
    config: &Arc<RwLock<AppConfig>>,
    account_lifecycle: &AccountLifecycleCoordinator,
) -> SessionContext {
    SessionContext {
        account_id: normalize_account_id(&config.read().await.active_account_id),
        session_epoch: account_lifecycle.current_session_epoch(),
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

async fn run_scheduler_owned_market_backfill<F, Fut>(operation: F) -> AppResult<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    // MarketFallback is always invoked beneath the scheduler's outer account
    // read guard (periodic execute or connection bootstrap). Tokio's fair
    // RwLock can deadlock if this inner path attempts to acquire a second read
    // after an account writer has queued.
    operation().await
}

async fn run_serialized_blocking<F, R>(
    gate: Arc<Mutex<()>>,
    operation: F,
) -> Result<R, tokio::task::JoinError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    // Reserve FIFO order before submitting to the blocking pool. Once the
    // reservation is acquired, its owned guard moves into the closure, so
    // cancelling the async waiter cannot let a later shutdown flush overtake
    // an older background flush.
    let guard = gate.lock_owned().await;
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        operation()
    })
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KlineFlushOrigin {
    Background,
    Shutdown,
}

#[derive(Debug, Default)]
struct KlineFlushCoordinator {
    gate: Arc<Mutex<()>>,
}

impl KlineFlushCoordinator {
    fn new() -> Self {
        Self::default()
    }

    async fn run_blocking<F, R>(
        self: Arc<Self>,
        _origin: KlineFlushOrigin,
        operation: F,
    ) -> Result<R, tokio::task::JoinError>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        run_serialized_blocking(Arc::clone(&self.gate), operation).await
    }
}

async fn execute_kline_flush(
    service: Arc<ChartWorkspaceService>,
    coordinator: Arc<KlineFlushCoordinator>,
    origin: KlineFlushOrigin,
) -> AppResult<()> {
    let outcomes = coordinator
        .run_blocking(origin, move || service.flush_dirty_klines())
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
    drain: StdMutex<GenerationDrainState>,
    drained: Notify,
}

#[derive(Debug, Default)]
struct GenerationDrainState {
    active_owners: usize,
    deferred_handles: Vec<DeferredGenerationHandles>,
}

impl GenerationControl {
    fn new() -> Arc<Self> {
        let (cancellation, _) = watch::channel(false);
        Arc::new(Self {
            cancelled: AtomicBool::new(false),
            cancellation,
            drain: StdMutex::new(GenerationDrainState::default()),
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
        let mut drain = lock_unpoisoned(&self.drain);
        drain.active_owners = drain.active_owners.saturating_add(1);
    }

    fn owner_finished(&self) {
        let drained = {
            let mut drain = lock_unpoisoned(&self.drain);
            if drain.active_owners == 0 {
                tracing::error!("scheduler generation owner count underflow");
                false
            } else {
                drain.active_owners -= 1;
                drain.active_owners == 0
            }
        };
        if drained {
            self.drained.notify_waiters();
        }
    }

    fn defer_handles(
        self: &Arc<Self>,
        handles: Vec<SchedulerHandle>,
        owner: Option<GenerationOwnerGuard>,
    ) {
        if handles.is_empty() {
            return;
        }
        for handle in &handles {
            handle.abort();
        }
        let owner = owner.unwrap_or_else(|| GenerationOwnerGuard::new(Arc::clone(self)));
        lock_unpoisoned(&self.drain)
            .deferred_handles
            .push(DeferredGenerationHandles { handles, owner });
        self.drained.notify_waiters();
    }

    async fn wait_drained(self: &Arc<Self>) {
        loop {
            let notified = self.drained.notified();
            tokio::pin!(notified);
            // `notify_waiters` does not retain a permit. Register before the
            // state check so the final owner cannot disappear in the gap
            // between checking the mutex and first polling this waiter.
            notified.as_mut().enable();
            let deferred = {
                let mut drain = lock_unpoisoned(&self.drain);
                if drain.deferred_handles.is_empty() && drain.active_owners == 0 {
                    return;
                }
                std::mem::take(&mut drain.deferred_handles)
            };
            if !deferred.is_empty() {
                let mut lease = DeferredDrainLease {
                    control: Arc::clone(self),
                    batches: deferred,
                };
                while let Some(batch) = lease.batches.last_mut() {
                    while let Some(handle) = batch.handles.last_mut() {
                        let _ = handle.await;
                        batch.handles.pop();
                    }
                    lease.batches.pop();
                }
                continue;
            }
            notified.as_mut().await;
        }
    }
}

struct DeferredDrainLease {
    control: Arc<GenerationControl>,
    batches: Vec<DeferredGenerationHandles>,
}

impl Drop for DeferredDrainLease {
    fn drop(&mut self) {
        if self.batches.is_empty() {
            return;
        }
        lock_unpoisoned(&self.control.drain)
            .deferred_handles
            .append(&mut self.batches);
        self.control.drained.notify_waiters();
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

struct GenerationActivityGuard {
    control: Arc<GenerationControl>,
}

impl Drop for GenerationActivityGuard {
    fn drop(&mut self) {
        self.control.owner_finished();
    }
}

fn claim_generation_activity(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
) -> AppResult<GenerationActivityGuard> {
    let control = {
        let lifecycle = lock_unpoisoned(lifecycle);
        if !matches!(
            lifecycle.state,
            SchedulerLifecycleState::Starting { .. } | SchedulerLifecycleState::Running { .. }
        ) {
            return Err(AppError::Internal("调度器未运行".into()));
        }
        let control = lifecycle
            .control
            .as_ref()
            .cloned()
            .ok_or_else(|| AppError::Internal("调度器未运行".into()))?;
        control.owner_started();
        control
    };
    Ok(GenerationActivityGuard { control })
}

async fn run_generation_activity<F, Fut>(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    operation: F,
) -> AppResult<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let activity = claim_generation_activity(lifecycle)?;
    run_claimed_generation_activity(activity, operation).await
}

async fn run_claimed_generation_activity<F, Fut>(
    activity: GenerationActivityGuard,
    operation: F,
) -> AppResult<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let cancellation = wait_for_generation_cancel(activity.control.subscribe());
    tokio::pin!(cancellation);
    let result = tokio::select! {
        biased;
        _ = &mut cancellation => cancelled_task_result(),
        result = operation() => result,
    };
    drop(activity);
    result
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

#[derive(Debug, Default)]
struct SchedulerHandleBatch {
    handles: Vec<(TaskId, SchedulerHandle)>,
    control: Option<Arc<GenerationControl>>,
}

impl SchedulerHandleBatch {
    fn attached(control: Arc<GenerationControl>) -> Self {
        Self {
            handles: Vec::new(),
            control: Some(control),
        }
    }

    fn attach(&mut self, control: Arc<GenerationControl>) {
        self.control = Some(control);
    }

    fn push(&mut self, task: TaskId, handle: SchedulerHandle) {
        self.handles.push((task, handle));
    }

    fn task_ids(&self) -> Vec<TaskId> {
        self.handles.iter().map(|(task, _)| *task).collect()
    }

    fn pop(&mut self) -> Option<(TaskId, SchedulerHandle)> {
        self.handles.pop()
    }
}

impl Drop for SchedulerHandleBatch {
    fn drop(&mut self) {
        let handles = std::mem::take(&mut self.handles)
            .into_iter()
            .map(|(_, handle)| handle)
            .collect::<Vec<_>>();
        if handles.is_empty() {
            return;
        }
        if let Some(control) = &self.control {
            defer_generation_handles(Arc::clone(control), handles);
        } else {
            for handle in handles {
                handle.abort();
            }
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

#[derive(Debug)]
struct DesiredRunToken {
    cancelled: AtomicBool,
    cancellation: watch::Sender<bool>,
}

impl DesiredRunToken {
    fn new() -> Arc<Self> {
        let (cancellation, _) = watch::channel(false);
        Arc::new(Self {
            cancelled: AtomicBool::new(false),
            cancellation,
        })
    }

    fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::SeqCst) {
            self.cancellation.send_replace(true);
        }
    }

    fn subscribe(&self) -> watch::Receiver<bool> {
        self.cancellation.subscribe()
    }

    fn is_active(&self) -> bool {
        !self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
struct RecoveryTicket {
    failed_generation: u64,
    attempt: u32,
    desired: Arc<DesiredRunToken>,
}

type RecoveryFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>;
type RecoveryHandler =
    Arc<dyn Fn(RecoveryTicket, watch::Receiver<bool>) -> RecoveryFuture + Send + Sync>;

#[derive(Debug)]
struct RecoveryDrain {
    active: watch::Sender<usize>,
}

impl RecoveryDrain {
    fn new() -> Arc<Self> {
        let (active, _) = watch::channel(0);
        Arc::new(Self { active })
    }

    fn owner_started(&self) {
        self.active.send_modify(|active| {
            *active = active
                .checked_add(1)
                .expect("scheduler recovery owner count exhausted");
        });
    }

    fn owner_finished(&self) {
        self.active
            .send_modify(|active| match active.checked_sub(1) {
                Some(next) => *active = next,
                None => tracing::error!("scheduler recovery owner count underflow"),
            });
    }

    async fn wait_drained(&self) {
        let mut active = self.active.subscribe();
        let _ = active.wait_for(|active| *active == 0).await;
    }

    #[cfg(test)]
    fn active_count(&self) -> usize {
        *self.active.borrow()
    }
}

struct RecoveryOwnerGuard {
    drain: Arc<RecoveryDrain>,
}

impl RecoveryOwnerGuard {
    fn new(drain: Arc<RecoveryDrain>) -> Self {
        drain.owner_started();
        Self { drain }
    }
}

impl Drop for RecoveryOwnerGuard {
    fn drop(&mut self) {
        self.drain.owner_finished();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequiredLoopStatus {
    Booting,
    Ready,
    CommitApproved,
}

enum RequiredCommitOutcome {
    Approved,
    Published,
    Rejected,
}

enum SchedulerStartDecision {
    Begin {
        generation: u64,
        run_states: Arc<HashMap<TaskId, TaskRunStateRef>>,
        generation_running: Arc<AtomicBool>,
        control: Arc<GenerationControl>,
    },
    AlreadyRunning,
    Wait(watch::Receiver<bool>),
    Exhausted,
}

enum ExplicitStartRequest {
    AlreadyRunning,
    Desired(Arc<DesiredRunToken>),
    Rejected,
}

struct SchedulerLifecycle {
    state: SchedulerLifecycleState,
    next_generation: u64,
    generation_exhausted: bool,
    handles: HashMap<TaskId, SchedulerHandle>,
    run_states: Option<Arc<HashMap<TaskId, TaskRunStateRef>>>,
    generation_running: Option<Arc<AtomicBool>>,
    control: Option<Arc<GenerationControl>>,
    cleanup_completion: Option<watch::Receiver<bool>>,
    required_loops: HashMap<TaskId, RequiredLoopStatus>,
    desired: Option<Arc<DesiredRunToken>>,
    recovery_attempt: u32,
    pending_recovery: Option<RecoveryTicket>,
    recovery_handler: Option<RecoveryHandler>,
    recovery_worker: Option<SchedulerHandle>,
    recovery_drain: Arc<RecoveryDrain>,
    cleanup_job: Option<Arc<CleanupJobSlot>>,
    cleanup_changed: Arc<Notify>,
    terminal: bool,
}

impl SchedulerLifecycle {
    fn new() -> Self {
        Self {
            state: SchedulerLifecycleState::Stopped,
            next_generation: 0,
            generation_exhausted: false,
            handles: HashMap::new(),
            run_states: None,
            generation_running: None,
            control: None,
            cleanup_completion: None,
            required_loops: HashMap::new(),
            desired: None,
            recovery_attempt: 0,
            pending_recovery: None,
            recovery_handler: None,
            recovery_worker: None,
            recovery_drain: RecoveryDrain::new(),
            cleanup_job: None,
            cleanup_changed: Arc::new(Notify::new()),
            terminal: false,
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
        if self.state != SchedulerLifecycleState::Stopped
            || self.generation_exhausted
            || self.terminal
        {
            return None;
        }
        let Some(next_generation) = self.next_generation.checked_add(1) else {
            self.fail_generation_exhaustion();
            return None;
        };
        self.next_generation = next_generation;
        let generation = self.next_generation;
        let control = GenerationControl::new();
        let run_states = new_task_run_states(&control);
        let generation_running = Arc::new(AtomicBool::new(true));
        self.state = SchedulerLifecycleState::Starting { generation };
        self.run_states = Some(Arc::clone(&run_states));
        self.generation_running = Some(Arc::clone(&generation_running));
        self.control = Some(Arc::clone(&control));
        self.required_loops.clear();
        Some((generation, run_states, generation_running, control))
    }

    fn start_decision(&mut self) -> SchedulerStartDecision {
        let desired = match self.request_explicit_start() {
            ExplicitStartRequest::AlreadyRunning => {
                return SchedulerStartDecision::AlreadyRunning;
            }
            ExplicitStartRequest::Desired(desired) => desired,
            ExplicitStartRequest::Rejected => return SchedulerStartDecision::Exhausted,
        };
        self.start_decision_for(&desired)
    }

    fn request_explicit_start(&mut self) -> ExplicitStartRequest {
        if self.terminal || self.generation_exhausted {
            return ExplicitStartRequest::Rejected;
        }
        match self.state {
            SchedulerLifecycleState::Starting { .. } | SchedulerLifecycleState::Running { .. } => {
                return ExplicitStartRequest::AlreadyRunning;
            }
            SchedulerLifecycleState::Stopping { .. } | SchedulerLifecycleState::Stopped => {}
        }
        if let Some(previous) = self.desired.replace(DesiredRunToken::new()) {
            previous.cancel();
        }
        self.pending_recovery = None;
        self.recovery_attempt = 0;
        ExplicitStartRequest::Desired(
            self.desired
                .as_ref()
                .expect("explicit start installed a desired token")
                .clone(),
        )
    }

    fn start_decision_for(&mut self, desired: &Arc<DesiredRunToken>) -> SchedulerStartDecision {
        if self.terminal
            || self.generation_exhausted
            || self
                .desired
                .as_ref()
                .is_none_or(|active| !Arc::ptr_eq(active, desired) || !active.is_active())
        {
            return SchedulerStartDecision::Exhausted;
        }
        match self.state {
            SchedulerLifecycleState::Starting { .. } | SchedulerLifecycleState::Running { .. } => {
                return SchedulerStartDecision::AlreadyRunning;
            }
            SchedulerLifecycleState::Stopping { .. } => {
                return self
                    .cleanup_completion
                    .as_ref()
                    .cloned()
                    .map(SchedulerStartDecision::Wait)
                    .unwrap_or(SchedulerStartDecision::Exhausted);
            }
            SchedulerLifecycleState::Stopped => {}
        }
        let Some((generation, run_states, generation_running, control)) = self.begin_start() else {
            if self.generation_exhausted {
                self.fail_generation_exhaustion();
            }
            return SchedulerStartDecision::Exhausted;
        };
        control.owner_started();
        SchedulerStartDecision::Begin {
            generation,
            run_states,
            generation_running,
            control,
        }
    }

    fn recovery_start_decision(&mut self, ticket: &RecoveryTicket) -> SchedulerStartDecision {
        let valid = !self.terminal
            && !self.generation_exhausted
            && self.state == SchedulerLifecycleState::Stopped
            && self.pending_recovery.as_ref().is_some_and(|pending| {
                pending.failed_generation == ticket.failed_generation
                    && Arc::ptr_eq(&pending.desired, &ticket.desired)
            })
            && self.desired.as_ref().is_some_and(|desired| {
                Arc::ptr_eq(desired, &ticket.desired) && desired.is_active()
            });
        if !valid {
            return SchedulerStartDecision::Exhausted;
        }
        let Some((generation, run_states, generation_running, control)) = self.begin_start() else {
            self.fail_generation_exhaustion();
            return SchedulerStartDecision::Exhausted;
        };
        control.owner_started();
        SchedulerStartDecision::Begin {
            generation,
            run_states,
            generation_running,
            control,
        }
    }

    fn fail_generation_exhaustion(&mut self) {
        if !self.generation_exhausted {
            tracing::error!("scheduler generation space exhausted; failing closed");
        }
        self.generation_exhausted = true;
        if let Some(desired) = self.desired.take() {
            desired.cancel();
        }
        self.pending_recovery = None;
        self.recovery_attempt = 0;
    }

    fn mark_start_committed(&mut self, generation: u64) {
        if self.state != (SchedulerLifecycleState::Running { generation }) {
            return;
        }
        self.pending_recovery = None;
        self.recovery_attempt = 0;
    }

    fn register_required_loops(&mut self, generation: u64, tasks: &[TaskId]) -> bool {
        if self.state != (SchedulerLifecycleState::Starting { generation }) {
            return false;
        }
        self.required_loops = tasks
            .iter()
            .copied()
            .map(|task| (task, RequiredLoopStatus::Booting))
            .collect();
        true
    }

    fn mark_required_loop_ready(&mut self, generation: u64, task: TaskId) -> bool {
        if self.state != (SchedulerLifecycleState::Starting { generation }) {
            return false;
        }
        let Some(status) = self.required_loops.get_mut(&task) else {
            return false;
        };
        if *status != RequiredLoopStatus::Booting {
            return false;
        }
        *status = RequiredLoopStatus::Ready;
        true
    }

    fn approve_required_loop_and_maybe_publish(
        &mut self,
        generation: u64,
        task: TaskId,
    ) -> RequiredCommitOutcome {
        if self.state != (SchedulerLifecycleState::Starting { generation }) {
            return RequiredCommitOutcome::Rejected;
        }
        let Some(status) = self.required_loops.get_mut(&task) else {
            return RequiredCommitOutcome::Rejected;
        };
        if *status != RequiredLoopStatus::Ready {
            return RequiredCommitOutcome::Rejected;
        }
        *status = RequiredLoopStatus::CommitApproved;
        if self
            .required_loops
            .values()
            .any(|status| *status != RequiredLoopStatus::CommitApproved)
        {
            return RequiredCommitOutcome::Approved;
        }
        if self
            .control
            .as_ref()
            .is_none_or(|control| control.is_cancelled())
        {
            return RequiredCommitOutcome::Rejected;
        }
        self.state = SchedulerLifecycleState::Running { generation };
        RequiredCommitOutcome::Published
    }

    fn install_handles(
        &mut self,
        generation: u64,
        mut handles: SchedulerHandleBatch,
    ) -> Result<(), SchedulerHandleBatch> {
        if self.state != (SchedulerLifecycleState::Starting { generation }) {
            return Err(handles);
        }
        for (task, handle) in handles.handles.drain(..) {
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
            || self
                .required_loops
                .values()
                .any(|status| *status != RequiredLoopStatus::CommitApproved)
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
        self.begin_cleanup_with_cause(generation, CleanupCause::StartAborted)
    }

    fn begin_cleanup_with_cause(&mut self, generation: u64, cause: CleanupCause) -> CleanupRequest {
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
        let recovery = if cause == CleanupCause::ExplicitStop {
            None
        } else {
            self.reserve_recovery(generation)
        };
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
            recovery,
        })
    }

    fn reserve_recovery(
        &mut self,
        failed_generation: u64,
    ) -> Option<(RecoveryHandler, RecoveryTicket)> {
        if self.terminal || self.generation_exhausted {
            return None;
        }
        let (Some(desired), Some(handler)) = (&self.desired, &self.recovery_handler) else {
            return None;
        };
        if !desired.is_active() {
            return None;
        }
        if self.pending_recovery.as_ref().is_some_and(|pending| {
            pending.failed_generation == failed_generation && Arc::ptr_eq(&pending.desired, desired)
        }) {
            return None;
        }
        let attempt = self.recovery_attempt.saturating_add(1);
        self.recovery_attempt = attempt;
        let ticket = RecoveryTicket {
            failed_generation,
            attempt,
            desired: Arc::clone(desired),
        };
        self.pending_recovery = Some(ticket.clone());
        Some((Arc::clone(handler), ticket))
    }

    fn disable_desired_running(&mut self) {
        if let Some(desired) = self.desired.take() {
            desired.cancel();
        }
        self.pending_recovery = None;
        self.recovery_attempt = 0;
    }

    fn begin_explicit_stop(&mut self) -> CleanupRequest {
        self.disable_desired_running();
        let Some(generation) = self.active_generation() else {
            return CleanupRequest::Inactive;
        };
        self.begin_cleanup_with_cause(generation, CleanupCause::ExplicitStop)
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
        self.cleanup_job = None;
        self.required_loops.clear();
        true
    }

    fn begin_terminal_shutdown(&mut self) -> CleanupRequest {
        self.terminal = true;
        self.begin_explicit_stop()
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
    recovery: Option<(RecoveryHandler, RecoveryTicket)>,
}

enum CleanupRequest {
    Start(CleanupResources),
    Wait(watch::Receiver<bool>),
    Inactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanupCause {
    ExplicitStop,
    StartAborted,
    UnexpectedRequiredLoop,
}

struct CleanupCompletionGuard {
    lifecycle: Arc<StdMutex<SchedulerLifecycle>>,
    generation: u64,
    completion: Option<watch::Sender<bool>>,
}

struct CleanupJob {
    handles: Vec<SchedulerHandle>,
    control: Option<Arc<GenerationControl>>,
    guard: CleanupCompletionGuard,
}

#[derive(Default)]
struct CleanupJobSlotState {
    job: Option<CleanupJob>,
    worker_active: bool,
    worker_pending: usize,
    workers: Vec<SchedulerHandle>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

struct CleanupJobSlot {
    state: StdMutex<CleanupJobSlotState>,
    worker_changed: Notify,
}

impl CleanupJobSlot {
    fn new(job: CleanupJob) -> Arc<Self> {
        Arc::new(Self {
            state: StdMutex::new(CleanupJobSlotState {
                job: Some(job),
                ..CleanupJobSlotState::default()
            }),
            worker_changed: Notify::new(),
        })
    }

    fn reserve_worker_launch(self: &Arc<Self>) -> Option<CleanupLaunchPermit> {
        let mut state = lock_unpoisoned(&self.state);
        if state.worker_active || state.worker_pending != 0 || state.job.is_none() {
            return None;
        }
        state.worker_pending = 1;
        Some(CleanupLaunchPermit {
            slot: Arc::clone(self),
            pending: true,
        })
    }

    fn claim_job(self: &Arc<Self>) -> Option<CleanupJobLease> {
        let job = lock_unpoisoned(&self.state).job.take()?;
        Some(CleanupJobLease {
            slot: Arc::clone(self),
            job: Some(job),
        })
    }

    fn store_worker(&self, handle: SchedulerHandle) {
        lock_unpoisoned(&self.state).workers.push(handle);
    }

    fn store_thread(&self, handle: std::thread::JoinHandle<()>) {
        lock_unpoisoned(&self.state).threads.push(handle);
    }

    #[cfg(test)]
    fn abort_latest_worker(&self) -> bool {
        let state = lock_unpoisoned(&self.state);
        let Some(worker) = state.workers.last() else {
            return false;
        };
        worker.abort();
        true
    }
}

struct CleanupWorkerPermit {
    slot: Arc<CleanupJobSlot>,
}

struct CleanupLaunchPermit {
    slot: Arc<CleanupJobSlot>,
    pending: bool,
}

impl CleanupLaunchPermit {
    fn begin(mut self) -> Option<CleanupWorkerPermit> {
        let active = {
            let mut state = lock_unpoisoned(&self.slot.state);
            state.worker_pending = state.worker_pending.saturating_sub(1);
            self.pending = false;
            if state.worker_active || state.job.is_none() {
                false
            } else {
                state.worker_active = true;
                true
            }
        };
        self.slot.worker_changed.notify_waiters();
        active.then(|| CleanupWorkerPermit {
            slot: Arc::clone(&self.slot),
        })
    }
}

impl Drop for CleanupLaunchPermit {
    fn drop(&mut self) {
        if !self.pending {
            return;
        }
        let mut state = lock_unpoisoned(&self.slot.state);
        state.worker_pending = state.worker_pending.saturating_sub(1);
        drop(state);
        self.slot.worker_changed.notify_waiters();
    }
}

impl Drop for CleanupWorkerPermit {
    fn drop(&mut self) {
        lock_unpoisoned(&self.slot.state).worker_active = false;
        self.slot.worker_changed.notify_waiters();
    }
}

struct CleanupJobLease {
    slot: Arc<CleanupJobSlot>,
    job: Option<CleanupJob>,
}

impl CleanupJobLease {
    async fn drain(&mut self) {
        let job = self.job.as_mut().expect("cleanup lease owns a job");
        for handle in &job.handles {
            handle.abort();
        }
        while let Some(handle) = job.handles.last_mut() {
            let _ = handle.await;
            job.handles.pop();
        }
        if let Some(control) = &job.control {
            control.wait_drained().await;
        }
        job.control = None;
    }

    fn complete(mut self) {
        let mut job = self.job.take().expect("cleanup lease owns a job");
        job.guard.complete(&self.slot);
    }
}

impl Drop for CleanupJobLease {
    fn drop(&mut self) {
        let Some(job) = self.job.take() else {
            return;
        };
        let mut state = lock_unpoisoned(&self.slot.state);
        if state.job.is_none() {
            state.job = Some(job);
        } else {
            tracing::error!("scheduler cleanup job was returned twice");
        }
    }
}

impl CleanupCompletionGuard {
    fn complete(&mut self, slot: &Arc<CleanupJobSlot>) {
        let finished = {
            let mut lifecycle = lock_unpoisoned(&self.lifecycle);
            let owned = lifecycle
                .cleanup_job
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, slot));
            owned && lifecycle.finish_cleanup(self.generation)
        };
        if finished {
            if let Some(completion) = self.completion.take() {
                completion.send_replace(true);
            }
        }
    }
}

async fn run_cleanup_job(
    slot: Arc<CleanupJobSlot>,
    launch: CleanupLaunchPermit,
    installed: oneshot::Receiver<()>,
) {
    if installed.await.is_err() {
        return;
    }
    let Some(_permit) = launch.begin() else {
        return;
    };
    let Some(mut job) = slot.claim_job() else {
        return;
    };
    job.drain().await;
    job.complete();
}

fn launch_cleanup_job(slot: Arc<CleanupJobSlot>) {
    launch_cleanup_job_inner(slot, true);
}

fn attempt_cleanup_spawn<F>(slot: &Arc<CleanupJobSlot>, spawn: F) -> bool
where
    F: FnOnce(
        std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>,
    ) -> SchedulerHandle,
{
    let Some(launch) = slot.reserve_worker_launch() else {
        return true;
    };
    let (installed, installed_receiver) = oneshot::channel();
    let worker_slot = Arc::clone(slot);
    let future = Box::pin(run_cleanup_job(worker_slot, launch, installed_receiver));
    let launched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| spawn(future)));
    match launched {
        Ok(handle) => {
            slot.store_worker(handle);
            let _ = installed.send(());
            true
        }
        Err(_) => false,
    }
}

fn spawn_cleanup_on_runtime(slot: &Arc<CleanupJobSlot>, runtime: &tokio::runtime::Handle) -> bool {
    attempt_cleanup_spawn(slot, |future| {
        tauri::async_runtime::JoinHandle::Tokio(runtime.spawn(future))
    })
}

fn spawn_cleanup_on_tauri_runtime(slot: &Arc<CleanupJobSlot>) -> bool {
    attempt_cleanup_spawn(slot, tauri::async_runtime::spawn)
}

fn launch_cleanup_job_inner(slot: Arc<CleanupJobSlot>, try_primary: bool) {
    let current = tokio::runtime::Handle::try_current().ok();
    if try_primary
        && current
            .as_ref()
            .is_some_and(|runtime| spawn_cleanup_on_runtime(&slot, runtime))
    {
        return;
    }
    if current
        .as_ref()
        .is_some_and(|runtime| spawn_cleanup_on_runtime(&slot, runtime))
    {
        return;
    }
    if spawn_cleanup_on_tauri_runtime(&slot) {
        return;
    }

    let Some(launch) = slot.reserve_worker_launch() else {
        return;
    };
    let (installed, installed_receiver) = oneshot::channel();
    let emergency = Arc::clone(&slot);
    match std::thread::Builder::new()
        .name("scheduler-cleanup".into())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                drop(launch);
                return;
            };
            runtime.block_on(run_cleanup_job(emergency, launch, installed_receiver));
        }) {
        Ok(thread) => {
            slot.store_thread(thread);
            let _ = installed.send(());
        }
        Err(error) => {
            tracing::error!(%error, "unable to launch scheduler cleanup fallback");
        }
    }
}

#[cfg(test)]
async fn wait_for_cleanup(mut completion: watch::Receiver<bool>) {
    let _ = completion.wait_for(|finished| *finished).await;
}

async fn wait_for_owned_cleanup(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    mut completion: watch::Receiver<bool>,
) {
    loop {
        if *completion.borrow() {
            return;
        }
        let cleanup_changed = { Arc::clone(&lock_unpoisoned(lifecycle).cleanup_changed) };
        let changed = cleanup_changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        let (slot, stopping) = {
            let lifecycle = lock_unpoisoned(lifecycle);
            (
                lifecycle.cleanup_job.clone(),
                matches!(lifecycle.state, SchedulerLifecycleState::Stopping { .. }),
            )
        };
        match slot {
            Some(slot) => {
                let worker_changed = slot.worker_changed.notified();
                tokio::pin!(worker_changed);
                // A failed/cancelled worker can return its lease immediately.
                // Register before launch so its edge cannot be lost before the
                // select first polls this waiter.
                worker_changed.as_mut().enable();
                launch_cleanup_job(Arc::clone(&slot));
                tokio::select! {
                    changed = completion.changed() => {
                        if changed.is_err() || *completion.borrow() {
                            return;
                        }
                    }
                    _ = worker_changed.as_mut() => {}
                }
            }
            None if stopping => {
                // begin_cleanup publishes Stopping before the durable job slot
                // is installed. Treat that short install window as owned
                // state, not as permission to degrade to a passive waiter.
                tokio::select! {
                    changed = completion.changed() => {
                        if changed.is_err() || *completion.borrow() {
                            return;
                        }
                    }
                    _ = changed.as_mut() => {}
                }
            }
            None => {
                let _ = completion.wait_for(|finished| *finished).await;
                return;
            }
        }
    }
}

struct RecoveryTaskGuard {
    lifecycle: Arc<StdMutex<SchedulerLifecycle>>,
    ticket: Option<RecoveryTicket>,
    armed: Arc<AtomicBool>,
}

impl RecoveryTaskGuard {
    fn finish(&mut self) {
        if self.armed.load(Ordering::SeqCst) {
            if let Some(ticket) = self.ticket.take() {
                reconcile_abandoned_recovery(&self.lifecycle, &ticket);
            }
        } else {
            self.ticket = None;
        }
    }
}

impl Drop for RecoveryTaskGuard {
    fn drop(&mut self) {
        self.finish();
    }
}

fn completed_cleanup_receiver() -> watch::Receiver<bool> {
    let (_, receiver) = watch::channel(true);
    receiver
}

fn reconcile_abandoned_recovery(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    abandoned: &RecoveryTicket,
) {
    let launch = {
        let mut state = lock_unpoisoned(lifecycle);
        let valid = !state.terminal
            && !state.generation_exhausted
            && state.pending_recovery.as_ref().is_some_and(|pending| {
                pending.failed_generation == abandoned.failed_generation
                    && pending.attempt == abandoned.attempt
                    && Arc::ptr_eq(&pending.desired, &abandoned.desired)
            })
            && state.desired.as_ref().is_some_and(|desired| {
                desired.is_active() && Arc::ptr_eq(desired, &abandoned.desired)
            });
        if !valid {
            None
        } else {
            let attempt = state.recovery_attempt.saturating_add(1);
            state.recovery_attempt = attempt;
            let ticket = RecoveryTicket {
                failed_generation: abandoned.failed_generation,
                attempt,
                desired: Arc::clone(&abandoned.desired),
            };
            state.pending_recovery = Some(ticket.clone());
            state.recovery_handler.as_ref().map(|handler| {
                let completion = state
                    .cleanup_completion
                    .clone()
                    .unwrap_or_else(completed_cleanup_receiver);
                (Arc::clone(handler), ticket, completion)
            })
        }
    };
    if let Some((handler, ticket, completion)) = launch {
        launch_recovery_handler(lifecycle, handler, ticket, completion);
    }
}

fn launch_recovery_handler(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    handler: RecoveryHandler,
    ticket: RecoveryTicket,
    completion: watch::Receiver<bool>,
) {
    let future = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handler(ticket.clone(), completion)
    }));
    let Ok(future) = future else {
        reconcile_abandoned_recovery(lifecycle, &ticket);
        return;
    };
    let armed = Arc::new(AtomicBool::new(false));
    let guard = RecoveryTaskGuard {
        lifecycle: Arc::clone(lifecycle),
        ticket: Some(ticket.clone()),
        armed: Arc::clone(&armed),
    };
    let (installed, installed_receiver) = oneshot::channel();
    let mut state = lock_unpoisoned(lifecycle);
    let valid = state.pending_recovery.as_ref().is_some_and(|pending| {
        pending.failed_generation == ticket.failed_generation
            && pending.attempt == ticket.attempt
            && Arc::ptr_eq(&pending.desired, &ticket.desired)
    }) && state
        .desired
        .as_ref()
        .is_some_and(|desired| desired.is_active() && Arc::ptr_eq(desired, &ticket.desired));
    if !valid {
        return;
    }
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        tracing::error!("scheduler recovery has no active Tokio runtime; failing closed");
        state.terminal = true;
        state.disable_desired_running();
        return;
    };
    let owner = RecoveryOwnerGuard::new(Arc::clone(&state.recovery_drain));
    let worker = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        SchedulerHandle::Tokio(runtime.spawn(async move {
            let _owner = owner;
            let mut guard = guard;
            if installed_receiver.await.is_err() {
                return;
            }
            future.await;
            guard.finish();
        }))
    }));
    let Ok(worker) = worker else {
        drop(state);
        reconcile_abandoned_recovery(lifecycle, &ticket);
        return;
    };
    state.recovery_worker = Some(worker);
    armed.store(true, Ordering::SeqCst);
    if installed.send(()).is_err() {
        drop(state);
        reconcile_abandoned_recovery(lifecycle, &ticket);
    }
}

#[cfg(test)]
fn request_scheduler_cleanup(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    _running: &Arc<AtomicBool>,
    generation: u64,
) -> Option<watch::Receiver<bool>> {
    request_scheduler_cleanup_with_cause(lifecycle, generation, CleanupCause::StartAborted)
}

fn request_scheduler_cleanup_with_cause(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    generation: u64,
    cause: CleanupCause,
) -> Option<watch::Receiver<bool>> {
    let request = lock_unpoisoned(lifecycle).begin_cleanup_with_cause(generation, cause);
    launch_scheduler_cleanup(lifecycle, request)
}

fn launch_scheduler_cleanup(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    request: CleanupRequest,
) -> Option<watch::Receiver<bool>> {
    match request {
        CleanupRequest::Wait(receiver) => {
            if let Some(slot) = lock_unpoisoned(lifecycle).cleanup_job.clone() {
                launch_cleanup_job(slot);
            }
            Some(receiver)
        }
        CleanupRequest::Inactive => None,
        CleanupRequest::Start(mut resources) => {
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
            let guard = CleanupCompletionGuard {
                lifecycle: Arc::clone(&lifecycle),
                generation: resources.generation,
                completion: Some(resources.completion),
            };
            let recovery = resources.recovery.take();
            let slot = CleanupJobSlot::new(CleanupJob {
                handles: std::mem::take(&mut resources.handles),
                control: resources.control.take(),
                guard,
            });
            {
                let mut state = lock_unpoisoned(&lifecycle);
                if state.state
                    == (SchedulerLifecycleState::Stopping {
                        generation: resources.generation,
                    })
                    && state.cleanup_job.is_none()
                {
                    state.cleanup_job = Some(Arc::clone(&slot));
                    state.cleanup_changed.notify_waiters();
                }
            }
            launch_cleanup_job(slot);
            if let Some((handler, ticket)) = recovery {
                launch_recovery_handler(&lifecycle, handler, ticket, receiver.clone());
            }
            Some(receiver)
        }
    }
}

#[derive(Debug)]
struct GenerationOwnerGuard {
    control: Arc<GenerationControl>,
}

#[derive(Debug)]
struct DeferredGenerationHandles {
    handles: Vec<SchedulerHandle>,
    owner: GenerationOwnerGuard,
}

impl GenerationOwnerGuard {
    fn new(control: Arc<GenerationControl>) -> Self {
        control.owner_started();
        Self { control }
    }
}

impl Drop for GenerationOwnerGuard {
    fn drop(&mut self) {
        self.control.owner_finished();
    }
}

fn defer_generation_handles(control: Arc<GenerationControl>, handles: Vec<SchedulerHandle>) {
    control.defer_handles(handles, None);
}

struct GenerationOwnedSchedulerHandle {
    handle: Option<SchedulerHandle>,
    owner: Option<GenerationOwnerGuard>,
}

impl GenerationOwnedSchedulerHandle {
    fn new(control: Arc<GenerationControl>, handle: SchedulerHandle) -> Self {
        Self {
            handle: Some(handle),
            owner: Some(GenerationOwnerGuard::new(control)),
        }
    }

    async fn completed_now(&mut self) -> bool {
        let Some(handle) = self.handle.as_mut() else {
            return true;
        };
        let completed = std::future::poll_fn(|context| {
            let completed =
                match std::future::Future::poll(std::pin::Pin::new(&mut *handle), context) {
                    std::task::Poll::Ready(_) => true,
                    std::task::Poll::Pending => false,
                };
            std::task::Poll::Ready(completed)
        })
        .await;
        if completed {
            self.handle.take();
        }
        completed
    }

    async fn abort_and_join(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
            let _ = handle.await;
        }
    }
}

impl Drop for GenerationOwnedSchedulerHandle {
    fn drop(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        let Some(owner) = self.owner.take() else {
            handle.abort();
            return;
        };
        let control = Arc::clone(&owner.control);
        control.defer_handles(vec![handle], Some(owner));
    }
}

enum SupervisorEvent {
    Cancelled,
    Commit(bool),
    Exited,
}

fn required_loop_failed(
    task: TaskId,
    generation: u64,
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
) {
    tracing::warn!(
        ?task,
        generation,
        "scheduler periodic loop exited unexpectedly"
    );
    let _ = request_scheduler_cleanup_with_cause(
        lifecycle,
        generation,
        CleanupCause::UnexpectedRequiredLoop,
    );
}

/// Lives inside the required-loop future itself, rather than in its supervisor.
///
/// Its destructor and the final lifecycle publication take the same lifecycle
/// mutex. Therefore an exit that wins the mutex first prevents `Running` from
/// being published; an exit that loses is ordered after publication and
/// atomically starts teardown/recovery before the loop future can finish.
struct RequiredLoopRuntimeGuard {
    task: TaskId,
    generation: u64,
    control: Arc<GenerationControl>,
    lifecycle: Arc<StdMutex<SchedulerLifecycle>>,
}

impl Drop for RequiredLoopRuntimeGuard {
    fn drop(&mut self) {
        if !self.control.is_cancelled() {
            required_loop_failed(self.task, self.generation, &self.lifecycle);
        }
    }
}

fn spawn_required_loop_future<F>(
    task: TaskId,
    generation: u64,
    control: Arc<GenerationControl>,
    lifecycle: Arc<StdMutex<SchedulerLifecycle>>,
    future: F,
) -> SchedulerHandle
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let owner = GenerationOwnerGuard::new(Arc::clone(&control));
    tauri::async_runtime::JoinHandle::Tokio(tokio::spawn(async move {
        let _owner = owner;
        let _runtime = RequiredLoopRuntimeGuard {
            task,
            generation,
            control,
            lifecycle,
        };
        future.await;
    }))
}

async fn supervise_scheduler_handle(
    task: TaskId,
    generation: u64,
    mut owned: GenerationOwnedSchedulerHandle,
    control: Arc<GenerationControl>,
    lifecycle: Arc<StdMutex<SchedulerLifecycle>>,
    ready: oneshot::Sender<bool>,
    commit: oneshot::Receiver<()>,
    commit_outcome: watch::Sender<Option<bool>>,
) {
    let completed_before_start = owned.completed_now().await;
    let alive = !completed_before_start && !control.is_cancelled();
    let ready_in_lifecycle =
        alive && lock_unpoisoned(&lifecycle).mark_required_loop_ready(generation, task);
    let _ = ready.send(ready_in_lifecycle);
    if !alive {
        if completed_before_start && !control.is_cancelled() {
            required_loop_failed(task, generation, &lifecycle);
        }
        return;
    }
    if !ready_in_lifecycle {
        owned.abort_and_join().await;
        return;
    }

    let cancellation = wait_for_generation_cancel(control.subscribe());
    tokio::pin!(cancellation);
    tokio::pin!(commit);
    let event = tokio::select! {
        biased;
        _ = owned.handle.as_mut().expect("ready handle remains owned") => SupervisorEvent::Exited,
        _ = &mut cancellation => SupervisorEvent::Cancelled,
        commit = &mut commit => SupervisorEvent::Commit(commit.is_ok()),
    };
    match event {
        SupervisorEvent::Cancelled => {
            owned.abort_and_join().await;
            return;
        }
        SupervisorEvent::Exited => {
            owned.handle.take();
            if !control.is_cancelled() {
                required_loop_failed(task, generation, &lifecycle);
            }
            return;
        }
        SupervisorEvent::Commit(false) => {
            if !control.is_cancelled() {
                required_loop_failed(task, generation, &lifecycle);
            }
            owned.abort_and_join().await;
            return;
        }
        SupervisorEvent::Commit(true) => {
            let outcome = lock_unpoisoned(&lifecycle)
                .approve_required_loop_and_maybe_publish(generation, task);
            match outcome {
                RequiredCommitOutcome::Approved => {}
                RequiredCommitOutcome::Published => {
                    commit_outcome.send_replace(Some(true));
                }
                RequiredCommitOutcome::Rejected => {
                    commit_outcome.send_replace(Some(false));
                    owned.abort_and_join().await;
                    return;
                }
            }
        }
    }

    let cancellation = wait_for_generation_cancel(control.subscribe());
    tokio::pin!(cancellation);
    let unexpected = tokio::select! {
        biased;
        _ = owned.handle.as_mut().expect("committed handle remains owned") => true,
        _ = &mut cancellation => false,
    };
    if unexpected {
        owned.handle.take();
        if !control.is_cancelled() {
            required_loop_failed(task, generation, &lifecycle);
        }
    } else {
        owned.abort_and_join().await;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandleWrapStage {
    BeforePop,
    Owned,
    Spawned,
}

type SupervisorStartup = (oneshot::Receiver<bool>, oneshot::Sender<()>);

fn supervise_scheduler_batch<F>(
    generation: u64,
    control: &Arc<GenerationControl>,
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    mut handles: SchedulerHandleBatch,
    mut stage_hook: F,
) -> (
    SchedulerHandleBatch,
    Vec<SupervisorStartup>,
    watch::Receiver<Option<bool>>,
)
where
    F: FnMut(HandleWrapStage, usize),
{
    let mut supervised = SchedulerHandleBatch::attached(Arc::clone(control));
    supervised.handles.reserve(handles.handles.len());
    let mut supervisors = Vec::with_capacity(handles.handles.len());
    let (commit_outcome, commit_result) = watch::channel(None);
    let mut index = 0;
    while !handles.handles.is_empty() {
        stage_hook(HandleWrapStage::BeforePop, index);
        let (task, handle) = handles.pop().expect("non-empty batch has a handle");
        let owned = GenerationOwnedSchedulerHandle::new(Arc::clone(control), handle);
        stage_hook(HandleWrapStage::Owned, index);
        let (ready, ready_receiver) = oneshot::channel();
        let (commit, commit_receiver) = oneshot::channel();
        let supervisor = tokio::spawn(supervise_scheduler_handle(
            task,
            generation,
            owned,
            Arc::clone(control),
            Arc::clone(lifecycle),
            ready,
            commit_receiver,
            commit_outcome.clone(),
        ));
        supervised.push(task, tauri::async_runtime::JoinHandle::Tokio(supervisor));
        stage_hook(HandleWrapStage::Spawned, index);
        supervisors.push((ready_receiver, commit));
        index += 1;
    }
    (supervised, supervisors, commit_result)
}

struct SchedulerStartGuard {
    lifecycle: Arc<StdMutex<SchedulerLifecycle>>,
    control: Arc<GenerationControl>,
    generation: u64,
    committed: bool,
    activity_released: bool,
}

impl SchedulerStartGuard {
    fn commit(&mut self) {
        lock_unpoisoned(&self.lifecycle).mark_start_committed(self.generation);
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
        let _ = request_scheduler_cleanup_with_cause(
            &self.lifecycle,
            self.generation,
            CleanupCause::StartAborted,
        );
        self.release_activity();
    }
}

#[derive(Clone)]
enum SchedulerStartOrigin {
    Explicit,
    Recovery(RecoveryTicket),
}

enum SchedulerStartOwner {
    Explicit(Arc<DesiredRunToken>),
    Recovery(RecoveryTicket),
}

enum SchedulerStartClaimPhase {
    Begin {
        generation: u64,
        run_states: Arc<HashMap<TaskId, TaskRunStateRef>>,
        generation_running: Arc<AtomicBool>,
        control: Arc<GenerationControl>,
        guard: SchedulerStartGuard,
    },
    Wait(watch::Receiver<bool>),
    Inactive,
}

struct SchedulerStartClaim {
    lifecycle: Arc<StdMutex<SchedulerLifecycle>>,
    owner: Option<SchedulerStartOwner>,
    phase: SchedulerStartClaimPhase,
}

impl SchedulerStartClaim {
    fn apply_decision(&mut self, decision: SchedulerStartDecision) {
        self.phase = match decision {
            SchedulerStartDecision::Begin {
                generation,
                run_states,
                generation_running,
                control,
            } => {
                let guard = SchedulerStartGuard {
                    lifecycle: Arc::clone(&self.lifecycle),
                    control: Arc::clone(&control),
                    generation,
                    committed: false,
                    activity_released: false,
                };
                SchedulerStartClaimPhase::Begin {
                    generation,
                    run_states,
                    generation_running,
                    control,
                    guard,
                }
            }
            SchedulerStartDecision::Wait(waiting) => SchedulerStartClaimPhase::Wait(waiting),
            SchedulerStartDecision::AlreadyRunning | SchedulerStartDecision::Exhausted => {
                SchedulerStartClaimPhase::Inactive
            }
        };
    }

    fn refresh(&mut self) {
        let decision = {
            let mut lifecycle = lock_unpoisoned(&self.lifecycle);
            match self.owner.as_ref() {
                Some(SchedulerStartOwner::Explicit(desired)) => {
                    lifecycle.start_decision_for(desired)
                }
                Some(SchedulerStartOwner::Recovery(ticket)) => {
                    lifecycle.recovery_start_decision(ticket)
                }
                None => SchedulerStartDecision::Exhausted,
            }
        };
        self.apply_decision(decision);
    }
}

impl Drop for SchedulerStartClaim {
    fn drop(&mut self) {
        if !matches!(self.phase, SchedulerStartClaimPhase::Wait(_)) {
            return;
        }
        let Some(SchedulerStartOwner::Explicit(desired)) = self.owner.as_ref() else {
            return;
        };
        let launch = {
            let mut lifecycle = lock_unpoisoned(&self.lifecycle);
            let current = lifecycle
                .desired
                .as_ref()
                .is_some_and(|active| active.is_active() && Arc::ptr_eq(active, desired));
            if !current {
                None
            } else {
                let failed_generation = lifecycle
                    .active_generation()
                    .unwrap_or(lifecycle.next_generation);
                lifecycle
                    .reserve_recovery(failed_generation)
                    .map(|(handler, ticket)| {
                        let completion = lifecycle
                            .cleanup_completion
                            .clone()
                            .unwrap_or_else(completed_cleanup_receiver);
                        (handler, ticket, completion)
                    })
            }
        };
        if let Some((handler, ticket, completion)) = launch {
            launch_recovery_handler(&self.lifecycle, handler, ticket, completion);
        }
    }
}

fn claim_scheduler_start(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    origin: SchedulerStartOrigin,
) -> SchedulerStartClaim {
    let mut lifecycle_state = lock_unpoisoned(lifecycle);
    let owner = match origin {
        SchedulerStartOrigin::Explicit => match lifecycle_state.request_explicit_start() {
            ExplicitStartRequest::Desired(desired) => Some(SchedulerStartOwner::Explicit(desired)),
            ExplicitStartRequest::AlreadyRunning | ExplicitStartRequest::Rejected => None,
        },
        SchedulerStartOrigin::Recovery(ticket) => Some(SchedulerStartOwner::Recovery(ticket)),
    };
    let decision = match owner.as_ref() {
        Some(SchedulerStartOwner::Explicit(desired)) => lifecycle_state.start_decision_for(desired),
        Some(SchedulerStartOwner::Recovery(ticket)) => {
            lifecycle_state.recovery_start_decision(ticket)
        }
        None => SchedulerStartDecision::Exhausted,
    };
    drop(lifecycle_state);

    let mut claim = SchedulerStartClaim {
        lifecycle: Arc::clone(lifecycle),
        owner,
        phase: SchedulerStartClaimPhase::Inactive,
    };
    claim.apply_decision(decision);
    claim
}

#[cfg(test)]
async fn start_scheduler_lifecycle<Prepare, PrepareFut, Prepared, Install, Handles, FinishFut>(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    _running: &Arc<AtomicBool>,
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
        Arc<GenerationControl>,
        Prepared,
    ) -> (Handles, FinishFut),
    Handles: IntoSchedulerHandleBatch,
    FinishFut: std::future::Future<Output = ()>,
{
    start_scheduler_lifecycle_with_origin(
        lifecycle,
        SchedulerStartOrigin::Explicit,
        None,
        prepare,
        install,
    )
    .await
}

#[cfg(test)]
async fn start_scheduler_lifecycle_for_recovery<
    Prepare,
    PrepareFut,
    Prepared,
    Install,
    Handles,
    FinishFut,
>(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    _running: &Arc<AtomicBool>,
    ticket: RecoveryTicket,
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
        Arc<GenerationControl>,
        Prepared,
    ) -> (Handles, FinishFut),
    Handles: IntoSchedulerHandleBatch,
    FinishFut: std::future::Future<Output = ()>,
{
    start_scheduler_lifecycle_with_origin(
        lifecycle,
        SchedulerStartOrigin::Recovery(ticket),
        None,
        prepare,
        install,
    )
    .await
}

fn start_scheduler_lifecycle_with_origin<
    Prepare,
    PrepareFut,
    Prepared,
    Install,
    Handles,
    FinishFut,
>(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    origin: SchedulerStartOrigin,
    required_tasks: Option<&[TaskId]>,
    prepare: Prepare,
    install: Install,
) -> impl std::future::Future<Output = bool>
where
    Prepare: FnOnce(u64) -> PrepareFut,
    PrepareFut: std::future::Future<Output = Prepared>,
    Install: FnOnce(
        u64,
        Arc<HashMap<TaskId, TaskRunStateRef>>,
        Arc<AtomicBool>,
        Arc<GenerationControl>,
        Prepared,
    ) -> (Handles, FinishFut),
    Handles: IntoSchedulerHandleBatch,
    FinishFut: std::future::Future<Output = ()>,
{
    let claim = claim_scheduler_start(lifecycle, origin);
    let required_tasks = required_tasks.map(<[TaskId]>::to_vec);
    async move { drive_claimed_scheduler_start(claim, required_tasks, prepare, install).await }
}

async fn drive_claimed_scheduler_start<Prepare, PrepareFut, Prepared, Install, Handles, FinishFut>(
    mut claim: SchedulerStartClaim,
    required_tasks: Option<Vec<TaskId>>,
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
        Arc<GenerationControl>,
        Prepared,
    ) -> (Handles, FinishFut),
    Handles: IntoSchedulerHandleBatch,
    FinishFut: std::future::Future<Output = ()>,
{
    let (generation, run_states, generation_running, control, mut guard) = loop {
        match std::mem::replace(&mut claim.phase, SchedulerStartClaimPhase::Inactive) {
            SchedulerStartClaimPhase::Begin {
                generation,
                run_states,
                generation_running,
                control,
                guard,
            } => break (generation, run_states, generation_running, control, guard),
            SchedulerStartClaimPhase::Wait(waiting) => {
                claim.phase = SchedulerStartClaimPhase::Wait(waiting.clone());
                wait_for_owned_cleanup(&claim.lifecycle, waiting).await;
                claim.phase = SchedulerStartClaimPhase::Inactive;
                claim.refresh();
            }
            SchedulerStartClaimPhase::Inactive => return false,
        }
    };
    let lifecycle = Arc::clone(&claim.lifecycle);
    let cancellation = wait_for_generation_cancel(control.subscribe());
    let prepared = prepare(generation);
    tokio::pin!(cancellation);
    tokio::pin!(prepared);
    let prepared = tokio::select! {
        biased;
        _ = &mut cancellation => return false,
        prepared = &mut prepared => prepared,
    };
    if !lock_unpoisoned(&lifecycle).is_starting(generation) {
        return false;
    }
    let (handles, finish_start) = install(
        generation,
        Arc::clone(&run_states),
        Arc::clone(&generation_running),
        Arc::clone(&control),
        prepared,
    );
    let mut handles = handles.into_scheduler_handle_batch();
    handles.attach(Arc::clone(&control));
    let tasks = handles.task_ids();
    let required_tasks = required_tasks.as_deref().unwrap_or(&tasks);
    if tasks.iter().copied().collect::<HashSet<_>>()
        != required_tasks.iter().copied().collect::<HashSet<_>>()
        || !lock_unpoisoned(&lifecycle).register_required_loops(generation, required_tasks)
    {
        return false;
    }
    let (supervised, mut supervisors, mut commit_result) =
        supervise_scheduler_batch(generation, &control, &lifecycle, handles, |_, _| {});
    let install_result = {
        let mut lifecycle = lock_unpoisoned(&lifecycle);
        lifecycle.install_handles(generation, supervised)
    };
    if let Err(rejected) = install_result {
        drop(rejected);
        return false;
    }
    for (ready, _) in &mut supervisors {
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
    if supervisors.is_empty() {
        let committed = lock_unpoisoned(&lifecycle).finish_start(generation);
        if committed {
            guard.commit();
        }
        return committed;
    }
    for (_, commit) in supervisors {
        if commit.send(()).is_err() {
            return false;
        }
    }
    let cancellation = wait_for_generation_cancel(control.subscribe());
    tokio::pin!(cancellation);
    let committed = tokio::select! {
        biased;
        _ = &mut cancellation => false,
        result = commit_result.wait_for(|result| result.is_some()) => {
            result.ok().and_then(|result| *result).unwrap_or(false)
        }
    };
    if committed {
        guard.commit();
    }
    committed
}

#[cfg(test)]
async fn stop_scheduler_lifecycle(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    _running: &Arc<AtomicBool>,
) -> bool {
    stop_scheduler_lifecycle_inner(lifecycle).await
}

async fn stop_scheduler_lifecycle_inner(lifecycle: &Arc<StdMutex<SchedulerLifecycle>>) -> bool {
    stop_scheduler_lifecycle_with_mode(lifecycle, false).await
}

async fn shutdown_scheduler_lifecycle(lifecycle: &Arc<StdMutex<SchedulerLifecycle>>) -> bool {
    stop_scheduler_lifecycle_with_mode(lifecycle, true).await
}

async fn stop_scheduler_lifecycle_with_mode(
    lifecycle: &Arc<StdMutex<SchedulerLifecycle>>,
    terminal: bool,
) -> bool {
    let (request, recovery_worker, recovery_drain) = {
        let mut state = lock_unpoisoned(lifecycle);
        let request = if terminal {
            state.begin_terminal_shutdown()
        } else {
            state.begin_explicit_stop()
        };
        (
            request,
            state.recovery_worker.take(),
            Arc::clone(&state.recovery_drain),
        )
    };
    let active = !matches!(request, CleanupRequest::Inactive);
    let completion = launch_scheduler_cleanup(lifecycle, request);
    if let Some(worker) = recovery_worker {
        worker.abort();
        let _ = worker.await;
    }
    recovery_drain.wait_drained().await;
    if let Some(completion) = completion {
        wait_for_owned_cleanup(lifecycle, completion).await;
    }
    active
}

struct SchedulerRuntime {
    refs: SchedulerRefs,
    lifecycle: Arc<StdMutex<SchedulerLifecycle>>,
    market_fallback_interval_tx: watch::Sender<Duration>,
}

impl SchedulerRuntime {
    fn start_explicit(self: &Arc<Self>) -> impl std::future::Future<Output = bool> {
        self.start_with_origin(SchedulerStartOrigin::Explicit)
    }

    fn start_recovery(
        self: &Arc<Self>,
        ticket: RecoveryTicket,
    ) -> impl std::future::Future<Output = bool> {
        self.start_with_origin(SchedulerStartOrigin::Recovery(ticket))
    }

    fn start_with_origin(
        self: &Arc<Self>,
        origin: SchedulerStartOrigin,
    ) -> impl std::future::Future<Output = bool> {
        let prepare_runtime = Arc::clone(self);
        let install_runtime = Arc::clone(self);
        start_scheduler_lifecycle_with_origin(
            &self.lifecycle,
            origin,
            Some(periodic_task_ids()),
            move |_| {
                let runtime = Arc::clone(&prepare_runtime);
                async move {
                    let config = runtime.refs.config.read().await;
                    configured_task_interval(TaskId::MarketFallback, &config)
                        .expect("market fallback has an interval")
                }
            },
            move |generation, run_states, generation_running, control, market_fallback_interval| {
                install_runtime
                    .market_fallback_interval_tx
                    .send_replace(market_fallback_interval);
                // Own the batch before the first spawn. If synchronous
                // installation unwinds at any later instruction, Drop moves
                // every accumulated handle into the generation drain.
                let mut handles = SchedulerHandleBatch::attached(Arc::clone(&control));
                for task in periodic_task_ids().iter().copied() {
                    let run_state = run_states.get(&task).expect("task registered").clone();
                    let handle = if task == TaskId::MarketFallback {
                        install_runtime.spawn_market_fallback(
                            task,
                            generation,
                            Arc::clone(&control),
                            run_state,
                            Arc::clone(&generation_running),
                        )
                    } else {
                        install_runtime.spawn_periodic(
                            task,
                            generation,
                            Arc::clone(&control),
                            task.interval().expect("periodic task"),
                            run_state,
                            Arc::clone(&generation_running),
                        )
                    };
                    handles.push(task, handle);
                }
                let lifecycle = Arc::clone(&install_runtime.lifecycle);
                let finish_runtime = Arc::clone(&install_runtime);
                let finish_start = async move {
                    let time_state = run_states
                        .get(&TaskId::TimeSync)
                        .expect("time task registered")
                        .clone();
                    let _ = run_scheduled_task(time_state, true, || {
                        finish_runtime.refs.execute(TaskId::TimeSync)
                    })
                    .await;
                    if lock_unpoisoned(&lifecycle).is_starting(generation) {
                        let environment_state = run_states
                            .get(&TaskId::Environment)
                            .expect("environment task registered")
                            .clone();
                        let _ = run_scheduled_task(environment_state, true, || {
                            finish_runtime.refs.execute(TaskId::Environment)
                        })
                        .await;
                    }
                };
                (handles, finish_start)
            },
        )
    }

    fn spawn_periodic(
        &self,
        task: TaskId,
        generation: u64,
        control: Arc<GenerationControl>,
        interval: Duration,
        run_state: TaskRunStateRef,
        generation_running: Arc<AtomicBool>,
    ) -> SchedulerHandle {
        let scheduler = self.refs.clone();
        let lifecycle = Arc::clone(&self.lifecycle);
        spawn_required_loop_future(task, generation, control, lifecycle, async move {
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
        task: TaskId,
        generation: u64,
        control: Arc<GenerationControl>,
        run_state: TaskRunStateRef,
        generation_running: Arc<AtomicBool>,
    ) -> SchedulerHandle {
        let scheduler = self.refs.clone();
        let receiver = self.market_fallback_interval_tx.subscribe();
        let lifecycle = Arc::clone(&self.lifecycle);
        spawn_required_loop_future(task, generation, control, lifecycle, async move {
            run_reschedulable_periodic(
                generation_running,
                run_state,
                receiver,
                Duration::ZERO,
                move || {
                    let scheduler = scheduler.clone();
                    async move { scheduler.execute(TaskId::MarketFallback).await }
                },
            )
            .await;
        })
    }

    fn recovery_future(
        self: &Arc<Self>,
        ticket: RecoveryTicket,
        completion: watch::Receiver<bool>,
    ) -> RecoveryFuture {
        let runtime = Arc::clone(self);
        Box::pin(async move {
            wait_for_owned_cleanup(&runtime.lifecycle, completion).await;
            let delay = recovery_backoff(ticket.attempt);
            let cancellation = wait_for_generation_cancel(ticket.desired.subscribe());
            tokio::pin!(cancellation);
            tokio::select! {
                biased;
                _ = &mut cancellation => return,
                _ = tokio::time::sleep(delay) => {}
            }
            if !ticket.desired.is_active() {
                return;
            }
            tracing::warn!(
                failed_generation = ticket.failed_generation,
                attempt = ticket.attempt,
                backoff_ms = delay.as_millis(),
                "restarting scheduler after required loop failure"
            );
            let _ = runtime.start_recovery(ticket).await;
        })
    }
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
    notification_observer: SessionNotificationObserver,
    lifecycle: Arc<StdMutex<SchedulerLifecycle>>,
    kline_flush: Arc<KlineFlushCoordinator>,
    market_fallback_interval_tx: tokio::sync::watch::Sender<Duration>,
    prev_public_connected: Arc<Mutex<bool>>,
    prev_private_connected: Arc<Mutex<bool>>,
    runtime: Arc<SchedulerRuntime>,
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
        notification_observer: SessionNotificationObserver,
    ) -> Self {
        let (market_fallback_interval_tx, _) =
            tokio::sync::watch::channel(INTERVAL_MARKET_FALLBACK);
        let lifecycle = Arc::new(StdMutex::new(SchedulerLifecycle::new()));
        let kline_flush = Arc::new(KlineFlushCoordinator::new());
        let prev_public_connected = Arc::new(Mutex::new(false));
        let prev_private_connected = Arc::new(Mutex::new(false));
        let runtime = Arc::new(SchedulerRuntime {
            refs: SchedulerRefs {
                time: Arc::clone(&time),
                market: Arc::clone(&market),
                chart_workspace: Arc::clone(&chart_workspace),
                trading: Arc::clone(&trading),
                daily_pnl: Arc::clone(&daily_pnl),
                connection: Arc::clone(&connection),
                ws: Arc::clone(&ws),
                config: Arc::clone(&config),
                notification: Arc::clone(&notification),
                emitter: emitter.clone(),
                api: Arc::clone(&api),
                environment_status: Arc::clone(&environment_status),
                account_lifecycle: Arc::clone(&account_lifecycle),
                notification_observer: notification_observer.clone(),
                kline_flush: Arc::clone(&kline_flush),
                prev_public_connected: Arc::clone(&prev_public_connected),
                prev_private_connected: Arc::clone(&prev_private_connected),
            },
            lifecycle: Arc::clone(&lifecycle),
            market_fallback_interval_tx: market_fallback_interval_tx.clone(),
        });
        let weak_runtime = Arc::downgrade(&runtime);
        let weak_lifecycle = Arc::downgrade(&lifecycle);
        lock_unpoisoned(&lifecycle).recovery_handler = Some(Arc::new(move |ticket, completion| {
            if let Some(runtime) = weak_runtime.upgrade() {
                runtime.recovery_future(ticket, completion)
            } else {
                if let Some(lifecycle) = weak_lifecycle.upgrade() {
                    let mut lifecycle = lock_unpoisoned(&lifecycle);
                    lifecycle.terminal = true;
                    lifecycle.disable_desired_running();
                }
                Box::pin(async {})
            }
        }));
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
            notification_observer,
            lifecycle,
            kline_flush,
            market_fallback_interval_tx,
            prev_public_connected,
            prev_private_connected,
            runtime,
        }
    }

    pub fn start(&self) -> impl std::future::Future<Output = ()> {
        let start = self.runtime.start_explicit();
        async move {
            let _ = start.await;
        }
    }

    pub async fn stop(&self) {
        stop_scheduler_lifecycle_inner(&self.lifecycle).await;
    }

    pub async fn shutdown(&self) {
        shutdown_scheduler_lifecycle(&self.lifecycle).await;
    }

    pub(crate) async fn flush_klines_for_shutdown(&self) -> AppResult<()> {
        execute_kline_flush(
            Arc::clone(&self.chart_workspace),
            Arc::clone(&self.kline_flush),
            KlineFlushOrigin::Shutdown,
        )
        .await
    }

    pub fn set_market_fallback_interval(&self, interval: Duration) {
        self.market_fallback_interval_tx.send_replace(interval);
    }

    pub async fn bootstrap_connection(&self) -> AppResult<()> {
        run_generation_activity(&self.lifecycle, || async {
            self.bootstrap_connection_inner().await
        })
        .await
    }

    pub fn spawn_connection_bootstrap(self: &Arc<Self>) -> AppResult<()> {
        let activity = claim_generation_activity(&self.lifecycle)?;
        let scheduler = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let result = run_claimed_generation_activity(activity, || async {
                scheduler.bootstrap_connection_inner().await
            })
            .await;
            if let Err(error) = result {
                tracing::warn!(
                    message = %error.user_message(),
                    "connection bootstrap did not complete"
                );
            }
        });
        Ok(())
    }

    async fn bootstrap_connection_inner(&self) -> AppResult<()> {
        let (tasks, mut failed) = run_bootstrap_account_phase(
            self.account_lifecycle.as_ref(),
            || async { self.connection.status().await == ConnectionStatus::Connected },
            |task| self.execute_inner(task, ExecutionMode::Bootstrap),
        )
        .await;
        if let Err(error) = self.run_environment().await {
            failed.push((TaskId::Environment, error));
        }
        failed.sort_by_key(|(failed_task, _)| {
            tasks
                .iter()
                .position(|task| task == failed_task)
                .unwrap_or(usize::MAX)
        });
        for (task, error) in &failed {
            if bootstrap_failure_needs_generic_error(error) {
                self.emitter
                    .emit_error(&format!("连接后{}同步失败", task.bootstrap_label()));
            }
        }
        if failed.is_empty() {
            Ok(())
        } else {
            let failed_tasks = failed.into_iter().map(|(task, _)| task).collect::<Vec<_>>();
            Err(bootstrap_failure_error(&failed_tasks))
        }
    }

    pub async fn run_now(&self, task: TaskId, force: bool) -> AppResult<()> {
        let run_state = lock_unpoisoned(&self.lifecycle)
            .run_state(task)
            .ok_or_else(|| crate::error::AppError::Internal("调度器未运行".into()))?;
        run_scheduled_task(run_state, force, || self.execute(task)).await
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
            TaskId::DailyPnl => {
                let context = capture_session_context(&self.config, &self.account_lifecycle).await;
                self.daily_pnl.refresh(&context).await.map(|_| ())
            }
            TaskId::MarketFallback => self.run_market_fallback(mode).await,
            TaskId::KlineFlush => {
                execute_kline_flush(
                    self.chart_workspace.clone(),
                    self.kline_flush.clone(),
                    KlineFlushOrigin::Background,
                )
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
                let context = capture_session_context(&self.config, &self.account_lifecycle).await;
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
                self.emitter.emit_account_snapshot(
                    &context,
                    AccountSummary {
                        account_id: context.account_id.clone(),
                        balances,
                        total_equity,
                    },
                );
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
                let context = capture_session_context(&self.config, &self.account_lifecycle).await;
                let symbol = self.market.active_symbol().await;
                let sym = Some(symbol.as_str());
                let open_orders = self.trading.fetch_open_orders_unobserved(sym).await?;
                let order_history = self
                    .trading
                    .fetch_order_history_unobserved(sym, Some(50))
                    .await?;
                let order_snapshots = open_orders
                    .iter()
                    .chain(&order_history)
                    .cloned()
                    .collect::<Vec<_>>();
                self.ws
                    .seed_order_snapshots_and_mark(&context, &order_snapshots)
                    .await?;
                let params = build_order_query_params(
                    sym, None, None, None, None, None, None, None, None, None,
                );
                let payload = self.api.private_get(endpoints::POSITIONS, params).await?;
                let meta = list_envelope_meta(&payload);
                let positions = parse_positions(&payload);
                warn_if_parse_empty(&self.emitter, "position/list", &payload, positions.len());
                warn_if_raw_parsed_mismatch(&self.emitter, "position/list", &meta, positions.len());
                self.emitter.emit_private_panels_snapshot(
                    &context,
                    PrivatePanelsSnapshot {
                        open_orders,
                        order_history,
                        positions,
                    },
                );
                Ok(())
            },
        )
        .await
    }

    async fn run_market_fallback(&self, mode: ExecutionMode) -> AppResult<()> {
        let symbol = self.market.active_symbol().await;
        let public_connected = self.ws.is_public_connected();
        let private_connected = self.ws.is_private_connected();
        let _ = run_market_reconnect_backfill(
            Arc::clone(&self.prev_public_connected),
            public_connected,
            || async {
                let interval = self.market.kline_interval().await;
                run_scheduler_owned_market_backfill(|| {
                    self.market
                        .run_kline_backfill_under_account_guard(&symbol, &interval)
                })
                .await
            },
        )
        .await;
        {
            let mut prev = self.prev_private_connected.lock().await;
            *prev = private_connected;
        }

        run_rest_snapshot_if_needed(mode, self.ws.is_market_healthy(PUBLIC_STALE_MS), || async {
            let mut failures = Vec::new();
            if let Err(error) = self.market.refresh_ticker_depth(&symbol).await {
                failures.push(error.to_string());
            }
            if let Err(error) = self
                .market
                .refresh_klines_under_account_guard(&symbol)
                .await
            {
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
            &self.account_lifecycle,
            &self.notification_observer,
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
    notification_observer: SessionNotificationObserver,
    kline_flush: Arc<KlineFlushCoordinator>,
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
            TaskId::DailyPnl => {
                let context = capture_session_context(&self.config, &self.account_lifecycle).await;
                self.daily_pnl.refresh(&context).await.map(|_| ())
            }
            TaskId::MarketFallback => self.run_market_fallback(mode).await,
            TaskId::KlineFlush => {
                execute_kline_flush(
                    self.chart_workspace.clone(),
                    self.kline_flush.clone(),
                    KlineFlushOrigin::Background,
                )
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
                let context = capture_session_context(&self.config, &self.account_lifecycle).await;
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
                self.emitter.emit_account_snapshot(
                    &context,
                    AccountSummary {
                        account_id: context.account_id.clone(),
                        balances,
                        total_equity,
                    },
                );
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
                let context = capture_session_context(&self.config, &self.account_lifecycle).await;
                let symbol = self.market.active_symbol().await;
                let sym = Some(symbol.as_str());
                let open_orders = self.trading.fetch_open_orders_unobserved(sym).await?;
                let order_history = self
                    .trading
                    .fetch_order_history_unobserved(sym, Some(50))
                    .await?;
                let order_snapshots = open_orders
                    .iter()
                    .chain(&order_history)
                    .cloned()
                    .collect::<Vec<_>>();
                self.ws
                    .seed_order_snapshots_and_mark(&context, &order_snapshots)
                    .await?;
                let params = build_order_query_params(
                    sym, None, None, None, None, None, None, None, None, None,
                );
                let payload = self.api.private_get(endpoints::POSITIONS, params).await?;
                let meta = list_envelope_meta(&payload);
                let positions = parse_positions(&payload);
                warn_if_parse_empty(&self.emitter, "position/list", &payload, positions.len());
                warn_if_raw_parsed_mismatch(&self.emitter, "position/list", &meta, positions.len());
                self.emitter.emit_private_panels_snapshot(
                    &context,
                    PrivatePanelsSnapshot {
                        open_orders,
                        order_history,
                        positions,
                    },
                );
                Ok(())
            },
        )
        .await
    }

    async fn run_market_fallback(&self, mode: ExecutionMode) -> AppResult<()> {
        let symbol = self.market.active_symbol().await;
        let public_connected = self.ws.is_public_connected();
        let _ = run_market_reconnect_backfill(
            Arc::clone(&self.prev_public_connected),
            public_connected,
            || async {
                let interval = self.market.kline_interval().await;
                run_scheduler_owned_market_backfill(|| {
                    self.market
                        .run_kline_backfill_under_account_guard(&symbol, &interval)
                })
                .await
            },
        )
        .await;
        {
            let mut prev = self.prev_private_connected.lock().await;
            *prev = self.ws.is_private_connected();
        }
        run_rest_snapshot_if_needed(mode, self.ws.is_market_healthy(PUBLIC_STALE_MS), || async {
            let mut failures = Vec::new();
            if let Err(error) = self.market.refresh_ticker_depth(&symbol).await {
                failures.push(error.to_string());
            }
            if let Err(error) = self
                .market
                .refresh_klines_under_account_guard(&symbol)
                .await
            {
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
            &self.account_lifecycle,
            &self.notification_observer,
        )
        .await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnvironmentProbeSnapshot {
    context: SessionContext,
    probe_base_url: Option<String>,
    safe_base_url: String,
    environment_key: String,
    environment: NotificationEnvironment,
    label: &'static str,
}

impl EnvironmentProbeSnapshot {
    fn new(context: SessionContext, base_url: &str) -> Self {
        let parsed = canonical_api_base_url(base_url)
            .and_then(|canonical| reqwest::Url::parse(&canonical).ok());
        let (probe_base_url, safe_base_url, environment, label, identity) = match parsed {
            Some(mut url) => {
                if matches!(
                    (url.scheme(), url.port()),
                    ("https", Some(443)) | ("http", Some(80))
                ) {
                    let _ = url.set_port(None);
                }
                let normalized_path = url.path().trim_end_matches('/').to_string();
                url.set_path(&normalized_path);
                let normalized = url.to_string().trim_end_matches('/').to_string();
                let production = url.scheme() == "https"
                    && url.host_str() == Some("api.easicoin.io")
                    && url.port().is_none()
                    && normalized_path.is_empty();
                if production {
                    (
                        Some(normalized.clone()),
                        "production".to_string(),
                        NotificationEnvironment::Production,
                        "正式",
                        normalized,
                    )
                } else {
                    (
                        Some(normalized.clone()),
                        "custom-environment".to_string(),
                        NotificationEnvironment::Development,
                        "开发",
                        normalized,
                    )
                }
            }
            None => (
                None,
                "invalid-environment".to_string(),
                NotificationEnvironment::Unknown,
                "未知",
                "invalid".to_string(),
            ),
        };
        let mut digest = Sha256::new();
        digest.update(b"easiflux.environment.v1\0");
        digest.update(identity.as_bytes());
        let environment_key = format!("env-v1-{:x}", digest.finalize());
        Self {
            context,
            probe_base_url,
            safe_base_url,
            environment_key,
            environment,
            label,
        }
    }
}

fn confirmed_environment_status(
    snapshot: &EnvironmentProbeSnapshot,
    reachable: bool,
    checked_at: u64,
) -> EnvironmentStatus {
    EnvironmentStatus {
        label: snapshot.label.to_string(),
        base_url: snapshot.safe_base_url.clone(),
        reachable,
        checked_at,
        error: (!reachable).then(|| "环境不可达".to_string()),
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
    account_lifecycle: &Arc<AccountLifecycleCoordinator>,
    notification_observer: &SessionNotificationObserver,
) -> AppResult<()> {
    let (active_account_id, selected_base_url) = {
        let config = config.read().await;
        let active_account_id = normalize_account_id(&config.active_account_id);
        let selected_base_url = CredentialStore::load(&active_account_id)?
            .map(|credential| credential.base_url)
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        (active_account_id, selected_base_url)
    };
    let snapshot = EnvironmentProbeSnapshot::new(
        SessionContext {
            account_id: active_account_id,
            session_epoch: account_lifecycle.current_session_epoch(),
        },
        &selected_base_url,
    );
    let reachable = if let Some(base_url) = snapshot.probe_base_url.as_deref() {
        let client = environment_probe_client(base_url).await;
        PublicApi::server_time(&client).await.is_ok()
    } else {
        false
    };
    let checked_at = time.local_now_ms();
    let _commit_guard = account_lifecycle.read_guard().await;
    let current_base_url = {
        let current_account = normalize_account_id(&config.read().await.active_account_id);
        if current_account != snapshot.context.account_id
            || account_lifecycle.current_session_epoch() != snapshot.context.session_epoch
        {
            return Ok(());
        }
        CredentialStore::load(&current_account)?
            .map(|credential| credential.base_url)
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
    };
    if EnvironmentProbeSnapshot::new(snapshot.context.clone(), &current_base_url).environment_key
        != snapshot.environment_key
    {
        return Ok(());
    }
    let status = confirmed_environment_status(&snapshot, reachable, checked_at);
    let task_result = publish_environment_task_status(environment_status, status, |status| {
        emitter.emit_environment_updated(&snapshot.context, status);
    })
    .await;
    notification_observer
        .observe_environment_guarded(
            &snapshot.context,
            &snapshot.environment_key,
            snapshot.environment,
            reachable,
            checked_at,
        )
        .await;
    if reachable {
        task_result
    } else {
        Err(AppError::Observed("环境检测失败"))
    }
}

#[cfg(test)]
mod tests;
