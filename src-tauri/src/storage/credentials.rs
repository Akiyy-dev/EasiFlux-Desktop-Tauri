use crate::error::{AppError, AppResult};
use crate::models::config::{ApiCredential, KEYRING_SERVICE};

pub struct CredentialStore;

impl CredentialStore {
    pub fn save(account_id: &str, credential: &ApiCredential) -> AppResult<()> {
        if !credential.is_valid() {
            return Err(AppError::Auth("API Key 与 Secret 均不能为空".into()));
        }
        let json = serde_json::to_string(credential)
            .map_err(|e| AppError::Config(e.to_string()))?;
        let entry = keyring::Entry::new(KEYRING_SERVICE, account_id)?;
        entry.set_password(&json)?;
        Ok(())
    }

    pub fn load(account_id: &str) -> AppResult<Option<ApiCredential>> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account_id)?;
        match entry.get_password() {
            Ok(json) => {
                let cred: ApiCredential = serde_json::from_str(&json)
                    .map_err(|e| AppError::Config(e.to_string()))?;
                Ok(Some(cred.normalize()))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::from(e)),
        }
    }

    pub fn delete(account_id: &str) -> AppResult<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account_id)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::from(e)),
        }
    }

    pub fn has(account_id: &str) -> bool {
        Self::load(account_id)
            .ok()
            .flatten()
            .map(|cred| cred.is_valid())
            .unwrap_or(false)
    }
}
