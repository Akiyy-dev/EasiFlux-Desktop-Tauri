use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{oneshot, Mutex, RwLock};

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

async fn execute_kline_flush(service: Arc<ChartWorkspaceService>) -> AppResult<()> {
    let outcomes = tokio::task::spawn_blocking(move || service.flush_dirty_klines())
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

#[derive(Debug, Default)]
struct TaskRunState {
    in_flight: bool,
    pending_force: bool,
    forced_rerun: bool,
    pending_force_waiters: Vec<oneshot::Sender<AppResult<()>>>,
    active_force_waiters: Vec<oneshot::Sender<AppResult<()>>>,
}

enum TaskRunClaim {
    Owner,
    Skip,
    Wait(oneshot::Receiver<AppResult<()>>),
}

async fn run_scheduled_task<F, Fut>(
    run_state: &Mutex<TaskRunState>,
    force_if_busy: bool,
    execute: F,
) -> AppResult<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let claim = {
        let mut state = run_state.lock().await;
        if state.in_flight {
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
            state.in_flight = true;
            state.forced_rerun = false;
            TaskRunClaim::Owner
        }
    };
    match claim {
        TaskRunClaim::Owner => drain_scheduled_runs(run_state, execute).await,
        TaskRunClaim::Skip => Ok(()),
        TaskRunClaim::Wait(receiver) => receiver
            .await
            .unwrap_or_else(|_| Err(AppError::Internal("调度任务在强制重跑完成前被取消".into()))),
    }
}

async fn run_reschedulable_periodic<F, Fut>(
    running: Arc<AtomicBool>,
    run_state: Arc<Mutex<TaskRunState>>,
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
                let _ = run_scheduled_task(run_state.as_ref(), false, &mut execute).await;
            }
        }
    }
}

async fn run_fixed_periodic<F, Fut>(
    running: Arc<AtomicBool>,
    run_state: Arc<Mutex<TaskRunState>>,
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
        let _ = run_scheduled_task(run_state.as_ref(), false, &mut execute).await;
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
    run_state: &Mutex<TaskRunState>,
    mut execute: F,
) -> AppResult<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    loop {
        let result = execute().await;
        let completed_waiters = {
            let mut state = run_state.lock().await;
            if !state.forced_rerun && state.pending_force {
                state.pending_force = false;
                state.forced_rerun = true;
                let pending = std::mem::take(&mut state.pending_force_waiters);
                state.active_force_waiters.extend(pending);
                None
            } else {
                state.in_flight = false;
                state.forced_rerun = false;
                Some(std::mem::take(&mut state.active_force_waiters))
            }
        };
        let Some(waiters) = completed_waiters else {
            continue;
        };
        for waiter in waiters {
            let _ = waiter.send(result.clone());
        }
        return result;
    }
}

type SchedulerHandle = tauri::async_runtime::JoinHandle<()>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchedulerLifecycleState {
    Stopped,
    Starting { generation: u64 },
    Running { generation: u64 },
}

struct SchedulerLifecycle {
    state: SchedulerLifecycleState,
    next_generation: u64,
    handles: HashMap<TaskId, SchedulerHandle>,
}

impl SchedulerLifecycle {
    fn new() -> Self {
        Self {
            state: SchedulerLifecycleState::Stopped,
            next_generation: 0,
            handles: HashMap::new(),
        }
    }

    fn begin_start(&mut self) -> Option<u64> {
        if self.state != SchedulerLifecycleState::Stopped {
            return None;
        }
        self.next_generation = self.next_generation.saturating_add(1);
        let generation = self.next_generation;
        self.state = SchedulerLifecycleState::Starting { generation };
        Some(generation)
    }

    fn install_handles(&mut self, handles: Vec<(TaskId, SchedulerHandle)>) {
        for (task, handle) in handles {
            if let Some(previous) = self.handles.insert(task, handle) {
                previous.abort();
            }
        }
    }

    fn finish_start(&mut self, generation: u64) {
        debug_assert_eq!(self.state, SchedulerLifecycleState::Starting { generation });
        self.state = SchedulerLifecycleState::Running { generation };
    }

    fn stop(&mut self) -> (bool, Vec<SchedulerHandle>) {
        let was_active = self.state != SchedulerLifecycleState::Stopped || !self.handles.is_empty();
        self.state = SchedulerLifecycleState::Stopped;
        let handles = std::mem::take(&mut self.handles).into_values().collect();
        (was_active, handles)
    }

