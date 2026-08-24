use std::sync::{Arc, Mutex};

use crate::models::config::AppConfig;
use crate::models::notification::{NotificationEnvironment, NotificationKind};
use crate::models::trading::SessionContext;
use crate::services::connection::SessionNotificationObserver;
use crate::services::notification::{
    NotificationEmitter, NotificationRuntime, NotificationService,
};
use crate::storage::notification_store::{NotificationFileV1, NotificationPersistence};

use super::super::{confirmed_environment_status, EnvironmentProbeSnapshot};

const NOW: u64 = 1_700_000_000_000;
const ENVIRONMENT_KEY: &str =
    "env-v1-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Default)]
struct MemoryPersistence(Mutex<Vec<NotificationFileV1>>);

impl NotificationPersistence for MemoryPersistence {
    fn save(&self, file: &NotificationFileV1) -> crate::error::AppResult<()> {
        self.0.lock().unwrap().push(file.clone());
        Ok(())
    }
}

struct Harness {
    observer: SessionNotificationObserver,
    persistence: Arc<MemoryPersistence>,
    config: Arc<tokio::sync::RwLock<AppConfig>>,
    lifecycle: Arc<crate::services::AccountLifecycleCoordinator>,
}

fn harness() -> Harness {
    let persistence = Arc::new(MemoryPersistence::default());
    let emitter: NotificationEmitter = Arc::new(|_| Ok(()));
    let service = Arc::new(NotificationService::from_snapshot(
        NotificationFileV1::empty(),
        Arc::clone(&persistence),
        emitter,
        NOW,
    ));
    let mut app_config = AppConfig::default();
    app_config.active_account_id = "alpha".into();
    let config = Arc::new(tokio::sync::RwLock::new(app_config));
    let lifecycle = Arc::new(crate::services::AccountLifecycleCoordinator::new());
    let observer = SessionNotificationObserver::new(
        Arc::new(NotificationRuntime::Available(service)),
        Arc::clone(&config),
        Arc::clone(&lifecycle),
    );
    Harness {
        observer,
        persistence,
        config,
        lifecycle,
    }
}

fn context(account_id: &str, session_epoch: u64) -> SessionContext {
    SessionContext {
        account_id: account_id.into(),
        session_epoch,
    }
}

fn records(harness: &Harness) -> Vec<crate::models::notification::NotificationRecord> {
    harness
        .persistence
        .0
        .lock()
        .unwrap()
        .last()
        .cloned()
        .unwrap_or_else(NotificationFileV1::empty)
        .partitions
        .into_iter()
        .flat_map(|partition| partition.items)
        .collect()
}

#[tokio::test]
async fn repeated_unavailable_recovered_environment_cycles_are_deterministic() {
    let harness = harness();
    let context = context("alpha", 0);

    assert!(harness
        .observer
        .observe_environment(
            &context,
            ENVIRONMENT_KEY,
            NotificationEnvironment::Production,
            false,
            NOW,
        )
        .await
        .is_some());
    assert!(harness
        .observer
        .observe_environment(
            &context,
            ENVIRONMENT_KEY,
            NotificationEnvironment::Production,
            false,
            NOW + 1,
        )
        .await
        .is_none());
    assert!(harness
        .observer
        .observe_environment(
            &context,
            ENVIRONMENT_KEY,
            NotificationEnvironment::Production,
            true,
            NOW + 2,
        )
        .await
        .is_some());
    assert!(harness
        .observer
        .observe_environment(
            &context,
            ENVIRONMENT_KEY,
            NotificationEnvironment::Production,
            true,
            NOW + 3,
        )
        .await
        .is_none());
    assert!(harness
        .observer
        .observe_environment(
            &context,
            ENVIRONMENT_KEY,
            NotificationEnvironment::Production,
            false,
            NOW + 4,
        )
        .await
        .is_some());

    let records = records(&harness);
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].kind, NotificationKind::EnvironmentUnavailable);
    assert_eq!(records[1].kind, NotificationKind::EnvironmentRecovered);
    assert_eq!(records[2].kind, NotificationKind::EnvironmentUnavailable);
    assert_ne!(
        records[0].dedupe_key.split(':').nth(2),
        records[2].dedupe_key.split(':').nth(2)
    );
}

#[tokio::test]
async fn late_wrong_account_and_old_epoch_probe_completions_are_noops() {
    let harness = harness();
    let old = context("alpha", 0);
    harness.lifecycle.advance_session_epoch();
    harness.config.write().await.active_account_id = "beta".into();

    for context in [old, context("alpha", 1), context("beta", 0)] {
        assert!(harness
            .observer
            .observe_environment(
                &context,
                ENVIRONMENT_KEY,
                NotificationEnvironment::Development,
                false,
                NOW,
            )
            .await
            .is_none());
    }
    assert!(harness.persistence.0.lock().unwrap().is_empty());
}

