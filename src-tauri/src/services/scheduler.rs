use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, RwLock};

use crate::api::diagnostic::{warn_if_parse_empty, warn_if_raw_parsed_mismatch};
use crate::api::mapper::{build_order_query_params, list_envelope_meta, parse_balances, parse_positions};
use crate::api::endpoints;
use crate::error::AppResult;
use crate::events::EventEmitter;
use crate::models::account::AccountSummary;
use crate::models::config::{environment_label, normalize_account_id, AppConfig, ConnectionStatus, EnvironmentStatus};
use crate::models::trading::PrivatePanelsSnapshot;
use crate::services::{
    ConnectionService, DailyPnlService, MarketService, TimeService, TradingService,
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
}

struct TaskRuntime {
    in_flight: Arc<AtomicBool>,
    pending_force: Arc<AtomicBool>,
    handle: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl TaskRuntime {
    fn new() -> Self {
        Self {
            in_flight: Arc::new(AtomicBool::new(false)),
            pending_force: Arc::new(AtomicBool::new(false)),
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
        self.run_bootstrap_task(TaskId::TimeSync).await;
        self.run_bootstrap_task(TaskId::MarketFallback).await;
        self.run_bootstrap_task(TaskId::FundingRate).await;
        if self.connection.status().await == ConnectionStatus::Connected {
            self.run_bootstrap_task(TaskId::Balances).await;
            self.run_bootstrap_task(TaskId::PrivatePanels).await;
            self.run_bootstrap_task(TaskId::DailyPnl).await;
        }
        Ok(())
    }

    async fn run_bootstrap_task(&self, task: TaskId) {
        if let Err(error) = self.run_now(task, true).await {
            let label = match task {
                TaskId::TimeSync => "时间同步",
                TaskId::FundingRate => "资金费率",
                TaskId::Balances => "账户余额",
                TaskId::PrivatePanels => "订单/持仓",
                TaskId::DailyPnl => "今日盈亏",
                TaskId::MarketFallback => "行情快照",
                TaskId::Environment => "环境检测",
            };
            self.emitter
                .emit_error(&format!("连接后{label}同步失败: {}", error.user_message()));
        }
    }

    pub async fn run_now(&self, task: TaskId, force: bool) -> AppResult<()> {
        let runtime = self
            .tasks
            .get(&task)
            .ok_or_else(|| crate::error::AppError::Internal("未知调度任务".into()))?;
        if runtime.in_flight.load(Ordering::Acquire) {
            if force {
                runtime.pending_force.store(true, Ordering::Release);
            }
            return Ok(());
        }
        runtime.in_flight.store(true, Ordering::Release);
        loop {
            let result = self.execute(task).await;
            if runtime.pending_force.swap(false, Ordering::AcqRel) {
                continue;
            }
            runtime.in_flight.store(false, Ordering::Release);
            return result;
        }
    }

    async fn spawn_periodic(&self, task: TaskId, interval: Duration) {
        let runtime = self.tasks.get(&task).expect("task registered");
        let scheduler = self.clone_refs();
        let in_flight = runtime.in_flight.clone();
        let pending_force = runtime.pending_force.clone();
        let handle = tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                if !scheduler.running.load(Ordering::Relaxed) {
                    break;
                }
                if in_flight.load(Ordering::Acquire) {
                    continue;
                }
                in_flight.store(true, Ordering::Release);
                loop {
                    let _ = scheduler.execute(task).await;
                    if !pending_force.swap(false, Ordering::AcqRel) {
                        break;
                    }
                }
                in_flight.store(false, Ordering::Release);
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
            running: self.running.clone(),
            prev_public_connected: self.prev_public_connected.clone(),
            prev_private_connected: self.prev_private_connected.clone(),
        }
    }

    async fn execute(&self, task: TaskId) -> AppResult<()> {
        match task {
            TaskId::TimeSync => self.time.sync().await.map(|_| ()),
            TaskId::FundingRate => self.run_funding_rate().await,
            TaskId::Balances => self.run_balances().await,
            TaskId::PrivatePanels => self.run_private_panels().await,
            TaskId::DailyPnl => self.daily_pnl.refresh().await.map(|_| ()),
            TaskId::MarketFallback => self.run_market_fallback().await,
            TaskId::Environment => self.run_environment().await,
        }
    }

    async fn run_funding_rate(&self) -> AppResult<()> {
        let symbol = self.market.active_symbol().await;
        self.market.refresh_funding_rate(&symbol).await
    }

    async fn run_balances(&self) -> AppResult<()> {
        if self.connection.status().await != ConnectionStatus::Connected {
            return Ok(());
        }
        if self.ws.is_private_healthy(PRIVATE_STALE_MS) {
            return Ok(());
        }
        let account_id = {
            let cfg = self.config.read().await;
            normalize_account_id(&cfg.active_account_id)
        };
        let params = build_order_query_params(
            None, None, None, None, None, None, None, None, None, None,
        );
        let payload = self
            .api
            .private_get(endpoints::BALANCES, params)
            .await?;
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
    }

    async fn run_private_panels(&self) -> AppResult<()> {
        if self.connection.status().await != ConnectionStatus::Connected {
            return Ok(());
        }
        if self.ws.is_private_healthy(PRIVATE_STALE_MS) {
            return Ok(());
        }
        let symbol = self.market.active_symbol().await;
        let sym = Some(symbol.as_str());
        let open_orders = self.trading.fetch_open_orders(sym).await?;
        let order_history = self
            .trading
            .fetch_order_history(sym, Some(50))
            .await?;
        let params = build_order_query_params(
            sym, None, None, None, None, None, None, None, None, None,
        );
        let payload = self
            .api
            .private_get(endpoints::POSITIONS, params)
            .await?;
        let meta = list_envelope_meta(&payload);
        let positions = parse_positions(&payload);
        warn_if_parse_empty(&self.emitter, "position/list", &payload, positions.len());
        warn_if_raw_parsed_mismatch(&self.emitter, "position/list", &meta, positions.len());
        self.emitter.emit_private_panels_snapshot(PrivatePanelsSnapshot {
            open_orders,
            order_history,
            positions,
        });
        Ok(())
    }

    async fn run_market_fallback(&self) -> AppResult<()> {
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

        if self.ws.is_public_healthy(PUBLIC_STALE_MS) {
            return Ok(());
        }
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
    }

    async fn run_environment(&self) -> AppResult<()> {
        let active_account_id = {
            let config = self.config.read().await;
            normalize_account_id(&config.active_account_id)
        };
        let credential = CredentialStore::load(&active_account_id)?;
        let base_url = if let Some(credential) = credential.as_ref() {
            credential.base_url.clone()
        } else {
            self.api.base_url().await
        };
        let base_url = if base_url.trim().is_empty() {
            crate::models::config::DEFAULT_BASE_URL.to_string()
        } else {
            base_url
        };
        let checked_at = self.time.local_now_ms();
        let status = match crate::api::PublicApi::server_time(&self.api).await {
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
        self.emitter.emit_environment_updated(&status);
        *self.environment_status.write().await = status;
        Ok(())
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
    running: Arc<AtomicBool>,
    prev_public_connected: Arc<Mutex<bool>>,
    prev_private_connected: Arc<Mutex<bool>>,
}

impl SchedulerRefs {
    async fn execute(&self, task: TaskId) -> AppResult<()> {
        match task {
            TaskId::TimeSync => self.time.sync().await.map(|_| ()),
            TaskId::FundingRate => {
                let symbol = self.market.active_symbol().await;
                self.market.refresh_funding_rate(&symbol).await
            }
            TaskId::Balances => self.run_balances().await,
            TaskId::PrivatePanels => self.run_private_panels().await,
            TaskId::DailyPnl => self.daily_pnl.refresh().await.map(|_| ()),
            TaskId::MarketFallback => self.run_market_fallback().await,
            TaskId::Environment => self.run_environment().await,
        }
    }

    async fn run_balances(&self) -> AppResult<()> {
        if self.connection.status().await != ConnectionStatus::Connected {
            return Ok(());
        }
        if self.ws.is_private_healthy(PRIVATE_STALE_MS) {
            return Ok(());
        }
        let account_id = {
            let cfg = self.config.read().await;
            normalize_account_id(&cfg.active_account_id)
        };
        let params = build_order_query_params(
            None, None, None, None, None, None, None, None, None, None,
        );
        let payload = self
            .api
            .private_get(endpoints::BALANCES, params)
            .await?;
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
    }

    async fn run_private_panels(&self) -> AppResult<()> {
        if self.connection.status().await != ConnectionStatus::Connected {
            return Ok(());
        }
        if self.ws.is_private_healthy(PRIVATE_STALE_MS) {
            return Ok(());
        }
        let symbol = self.market.active_symbol().await;
        let sym = Some(symbol.as_str());
        let open_orders = self.trading.fetch_open_orders(sym).await?;
        let order_history = self
            .trading
            .fetch_order_history(sym, Some(50))
            .await?;
        let params = build_order_query_params(
            sym, None, None, None, None, None, None, None, None, None,
        );
        let payload = self
            .api
            .private_get(endpoints::POSITIONS, params)
            .await?;
        let meta = list_envelope_meta(&payload);
        let positions = parse_positions(&payload);
        warn_if_parse_empty(&self.emitter, "position/list", &payload, positions.len());
        warn_if_raw_parsed_mismatch(&self.emitter, "position/list", &meta, positions.len());
        self.emitter.emit_private_panels_snapshot(PrivatePanelsSnapshot {
            open_orders,
            order_history,
            positions,
        });
        Ok(())
    }

    async fn run_market_fallback(&self) -> AppResult<()> {
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
        if self.ws.is_public_healthy(PUBLIC_STALE_MS) {
            return Ok(());
        }
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
    }

    async fn run_environment(&self) -> AppResult<()> {
        let active_account_id = {
            let config = self.config.read().await;
            normalize_account_id(&config.active_account_id)
        };
        let credential = CredentialStore::load(&active_account_id)?;
        let base_url = if let Some(credential) = credential.as_ref() {
            credential.base_url.clone()
        } else {
            self.api.base_url().await
        };
        let base_url = if base_url.trim().is_empty() {
            crate::models::config::DEFAULT_BASE_URL.to_string()
        } else {
            base_url
        };
        let checked_at = self.time.local_now_ms();
        let status = match crate::api::PublicApi::server_time(&self.api).await {
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
        self.emitter.emit_environment_updated(&status);
        *self.environment_status.write().await = status;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_id_parses_frontend_names() {
        assert_eq!(TaskId::from_name("dailyPnl"), Some(TaskId::DailyPnl));
        assert_eq!(TaskId::from_name("account"), Some(TaskId::Balances));
        assert_eq!(TaskId::from_name("market"), Some(TaskId::MarketFallback));
    }
}