    #[cfg(test)]
    fn running_generation(&self) -> Option<u64> {
        match self.state {
            SchedulerLifecycleState::Running { generation } => Some(generation),
            SchedulerLifecycleState::Stopped | SchedulerLifecycleState::Starting { .. } => None,
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

async fn start_scheduler_lifecycle<F, PrepareFut, FinishFut>(
    lifecycle: &Mutex<SchedulerLifecycle>,
    running: &Arc<AtomicBool>,
    install: F,
) -> bool
where
    F: FnOnce(u64) -> PrepareFut,
    PrepareFut: std::future::Future<Output = (Vec<(TaskId, SchedulerHandle)>, FinishFut)>,
    FinishFut: std::future::Future<Output = ()>,
{
    let mut lifecycle = lifecycle.lock().await;
    let Some(generation) = lifecycle.begin_start() else {
        return false;
    };
    running.store(true, Ordering::SeqCst);
    let (handles, finish_start) = install(generation).await;
    lifecycle.install_handles(handles);
    finish_start.await;
    lifecycle.finish_start(generation);
    true
}

async fn stop_scheduler_lifecycle(
    lifecycle: &Mutex<SchedulerLifecycle>,
    running: &Arc<AtomicBool>,
) -> bool {
    let mut lifecycle = lifecycle.lock().await;
    running.store(false, Ordering::SeqCst);
    let (was_active, handles) = lifecycle.stop();
    for handle in handles {
        handle.abort();
    }
    was_active
}

struct TaskRuntime {
    run_state: Arc<Mutex<TaskRunState>>,
}

impl TaskRuntime {
    fn new() -> Self {
        Self {
            run_state: Arc::new(Mutex::new(TaskRunState::default())),
        }
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
    running: Arc<AtomicBool>,
    lifecycle: Mutex<SchedulerLifecycle>,
    tasks: HashMap<TaskId, TaskRuntime>,
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
        let mut tasks = HashMap::new();
        for &id in TaskId::all() {
            tasks.insert(id, TaskRuntime::new());
        }
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
            lifecycle: Mutex::new(SchedulerLifecycle::new()),
            tasks,
            market_fallback_interval_tx,
            prev_public_connected: Arc::new(Mutex::new(false)),
            prev_private_connected: Arc::new(Mutex::new(false)),
        }
    }

    pub async fn start(&self) {
        start_scheduler_lifecycle(&self.lifecycle, &self.running, |_| async {
            let market_fallback_interval = {
                let config = self.config.read().await;
                configured_task_interval(TaskId::MarketFallback, &config)
                    .expect("market fallback has an interval")
            };
            self.market_fallback_interval_tx
                .send_replace(market_fallback_interval);
            let handles = periodic_task_ids()
                .iter()
                .copied()
                .map(|task| {
                    let handle = if task == TaskId::MarketFallback {
                        self.spawn_market_fallback()
                    } else {
                        self.spawn_periodic(task, task.interval().expect("periodic task"))
                    };
                    (task, handle)
                })
                .collect();
            let finish_start = async {
                let _ = self.run_now(TaskId::TimeSync, true).await;
                let _ = self.run_now(TaskId::Environment, true).await;
            };
            (handles, finish_start)
        })
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
        let runtime = self
            .tasks
            .get(&task)
            .ok_or_else(|| crate::error::AppError::Internal("未知调度任务".into()))?;
        run_scheduled_task(runtime.run_state.as_ref(), force, || self.execute(task)).await
    }

    fn spawn_periodic(&self, task: TaskId, interval: Duration) -> SchedulerHandle {
        let runtime = self.tasks.get(&task).expect("task registered");
        let scheduler = self.clone_refs();
        let run_state = runtime.run_state.clone();
        tauri::async_runtime::spawn(async move {
            run_fixed_periodic(
                scheduler.running.clone(),
                run_state,
                first_tick_delay(task),
                interval,
                || scheduler.execute(task),
            )
            .await;
        })
    }

    fn spawn_market_fallback(&self) -> SchedulerHandle {
        let runtime = self
            .tasks
            .get(&TaskId::MarketFallback)
            .expect("task registered");
        let scheduler = self.clone_refs();
        let run_state = Arc::clone(&runtime.run_state);
        let receiver = self.market_fallback_interval_tx.subscribe();
        tauri::async_runtime::spawn(run_reschedulable_periodic(
            Arc::clone(&self.running),
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
            running: self.running.clone(),
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
            TaskId::KlineFlush => execute_kline_flush(self.chart_workspace.clone()).await,
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
    running: Arc<AtomicBool>,
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
            TaskId::KlineFlush => execute_kline_flush(self.chart_workspace.clone()).await,
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