#[test]
fn probe_snapshot_normalizes_a_safe_environment_identity_and_status() {
    const PRIVATE: &str = "raw-password-token";
    let snapshot = EnvironmentProbeSnapshot::new(
        context("alpha", 7),
        &format!("https://user:{PRIVATE}@api.easicoin.io/private?token={PRIVATE}#secret"),
    );

    assert_eq!(snapshot.environment, NotificationEnvironment::Unknown);
    assert_eq!(snapshot.safe_base_url, "invalid-environment");
    assert_eq!(snapshot.label, "未知");
    assert!(!snapshot.safe_base_url.contains(PRIVATE));

    let status = confirmed_environment_status(&snapshot, false, NOW);
    let serialized = serde_json::to_string(&status).unwrap();
    assert_eq!(status.error.as_deref(), Some("环境不可达"));
    assert!(!serialized.contains(PRIVATE));
    assert!(!serialized.contains("token="));
    assert!(!serialized.contains("/private"));
}

#[tokio::test]
async fn environment_notifications_never_persist_raw_url_query_credentials_or_probe_error() {
    const PRIVATE: &str = "probe-error apiKey=raw-key https://secret.example/?token=raw";
    let harness = harness();
    let snapshot = EnvironmentProbeSnapshot::new(
        context("alpha", 0),
        "https://user:password@sandbox.example.test/path?token=raw",
    );
    let status = confirmed_environment_status(&snapshot, false, NOW);
    assert_ne!(status.error.as_deref(), Some(PRIVATE));

    harness
        .observer
        .observe_environment(
            &snapshot.context,
            &snapshot.environment_key,
            snapshot.environment,
            false,
            NOW,
        )
        .await;

    let serialized = serde_json::to_string(&records(&harness)).unwrap();
    for forbidden in [
        PRIVATE,
        "sandbox.example.test",
        "user:password",
        "token=raw",
        "/path",
    ] {
        assert!(!serialized.contains(forbidden), "leaked {forbidden}");
    }
}

#[test]
fn canonical_environment_identity_normalizes_defaults_without_colliding_custom_urls() {
    let context = context("alpha", 0);
    let canonical =
        EnvironmentProbeSnapshot::new(context.clone(), "https://CUSTOM.example.test:443/api/");
    let equivalent =
        EnvironmentProbeSnapshot::new(context.clone(), "https://custom.example.test/api");
    let different =
        EnvironmentProbeSnapshot::new(context.clone(), "https://custom.example.test/other");
    let insecure_official = EnvironmentProbeSnapshot::new(context, "http://api.easicoin.io");

    assert_eq!(canonical.environment_key, equivalent.environment_key);
    assert_ne!(canonical.environment_key, different.environment_key);
    assert_eq!(canonical.safe_base_url, "custom-environment");
    assert_eq!(canonical.environment, NotificationEnvironment::Development);
    assert_eq!(
        insecure_official.environment,
        NotificationEnvironment::Development
    );
}

#[tokio::test]
async fn distinct_custom_environment_incidents_recover_independently() {
    let harness = harness();
    let context = context("alpha", 0);
    let first = EnvironmentProbeSnapshot::new(context.clone(), "https://first.custom.example/api");
    let second =
        EnvironmentProbeSnapshot::new(context.clone(), "https://second.custom.example/api");

    for snapshot in [&first, &second] {
        assert!(harness
            .observer
            .observe_environment(
                &context,
                &snapshot.environment_key,
                snapshot.environment,
                false,
                NOW,
            )
            .await
            .is_some());
    }
    assert!(harness
        .observer
        .observe_environment(
            &context,
            &first.environment_key,
            first.environment,
            true,
            NOW + 1,
        )
        .await
        .is_some());
    assert!(harness
        .observer
        .observe_environment(
            &context,
            &second.environment_key,
            second.environment,
            false,
            NOW + 2,
        )
        .await
        .is_none());
    assert!(harness
        .observer
        .observe_environment(
            &context,
            &second.environment_key,
            second.environment,
            true,
            NOW + 3,
        )
        .await
        .is_some());

    let serialized = serde_json::to_string(&records(&harness)).unwrap();
    assert!(!serialized.contains("first.custom.example"));
    assert!(!serialized.contains("second.custom.example"));
}
