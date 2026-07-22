use crate::error::{AppError, AppResult};
use crate::models::account::AccountSwitchResult;
use crate::models::config::{normalize_account_id, ApiCredential, AppConfig, ConnectionStatus};

use super::{
    normalize_account_ids, safe_load, valid_credential, AccountLifecycleCoordinator,
    AccountLifecyclePort,
};

pub(crate) async fn switch_account<P: AccountLifecyclePort>(
    coordinator: &AccountLifecycleCoordinator,
    port: &P,
    account_id: &str,
    start_realtime: Option<bool>,
) -> AppResult<AccountSwitchResult> {
    let _guard = coordinator.mutation_guard().await;
    let former_config = port.read_config().await;
    let former_id = normalize_account_id(&former_config.active_account_id);
    let target_id = normalize_account_id(account_id);
    let former_status = port.connection_status().await;
    if target_id == former_id {
        return Ok(AccountSwitchResult {
            active_account_id: former_id,
            connected: former_status == ConnectionStatus::Connected,
        });
    }
    let accounts = normalize_account_ids(&former_config.accounts, &former_id);
    if !accounts.contains(&target_id) {
        return Err(AppError::Config("Account profile does not exist".into()));
    }
    let target_credential = valid_credential(safe_load(port, &target_id)?)?;
    port.preflight(&target_credential).await?;
    let former_credential = if former_status == ConnectionStatus::Connected {
        Some(valid_credential(safe_load(port, &former_id)?)?)
    } else {
        None
    };
    port.disconnect().await;

    let mut target_config = former_config.clone();
    target_config.active_account_id = target_id.clone();
    if let Err(primary) = port.persist_config(&target_config) {
        return Err(rollback_switch(
            port,
            &former_config,
            former_status,
            former_credential,
            primary,
        )
        .await);
    }
    port.replace_runtime_config(target_config).await;

    if former_status == ConnectionStatus::Connected {
        let realtime = start_realtime.unwrap_or(former_config.use_websocket);
        if let Err(primary) = port.connect(&target_id, realtime, target_credential).await {
            return Err(rollback_switch(
                port,
                &former_config,
                former_status,
                former_credential,
                primary,
            )
            .await);
        }
    }
    Ok(AccountSwitchResult {
        active_account_id: target_id,
        connected: former_status == ConnectionStatus::Connected,
    })
}

async fn rollback_switch<P: AccountLifecyclePort>(
    port: &P,
    former_config: &AppConfig,
    former_status: ConnectionStatus,
    former_credential: Option<ApiCredential>,
    primary: AppError,
) -> AppError {
    let mut rollback_errors = Vec::new();
    if let Err(error) = port.persist_config(former_config) {
        rollback_errors.push(error.to_string());
    }
    port.replace_runtime_config(former_config.clone()).await;
    if former_status == ConnectionStatus::Connected {
        if let Some(credential) = former_credential {
            if let Err(error) = port
                .connect(
                    &normalize_account_id(&former_config.active_account_id),
                    former_config.use_websocket,
                    credential,
                )
                .await
            {
                rollback_errors.push(error.to_string());
            }
        }
    }
    if rollback_errors.is_empty() {
        primary
    } else {
        AppError::Internal(format!(
            "{}; rollback failed: {}",
            primary,
            rollback_errors.join("; ")
        ))
    }
}
