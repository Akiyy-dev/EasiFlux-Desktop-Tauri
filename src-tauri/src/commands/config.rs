use tauri::State;

use crate::error::AppResult;
use crate::models::config::{normalize_account_id, AppConfig, RiskConfig, SaveCredentialRequest};
use crate::services::risk::validate_risk_config;
use crate::state::AppState;
use crate::storage::CredentialStore;

mod general_settings;
pub use general_settings::update_general_settings;

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> AppResult<AppConfig> {
    Ok(state.config.read().await.clone())
}

fn merge_lifecycle_config(mut incoming: AppConfig, current: &AppConfig) -> AppConfig {
    incoming.active_account_id = current.active_account_id.clone();
    incoming.accounts = current.accounts.clone();
    incoming
}

fn merge_authoritative_config(incoming: AppConfig, current: &AppConfig) -> AppConfig {
    let mut merged = merge_lifecycle_config(incoming, current);
    merged.risk_enabled = current.risk_enabled;
    merged.risk_max_order_qty = current.risk_max_order_qty.clone();
    merged.risk_max_price_deviation_pct = current.risk_max_price_deviation_pct.clone();
    merged.risk_max_daily_orders = current.risk_max_daily_orders;
    merged.trading_day_timezone = current.trading_day_timezone.clone();
    merged
}

fn prepare_config_save(incoming: AppConfig, current: &AppConfig) -> AppResult<(AppConfig, bool)> {
    validate_risk_config(&RiskConfig::from(&incoming))?;
    let merged = merge_authoritative_config(incoming, current);
    validate_risk_config(&RiskConfig::from(&merged))?;
    let timezone_changed = current.trading_day_timezone != merged.trading_day_timezone;
    Ok((merged, timezone_changed))
}

#[tauri::command]
pub async fn save_config(state: State<'_, AppState>, config: AppConfig) -> AppResult<AppConfig> {
    let (merged, timezone_changed) =
        crate::services::account_profiles::run_serialized_account_mutation(
            state.account_lifecycle.as_ref(),
            || async {
                let current = state.config.read().await.clone();
                let (merged, timezone_changed) = prepare_config_save(config, &current)?;
                state.config_store.save(&merged)?;
                *state.config.write().await = merged.clone();
                state
                    .risk
                    .write()
                    .await
                    .update_config(RiskConfig::from(&merged));
                Ok::<_, crate::error::AppError>((merged, timezone_changed))
            },
        )
        .await?;
    if timezone_changed {
        let _ = state
            .scheduler
            .run_now(crate::services::scheduler::TaskId::DailyPnl, true)
            .await;
    }
    Ok(merged)
}

#[tauri::command]
pub async fn save_credentials(
    state: State<'_, AppState>,
    request: SaveCredentialRequest,
) -> AppResult<()> {
    crate::commands::account_profiles::save_credentials_transaction(&state, request).await
}

#[tauri::command]
pub async fn has_credentials(account_id: String) -> AppResult<bool> {
    Ok(CredentialStore::has(&normalize_account_id(&account_id)))
}

#[tauri::command]
pub async fn save_window_size(
    state: State<'_, AppState>,
    width: u32,
    height: u32,
) -> AppResult<()> {
    crate::services::account_profiles::run_serialized_account_mutation(
        state.account_lifecycle.as_ref(),
        || async {
            let mut config = state.config.read().await.clone();
            config.window_width = width;
            config.window_height = height;
            state.config_store.save(&config)?;
            *state.config.write().await = config;
            Ok(())
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{merge_lifecycle_config, prepare_config_save};
    use crate::models::config::AppConfig;

    #[test]
    fn stale_settings_save_preserves_current_account_lifecycle_fields() {
        let mut current = AppConfig::default();
        current.active_account_id = "backup".into();
        current.accounts = vec!["primary".into(), "backup".into()];

        let mut stale = current.clone();
        stale.active_account_id = "primary".into();
        stale.accounts = vec!["primary".into()];
        stale.window_width = 1440;

        let merged = merge_lifecycle_config(stale, &current);

        assert_eq!(merged.active_account_id, "backup");
        assert_eq!(merged.accounts, vec!["primary", "backup"]);
        assert_eq!(merged.window_width, 1440);
    }

    #[test]
    fn stale_settings_save_preserves_current_authoritative_risk_fields() {
        let mut current = AppConfig::default();
        current.risk_enabled = false;
        current.risk_max_order_qty = "25.5".into();
        current.risk_max_price_deviation_pct = "2.5".into();
        current.risk_max_daily_orders = 20;
        current.trading_day_timezone = "UTC".into();

        let mut stale = AppConfig::default();
        stale.risk_enabled = true;
        stale.risk_max_order_qty = "10".into();
        stale.risk_max_price_deviation_pct = "5".into();
        stale.risk_max_daily_orders = 100;
        stale.trading_day_timezone = "Asia/Shanghai".into();
        stale.window_width = 1440;

        let (merged, timezone_changed) = prepare_config_save(stale, &current).unwrap();

        assert!(!merged.risk_enabled);
        assert_eq!(merged.risk_max_order_qty, "25.5");
        assert_eq!(merged.risk_max_price_deviation_pct, "2.5");
        assert_eq!(merged.risk_max_daily_orders, 20);
        assert_eq!(merged.trading_day_timezone, "UTC");
        assert_eq!(merged.window_width, 1440);
        assert!(!timezone_changed);
    }

    #[test]
    fn save_config_cannot_bypass_strict_risk_validation() {
        let current = AppConfig::default();
        let mut invalid = current.clone();
        invalid.risk_max_order_qty = "0".into();

        assert!(prepare_config_save(invalid, &current).is_err());
    }
}
