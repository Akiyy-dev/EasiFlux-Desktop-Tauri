use crate::error::{AppError, AppResult};
use crate::models::config::{normalize_account_id, ApiCredential, SaveCredentialRequest};

use super::{
    normalize_account_ids, safe_load, valid_credential, AccountLifecycleCoordinator,
    AccountLifecyclePort,
};

pub(crate) async fn delete_account<P: AccountLifecyclePort>(
    coordinator: &AccountLifecycleCoordinator,
    port: &P,
    account_id: &str,
) -> AppResult<()> {
    let _guard = coordinator.mutation_guard().await;
    let config = port.read_config().await;
    let active = normalize_account_id(&config.active_account_id);
    let target = normalize_account_id(account_id);
    let accounts = normalize_account_ids(&config.accounts, &active);
    if target == active {
        return Err(AppError::Config("不能删除当前账户".into()));
    }
    if accounts.len() <= 1 {
        return Err(AppError::Config("不能删除唯一账户".into()));
    }
    if !accounts.contains(&target) {
        return Err(AppError::Config("账户配置不存在".into()));
    }
    let former_credential = safe_load(port, &target)?;
    port.delete_credential(&target)
        .map_err(|_| AppError::Auth("删除账户凭据失败".into()))?;
    let mut next = config.clone();
    next.accounts = accounts.into_iter().filter(|id| id != &target).collect();
    if let Err(primary) = port.persist_config(&next) {
        if let Some(credential) = former_credential {
            if port.save_credential(&target, &credential).is_err() {
                return Err(AppError::Internal(format!(
                    "{}；回滚失败：无法恢复账户凭据",
                    primary
                )));
            }
        }
        return Err(primary);
    }
    port.replace_runtime_config(next).await;
    Ok(())
}

pub(crate) async fn save_credentials<P: AccountLifecyclePort>(
    coordinator: &AccountLifecycleCoordinator,
    port: &P,
    request: SaveCredentialRequest,
) -> AppResult<()> {
    let _guard = coordinator.mutation_guard().await;
    let account_id = normalize_account_id(&request.account_id);
    let config = port.read_config().await;
    let configured_accounts = normalize_account_ids(&config.accounts, &config.active_account_id);
    let configured = configured_accounts.contains(&account_id);
    let existing = safe_load(port, &account_id)?;
    let api_key = request.api_key.trim();
    let api_secret = request.api_secret.trim();
    if api_key.is_empty() != api_secret.is_empty() {
        return Err(AppError::Auth("API 访问密钥和签名密钥必须同时填写".into()));
    }
    let mut credential = if api_key.is_empty() && configured {
        valid_credential(existing.clone())?
    } else if api_key.is_empty() {
        return Err(AppError::Auth(
            "新账户必须填写 API 访问密钥和签名密钥".into(),
        ));
    } else {
        ApiCredential {
            api_key: api_key.into(),
            api_secret: api_secret.into(),
            base_url: request.base_url.clone(),
            label: request.label.clone(),
        }
        .normalize()
    };
    credential.base_url = request.base_url;
    credential.label = request.label;
    credential = credential.normalize();
    if credential.label.is_empty() {
        credential.label = account_id.clone();
    }
    if !credential.is_valid() {
        return Err(AppError::Auth("API 服务地址无效".into()));
    }
    port.save_credential(&account_id, &credential)
        .map_err(|_| AppError::Auth("保存账户凭据失败".into()))?;

    let mut next = config.clone();
    next.accounts = configured_accounts;
    if !next.accounts.contains(&account_id) {
        next.accounts.push(account_id.clone());
    }
    if let Err(primary) = port.persist_config(&next) {
        let rollback_failed = match existing {
            Some(credential) => port.save_credential(&account_id, &credential).is_err(),
            None => port.delete_credential(&account_id).is_err(),
        };
        if rollback_failed {
            return Err(AppError::Internal(format!(
                "{}；回滚失败：无法恢复账户凭据",
                primary
            )));
        }
        return Err(primary);
    }
    port.replace_runtime_config(next).await;
    Ok(())
}
