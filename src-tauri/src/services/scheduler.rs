use std::collections::HashMap;
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
    DEFAULT_BASE_URL,
};
use crate::models::time::{TimeSnapshot, TimeSyncStatus};
use crate::models::trading::PrivatePanelsSnapshot;
use crate::services::{
    AccountLifecycleCoordinator, ConnectionService, DailyPnlService, MarketService, TimeService,
    TradingService,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskId {
    TimeSync,
    FundingRate,
    Balances,
    PrivatePanels,
    DailyPnl,
    MarketFallback,
    Environment,
}

impl TaskId {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "time" | "timeSync" => Some(Self::TimeSync),
            "funding" | "fundingRate" => Some(Self::FundingRate),
            "account" | "balances" => Some(Self::Balances),
            "privatePanels" => Some(Self::PrivatePanels),
            "dailyPnl" => Some(Self::DailyPnl),
            "market" | "marketFallback" => Some(Self::MarketFallback),
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
            Self::Environment => None,
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
            Self::Environment => "环境检测",
        }
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
    _task: TaskId,
    operation: F,
) -> AppResult<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    crate::services::account_profiles::run_account_public_operation(coordinator, operation).await
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

struct TaskRuntime {
    run_state: Arc<Mutex<TaskRunState>>,
    handle: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl TaskRuntime {
    fn new() -> Self {
        Self {
            run_state: Arc::new(Mutex::new(TaskRunState::default())),
            handle: Mutex::new(None),
        }
    }
}

pub struct SchedulerService {
    time: Arc<TimeService>,
    market: Arc<MarketService>,
    trading: Arc<TradingService>,
    daily_pnl: Arc<DailyPnlService>,
    connection: Arc<ConnectionService>,
    ws: Arc<WsManager>,
    config: Arc<RwLock<AppConfig>>,
    emitter: EventEmitter,
    api: Arc<crate::api::ApiClient>,
    environment_status: Arc<RwLock<EnvironmentStatus>>,
    account_lifecycle: Arc<AccountLifecycleCoordinator>,
    running: Arc<AtomicBool>,
    tasks: HashMap<TaskId, TaskRuntime>,
    prev_public_connected: Arc<Mutex<bool>>,
    prev_private_connected: Arc<Mutex<bool>>,
}

impl SchedulerService {
    pub fn new(
        time: Arc<TimeService>,
        market: Arc<MarketService>,
        trading: Arc<TradingService>,
        daily_pnl: Arc<DailyPnlService>,
        connection: Arc<ConnectionService>,
        ws: Arc<WsManager>,
        config: Arc<RwLock<AppConfig>>,
        emitter: EventEmitter,
        api: Arc<crate::api::ApiClient>,
        environment_status: Arc<RwLock<EnvironmentStatus>>,
        account_lifecycle: Arc<AccountLifecycleCoordinator>,
    ) -> Self {
        let mut tasks = HashMap::new();
        for id in [
            TaskId::TimeSync,
            TaskId::FundingRate,
            TaskId::Balances,
            TaskId::PrivatePanels,
            TaskId::DailyPnl,
            TaskId::MarketFallback,
            TaskId::Environment,
        ] {
            tasks.insert(id, TaskRuntime::new());
        }
        Self {
            time,
            market,
            trading,
            daily_pnl,
            connection,
            ws,
            config,
            emitter,
            api,
            environment_status,
            account_lifecycle,
            running: Arc::new(AtomicBool::new(false)),
            tasks,
            prev_public_connected: Arc::new(Mutex::new(false)),
            prev_private_connected: Arc::new(Mutex::new(false)),
        }
    }

    pub async fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        for id in [
            TaskId::TimeSync,
            TaskId::FundingRate,
            TaskId::Balances,
            TaskId::PrivatePanels,
            TaskId::DailyPnl,
            TaskId::MarketFallback,
        ] {
            if let Some(interval) = id.interval() {
                self.spawn_periodic(id, interval).await;
            }
        }
        let _ = self.run_now(TaskId::TimeSync, true).await;
        let _ = self.run_now(TaskId::Environment, true).await;
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        for runtime in self.tasks.values() {
            if let Some(handle) = runtime.handle.lock().await.take() {
                handle.abort();
            }
        }
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

    async fn spawn_periodic(&self, task: TaskId, interval: Duration) {
        let runtime = self.tasks.get(&task).expect("task registered");
        let scheduler = self.clone_refs();
        let run_state = runtime.run_state.clone();
        let handle = tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                if !scheduler.running.load(Ordering::Relaxed) {
                    break;
                }
                let _ =
                    run_scheduled_task(run_state.as_ref(), false, || scheduler.execute(task)).await;
            }
        });
        *runtime.handle.lock().await = Some(handle);
    }

    fn clone_refs(&self) -> SchedulerRefs {
        SchedulerRefs {
            time: self.time.clone(),
            market: self.market.clone(),
            trading: self.trading.clone(),
            daily_pnl: self.daily_pnl.clone(),
            connection: self.connection.clone(),
            ws: self.ws.clone(),
            config: self.config.clone(),
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
            TaskId::Environment => self.run_environment().await,
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

struct SchedulerRefs {
    time: Arc<TimeService>,
    market: Arc<MarketService>,
    trading: Arc<TradingService>,
    daily_pnl: Arc<DailyPnlService>,
    connection: Arc<ConnectionService>,
    ws: Arc<WsManager>,
    config: Arc<RwLock<AppConfig>>,
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
            TaskId::Environment => self.run_environment().await,
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
