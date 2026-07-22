use std::sync::Arc;

use tokio::sync::RwLock;

use crate::error::AppResult;
use crate::models::config::AppConfig;
use crate::services::AccountLifecycleCoordinator;
use crate::storage::ConfigStore;

pub(crate) async fn persist_market_config_update<F, C, Fut>(
    coordinator: &AccountLifecycleCoordinator,
    config_store: &ConfigStore,
    runtime_config: &Arc<RwLock<AppConfig>>,
    mutate: F,
    after_commit: C,
) -> AppResult<()>
where
    F: FnOnce(&mut AppConfig),
    C: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    crate::services::account_profiles::run_serialized_account_mutation(coordinator, || async {
        let mut next = runtime_config.read().await.clone();
        mutate(&mut next);
        config_store.save(&next)?;
        *runtime_config.write().await = next;
        after_commit().await;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn after_commit_ws_mutations_are_serialized_with_market_config_updates() {
        let path = std::env::temp_dir().join(format!(
            "easiflux-market-config-ws-{}-{}.toml",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let store = ConfigStore::with_path(path.clone());
        let config = Arc::new(RwLock::new(AppConfig::default()));
        let coordinator = AccountLifecycleCoordinator::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let release_first = Arc::new(tokio::sync::Notify::new());

        let first_events = Arc::clone(&events);
        let first_release = Arc::clone(&release_first);
        let first = persist_market_config_update(
            &coordinator,
            &store,
            &config,
            |next| next.active_symbol = "ETHUSDT".into(),
            move || async move {
                first_events.lock().unwrap().push("ws:first:start");
                first_release.notified().await;
                first_events.lock().unwrap().push("ws:first:end");
            },
        );
        tokio::pin!(first);
        assert!(matches!(
            futures_util::poll!(&mut first),
            std::task::Poll::Pending
        ));

        let second_events = Arc::clone(&events);
        let second = persist_market_config_update(
            &coordinator,
            &store,
            &config,
            |next| next.kline_interval = "5".into(),
            move || async move {
                second_events.lock().unwrap().push("ws:second");
            },
        );
        tokio::pin!(second);
        assert!(matches!(
            futures_util::poll!(&mut second),
            std::task::Poll::Pending
        ));
        assert_eq!(*events.lock().unwrap(), ["ws:first:start"]);

        release_first.notify_one();
        let (first_result, second_result) = tokio::join!(first, second);
        first_result.unwrap();
        second_result.unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            ["ws:first:start", "ws:first:end", "ws:second"]
        );

        for candidate in [
            path.clone(),
            std::path::PathBuf::from(format!("{}.bak", path.display())),
            std::path::PathBuf::from(format!("{}.tmp", path.display())),
        ] {
            let _ = std::fs::remove_file(candidate);
        }
    }
}
