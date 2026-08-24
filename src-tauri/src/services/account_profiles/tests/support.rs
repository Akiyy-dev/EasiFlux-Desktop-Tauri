use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};

use crate::error::{AppError, AppResult};
use crate::models::config::{ApiCredential, AppConfig, ConnectionStatus};

use super::super::{AccountLifecyclePort, AccountProfileListPort, CredentialRepository};

#[derive(Default)]
pub(super) struct FailurePlan {
    pub preflight: bool,
    pub target_connect: bool,
    pub former_connect: bool,
    pub former_connect_notified: Option<String>,
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
    analytics_clears: AtomicUsize,
    public_base_url: Mutex<String>,
    public_has_credential: AtomicBool,
    public_environment_activations: AtomicUsize,
    pub failures: Mutex<FailurePlan>,
    pub delay_effects: bool,
    pub delay_connect: bool,
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
            analytics_clears: AtomicUsize::new(0),
            public_base_url: Mutex::new(credential("primary").base_url),
            public_has_credential: AtomicBool::new(status == ConnectionStatus::Connected),
            public_environment_activations: AtomicUsize::new(0),
            failures: Mutex::new(FailurePlan::default()),
            delay_effects: false,
            delay_connect: false,
            profile_load_sync: Mutex::new(None),
        }
    }

    pub fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    pub fn runtime_config(&self) -> AppConfig {
        self.runtime.lock().unwrap().clone()
    }

    pub fn analytics_clear_count(&self) -> usize {
        self.analytics_clears.load(Ordering::SeqCst)
    }

    pub fn public_base_url(&self) -> String {
        self.public_base_url.lock().unwrap().clone()
    }

    pub fn public_has_credential(&self) -> bool {
        self.public_has_credential.load(Ordering::SeqCst)
    }

    pub fn public_environment_activation_count(&self) -> usize {
        self.public_environment_activations.load(Ordering::SeqCst)
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
        base_url: format!("https://{label}.example.test"),
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
        self.public_has_credential.store(false, Ordering::SeqCst);
    }

    async fn connect(
        &self,
        account_id: &str,
        _realtime: bool,
        credential: ApiCredential,
        _session_epoch: u64,
    ) -> AppResult<()> {
        self.events
            .lock()
            .unwrap()
            .push(format!("connect:{account_id}"));
        *self.public_base_url.lock().unwrap() = credential.normalize().base_url;
        self.public_has_credential.store(true, Ordering::SeqCst);
        if self.delay_connect {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let failures = self.failures.lock().unwrap();
        if failures.target_connect && account_id == "backup" {
            return Err(AppError::Connection("target connection failed".into()));
        }
        if failures.former_connect && account_id == "primary" {
            return Err(AppError::Connection("former connection failed".into()));
        }
        if account_id == "primary" {
            if let Some(notification_id) = failures.former_connect_notified.clone() {
                return Err(AppError::Notified {
                    code: "CONNECTION_UNAVAILABLE",
                    message: "交易连接暂时不可用",
                    notification_id,
                    cause: None,
                });
            }
        }
        drop(failures);
        *self.status.lock().unwrap() = ConnectionStatus::Connected;
        Ok(())
    }

    async fn clear_account_data(&self) {
        self.analytics_clears.fetch_add(1, Ordering::SeqCst);
    }

    async fn activate_public_environment(&self, credential: &ApiCredential) {
        *self.public_base_url.lock().unwrap() = credential.clone().normalize().base_url;
        self.public_has_credential.store(false, Ordering::SeqCst);
        self.public_environment_activations
            .fetch_add(1, Ordering::SeqCst);
    }

    async fn activate_session(&self, context: &crate::models::trading::SessionContext) {
        self.events.lock().unwrap().push(format!(
            "activate:{}:{}",
            context.account_id, context.session_epoch
        ));
    }
}
