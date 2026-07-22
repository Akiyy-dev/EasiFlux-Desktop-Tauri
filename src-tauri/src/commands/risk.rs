use std::sync::Arc;

use tauri::State;
use tokio::sync::RwLock;

use crate::error::AppResult;
use crate::models::config::{AppConfig, RiskConfig};
use crate::models::risk::{RiskStatus, UpdateRiskConfigRequest};
use crate::services::risk::{validate_risk_config, RiskService};
use crate::services::scheduler::TaskId;
use crate::services::AccountLifecycleCoordinator;
use crate::state::AppState;
use crate::storage::ConfigStore;

fn normalize_request(request: UpdateRiskConfigRequest) -> RiskConfig {
    RiskConfig {
        enabled: request.enabled,
        max_order_qty: request.max_order_qty.trim().to_string(),
        max_price_deviation_pct: request.max_price_deviation_pct.trim().to_string(),
        max_daily_orders: request.max_daily_orders,
        trading_day_timezone: request.trading_day_timezone.trim().to_string(),
    }
}

async fn apply_risk_config_update<N>(
    config_store: &ConfigStore,
    runtime_config: &Arc<RwLock<AppConfig>>,
    risk: &Arc<RwLock<RiskService>>,
    request: UpdateRiskConfigRequest,
    now_ms: N,
) -> AppResult<(RiskStatus, bool)>
where
    N: FnOnce() -> u64,
{
    let risk_config = normalize_request(request);
    validate_risk_config(&risk_config)?;

    let current = runtime_config.read().await.clone();
    let timezone_changed = current.trading_day_timezone != risk_config.trading_day_timezone;
    let mut next = current;
    next.risk_enabled = risk_config.enabled;
    next.risk_max_order_qty = risk_config.max_order_qty.clone();
    next.risk_max_price_deviation_pct = risk_config.max_price_deviation_pct.clone();
    next.risk_max_daily_orders = risk_config.max_daily_orders;
    next.trading_day_timezone = risk_config.trading_day_timezone.clone();

    config_store.save(&next)?;
    *runtime_config.write().await = next;
    let mut risk = risk.write().await;
    risk.update_config(risk_config);
    Ok((risk.status(now_ms()), timezone_changed))
}

struct RiskUpdateContext<'a> {
    coordinator: &'a AccountLifecycleCoordinator,
    config_store: &'a ConfigStore,
    runtime_config: &'a Arc<RwLock<AppConfig>>,
    risk: &'a Arc<RwLock<RiskService>>,
}

async fn execute_risk_config_update<N, F, Fut>(
    context: RiskUpdateContext<'_>,
    request: UpdateRiskConfigRequest,
    now_ms: N,
    refresh_daily_pnl: F,
) -> AppResult<RiskStatus>
where
    N: FnOnce() -> u64,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let (status, timezone_changed) =
        crate::services::account_profiles::run_serialized_account_mutation(
            context.coordinator,
            || async {
                apply_risk_config_update(
                    context.config_store,
                    context.runtime_config,
                    context.risk,
                    request,
                    now_ms,
                )
                .await
            },
        )
        .await?;
    if timezone_changed {
        refresh_daily_pnl().await;
    }
    Ok(status)
}

#[tauri::command]
pub async fn get_risk_status(state: State<'_, AppState>) -> AppResult<RiskStatus> {
    Ok(state.risk.read().await.status(state.time.now_ms()))
}

#[tauri::command]
pub async fn update_risk_config(
    state: State<'_, AppState>,
    request: UpdateRiskConfigRequest,
) -> AppResult<RiskStatus> {
    execute_risk_config_update(
        RiskUpdateContext {
            coordinator: state.account_lifecycle.as_ref(),
            config_store: &state.config_store,
            runtime_config: &state.config,
            risk: &state.risk,
        },
        request,
        || state.time.now_ms(),
        || async {
            let _ = state.scheduler.run_now(TaskId::DailyPnl, true).await;
        },
    )
    .await
}

#[cfg(test)]
mod tests;
