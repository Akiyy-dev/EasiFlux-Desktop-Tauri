use std::collections::HashSet;

use crate::models::account::{AccountProfile, CredentialState};
use crate::models::config::{normalize_account_id, DEFAULT_BASE_URL};

use super::{AccountLifecycleCoordinator, AccountProfileListPort, CredentialRepository};

pub(crate) async fn list_account_profiles_transaction<P: AccountProfileListPort>(
    coordinator: &AccountLifecycleCoordinator,
    port: &P,
) -> Vec<AccountProfile> {
    let _guard = coordinator.read_guard().await;
    let config = port.read_profile_config().await;
    build_account_profiles(&config.accounts, &config.active_account_id, port)
}

pub(crate) fn normalize_account_ids(accounts: &[String], active_account_id: &str) -> Vec<String> {
    let active = normalize_account_id(active_account_id);
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for id in accounts
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
    {
        if seen.insert(id.to_string()) {
            normalized.push(id.to_string());
        }
    }
    if seen.insert(active.clone()) {
        normalized.insert(0, active);
    }
    normalized
}

pub(crate) fn build_account_profiles<R: CredentialRepository>(
    accounts: &[String],
    active_account_id: &str,
    repository: &R,
) -> Vec<AccountProfile> {
    let active = normalize_account_id(active_account_id);
    normalize_account_ids(accounts, &active)
        .into_iter()
        .map(|account_id| match repository.load(&account_id) {
            Ok(Some(credential)) => {
                let credential = credential.normalize();
                if credential.is_valid() {
                    AccountProfile {
                        label: if credential.label.is_empty() {
                            account_id.clone()
                        } else {
                            credential.label
                        },
                        base_url: credential.base_url,
                        credential_state: CredentialState::Present,
                        active: account_id == active,
                        account_id,
                    }
                } else {
                    unavailable_profile(account_id, &active)
                }
            }
            Ok(None) => AccountProfile {
                label: account_id.clone(),
                base_url: DEFAULT_BASE_URL.to_string(),
                credential_state: CredentialState::Missing,
                active: account_id == active,
                account_id,
            },
            Err(_) => unavailable_profile(account_id, &active),
        })
        .collect()
}

fn unavailable_profile(account_id: String, active: &str) -> AccountProfile {
    AccountProfile {
        label: account_id.clone(),
        base_url: DEFAULT_BASE_URL.to_string(),
        credential_state: CredentialState::Unavailable,
        active: account_id == active,
        account_id,
    }
}
