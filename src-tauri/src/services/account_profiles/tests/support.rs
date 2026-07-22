use std::collections::HashMap;
use std::sync::{Arc, Barrier, Mutex};

use crate::error::{AppError, AppResult};
use crate::models::config::{ApiCredential, AppConfig, ConnectionStatus, DEFAULT_BASE_URL};

use super::super::{AccountLifecyclePort, AccountProfileListPort, CredentialRepository};

#[derive(Default)]
pub(super) struct FailurePlan {
    pub preflight: bool,
    pub target_connect: bool,
    pub persist_for: Option<String>,
    pub restore_persist: bool,
    pub credential_load_for: Option<String>,
}

pub(super) struct FakeLifecyclePort {
    pub runtime: Mutex<AppConfig>,
    pub persisted: Mutex<AppConfig>,
    pub credentials: Mutex<HashMap<String, ApiCredential>>,
    pub status: Mutex<ConnectionStatus>,
    pub events: Mutex<Vec<String>>,
    pub failures: Mutex<FailurePlan>,
    pub delay_effects: bool,
    profile_load_sync: Mutex<Option<(String, Arc<Barrier>, Arc<Barrier>)>>,
}

impl FakeLifecyclePort {
    pub fn new(status: ConnectionStatus) -> Self {
        let mut config = AppConfig::default();
        config.active_account_id = "primary".into();
        config.accounts = vec!["primary".into(), "backup".into(), "spare".into()];
        let credentials = ["primary", "backup", "spare"]
            .into_iter()
            .map(|id| (id.into(), credential(id)))
            .collect();
        Self {
            runtime: Mutex::new(config.clone()),
            persisted: Mutex::new(config),
            credentials: Mutex::new(credentials),
            status: Mutex::new(status),
            events: Mutex::new(Vec::new()),
            failures: Mutex::new(FailurePlan::default()),
            delay_effects: false,
            profile_load_sync: Mutex::new(None),
        }
    }

    pub fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    pub fn runtime_config(&self) -> AppConfig {
        self.runtime.lock().unwrap().clone()
    }

    pub fn persisted_config(&self) -> AppConfig {
        self.persisted.lock().unwrap().clone()
    }

    pub fn delay_profile_load(
        &self,
        account_id: &str,
        started: Arc<Barrier>,
        release: Arc<Barrier>,
    ) {
        *self.profile_load_sync.lock().unwrap() = Some((account_id.into(), started, release));
    }
}

fn credential(label: &str) -> ApiCredential {
    ApiCredential {
        api_key: format!("{label}-key"),
        api_secret: format!("{label}-secret"),
        base_url: DEFAULT_BASE_URL.into(),
        label: label.into(),
    }
}

impl CredentialRepository for FakeLifecyclePort {
    fn load(&self, account_id: &str) -> AppResult<Option<ApiCredential>> {
        let synchronization = self.profile_load_sync.lock().unwrap().clone();
        if let Some((delayed_id, started, release)) = synchronization {
            if delayed_id == account_id {
                started.wait();
                release.wait();
            }
        }
        self.load_credential(account_id)
    }

    fn save(&self, account_id: &str, credential: &ApiCredential) -> AppResult<()> {
        self.save_credential(account_id, credential)
    }

    fn delete(&self, account_id: &str) -> AppResult<()> {
        self.delete_credential(account_id)
    }
}

impl AccountProfileListPort for FakeLifecyclePort {
    async fn read_profile_config(&self) -> AppConfig {
        self.runtime_config()
    }
}

impl AccountLifecyclePort for FakeLifecyclePort {
    async fn read_config(&self) -> AppConfig {
        self.runtime_config()
    }

    async fn replace_runtime_config(&self, config: AppConfig) {
        *self.runtime.lock().unwrap() = config;
    }

    fn persist_config(&self, config: &AppConfig) -> AppResult<()> {
        self.events
            .lock()
            .unwrap()
            .push(format!("persist:{}", config.active_account_id));
        let failures = self.failures.lock().unwrap();
        if failures.persist_for.as_deref() == Some(&config.active_account_id)
            || (failures.restore_persist && config.active_account_id == "primary")
        {
            return Err(AppError::Config(format!(
                "persist {} failed",
                config.active_account_id
            )));
        }
        drop(failures);
        *self.persisted.lock().unwrap() = config.clone();
        Ok(())
    }

    fn load_credential(&self, account_id: &str) -> AppResult<Option<ApiCredential>> {
        if self.failures.lock().unwrap().credential_load_for.as_deref() == Some(account_id) {
            return Err(AppError::Config("raw keyring detail".into()));
        }
        Ok(self.credentials.lock().unwrap().get(account_id).cloned())
    }

    fn save_credential(&self, account_id: &str, value: &ApiCredential) -> AppResult<()> {
        self.events
            .lock()
            .unwrap()
            .push(format!("save:{account_id}"));
        self.credentials
            .lock()
            .unwrap()
            .insert(account_id.into(), value.clone());
        Ok(())
    }

    fn delete_credential(&self, account_id: &str) -> AppResult<()> {
        self.events
            .lock()
            .unwrap()
            .push(format!("delete:{account_id}"));
        self.credentials.lock().unwrap().remove(account_id);
        Ok(())
    }

    async fn connection_status(&self) -> ConnectionStatus {
        *self.status.lock().unwrap()
    }

    async fn preflight(&self, value: &ApiCredential) -> AppResult<()> {
        self.events
            .lock()
            .unwrap()
            .push(format!("preflight:{}", value.label));
        if self.delay_effects {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        if self.failures.lock().unwrap().preflight {
            return Err(AppError::Connection("preflight failed".into()));
        }
        Ok(())
    }

    async fn disconnect(&self) {
        self.events.lock().unwrap().push("disconnect".into());
        *self.status.lock().unwrap() = ConnectionStatus::Disconnected;
    }

    async fn connect(
        &self,
        account_id: &str,
        _realtime: bool,
        _credential: ApiCredential,
    ) -> AppResult<()> {
        self.events
            .lock()
            .unwrap()
            .push(format!("connect:{account_id}"));
        if self.failures.lock().unwrap().target_connect && account_id == "backup" {
            return Err(AppError::Connection("target connection failed".into()));
        }
        *self.status.lock().unwrap() = ConnectionStatus::Connected;
        Ok(())
    }
}
