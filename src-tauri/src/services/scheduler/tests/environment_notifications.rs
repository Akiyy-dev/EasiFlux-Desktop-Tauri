use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::api::ApiClient;
use crate::events::EventEmitter;
use crate::models::config::{ApiCredential, AppConfig, EnvironmentStatus};
use crate::models::notification::{NotificationEnvironment, NotificationKind};
use crate::models::trading::SessionContext;
use crate::services::account_profiles::CredentialRepository;
use crate::services::connection::SessionNotificationObserver;
use crate::services::notification::{
    NotificationAvailability, NotificationEmitter, NotificationRuntime, NotificationService,
};
use crate::services::TimeService;
use crate::storage::notification_store::{NotificationFileV1, NotificationPersistence};

use super::super::{confirmed_environment_status, probe_environment, EnvironmentProbeSnapshot};

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

#[derive(Default)]
struct ProbePersistence {
    files: Mutex<Vec<NotificationFileV1>>,
    fail: AtomicBool,
}

impl NotificationPersistence for ProbePersistence {
    fn save(&self, file: &NotificationFileV1) -> crate::error::AppResult<()> {
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(crate::error::AppError::Storage(
                "raw persistence token=private".into(),
            ));
        }
        self.files.lock().unwrap().push(file.clone());
        Ok(())
    }
}

#[derive(Default)]
struct MemoryCredentialRepository(Mutex<HashMap<String, ApiCredential>>);

impl MemoryCredentialRepository {
    fn set_base_url(&self, account_id: &str, base_url: impl Into<String>) {
        self.0.lock().unwrap().insert(
            account_id.into(),
            ApiCredential {
                api_key: "test-key".into(),
                api_secret: "test-secret".into(),
                base_url: base_url.into(),
                label: "test".into(),
            },
        );
    }
}

impl CredentialRepository for MemoryCredentialRepository {
    fn load(&self, account_id: &str) -> crate::error::AppResult<Option<ApiCredential>> {
        Ok(self.0.lock().unwrap().get(account_id).cloned())
    }

    fn save(&self, account_id: &str, credential: &ApiCredential) -> crate::error::AppResult<()> {
        self.0
            .lock()
            .unwrap()
            .insert(account_id.into(), credential.clone());
        Ok(())
    }

    fn delete(&self, account_id: &str) -> crate::error::AppResult<()> {
        self.0.lock().unwrap().remove(account_id);
        Ok(())
    }
}

enum ProbeReply {
    Success,
    Unavailable,
    DelayedSuccess {
        started: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    },
}

fn local_probe_server(
    replies: Vec<ProbeReply>,
) -> (String, Arc<AtomicUsize>, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe server");
    let address = listener.local_addr().expect("probe server address");
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    let server = std::thread::spawn(move || {
        for reply in replies {
            let (mut socket, _) = listener.accept().expect("accept environment probe");
            counted.fetch_add(1, Ordering::SeqCst);
            let mut request = [0_u8; 2048];
            let _ = socket.read(&mut request).expect("read environment probe");
            match reply {
                ProbeReply::Unavailable => write_probe_unavailable(&mut socket),
                ProbeReply::Success => write_probe_success(&mut socket),
                ProbeReply::DelayedSuccess { started, release } => {
                    started.send(()).expect("signal delayed environment probe");
                    release.recv().expect("release delayed environment probe");
                    write_probe_success(&mut socket);
                }
            }
        }
    });
    (format!("http://{address}"), hits, server)
}

fn write_probe_success(socket: &mut std::net::TcpStream) {
    let body = r#"{"code":0,"data":{"time":"1782850580"}}"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket
        .write_all(response.as_bytes())
        .expect("write environment probe response");
}

fn write_probe_unavailable(socket: &mut std::net::TcpStream) {
    let body = r#"{"code":90000000}"#;
    let response = format!(
        "HTTP/1.1 500 TEST\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket
        .write_all(response.as_bytes())
        .expect("write unavailable environment probe response");
}

struct ProbeHarness {
    config: Arc<tokio::sync::RwLock<AppConfig>>,
    lifecycle: Arc<crate::services::AccountLifecycleCoordinator>,
    credentials: Arc<MemoryCredentialRepository>,
    persistence: Arc<ProbePersistence>,
    status: Arc<tokio::sync::RwLock<EnvironmentStatus>>,
    events: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    emitter: EventEmitter,
    time: Arc<TimeService>,
    observer: SessionNotificationObserver,
}

impl ProbeHarness {
    fn new(runtime_available: bool) -> Self {
        let events = Arc::new(Mutex::new(Vec::new()));
        let emitter = EventEmitter::new_test(Arc::clone(&events));
        let persistence = Arc::new(ProbePersistence::default());
        let notification_emitter: NotificationEmitter = {
            let emitter = emitter.clone();
            Arc::new(move |event| emitter.emit_notification_changed(event))
        };
        let service = Arc::new(NotificationService::from_snapshot(
            NotificationFileV1::empty(),
            Arc::clone(&persistence),
            notification_emitter,
            NOW,
        ));
        let runtime = if runtime_available {
            NotificationRuntime::Available(service)
        } else {
            NotificationRuntime::Unavailable(NotificationAvailability::new(
                "NOTIFICATION_STORAGE_UNAVAILABLE",
                "通知存储不可用",
            ))
        };
        let mut app_config = AppConfig::default();
        app_config.active_account_id = "alpha".into();
        let config = Arc::new(tokio::sync::RwLock::new(app_config));
        let lifecycle = Arc::new(crate::services::AccountLifecycleCoordinator::new());
        let observer = SessionNotificationObserver::new(
            Arc::new(runtime),
            Arc::clone(&config),
            Arc::clone(&lifecycle),
        );
        let api = Arc::new(ApiClient::new());
        let time = Arc::new(TimeService::new(api.time_sync(), api, emitter.clone()));
        Self {
            config,
            lifecycle,
            credentials: Arc::new(MemoryCredentialRepository::default()),
            persistence,
            status: Arc::new(tokio::sync::RwLock::new(initial_environment_status())),
            events,
            emitter,
            time,
            observer,
        }
    }

    async fn probe(&self) -> crate::error::AppResult<()> {
        probe_environment(
            &self.config,
            &self.time,
            &self.emitter,
            &self.status,
            &self.lifecycle,
            &self.observer,
            self.credentials.as_ref(),
        )
        .await
    }

    fn records(&self) -> Vec<crate::models::notification::NotificationRecord> {
        self.persistence
            .files
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

    fn event_count(&self, name: &str) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(event, _)| event == name)
            .count()
    }
}

fn initial_environment_status() -> EnvironmentStatus {
    EnvironmentStatus {
        base_url: "initial".into(),
        label: "未知".into(),
        reachable: false,
        checked_at: 0,
        error: None,
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

async fn assert_production_probe_success_unavailable_repeat_and_recovery_share_one_incident() {
    let harness = ProbeHarness::new(true);
    let (base_url, hits, server) = local_probe_server(vec![
        ProbeReply::Success,
        ProbeReply::Unavailable,
        ProbeReply::Unavailable,
        ProbeReply::Success,
    ]);
    harness.credentials.set_base_url("alpha", &base_url);

    assert!(harness.probe().await.is_ok());
    assert!(matches!(
        harness.probe().await,
        Err(crate::error::AppError::Observed("环境检测失败"))
    ));
    assert!(matches!(
        harness.probe().await,
        Err(crate::error::AppError::Observed("环境检测失败"))
    ));
    assert!(harness.probe().await.is_ok());
    server.join().expect("probe server exits");

    assert_eq!(hits.load(Ordering::SeqCst), 4);
    assert_eq!(harness.event_count("environment:updated"), 4);
    assert_eq!(harness.event_count("notification:changed"), 2);
    assert_eq!(harness.event_count("error:occurred"), 0);
    let records = harness.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].kind, NotificationKind::EnvironmentUnavailable);
    assert_eq!(records[1].kind, NotificationKind::EnvironmentRecovered);
    assert_eq!(
        records[0].dedupe_key.split(':').nth(2),
        records[1].dedupe_key.split(':').nth(2)
    );
}

#[tokio::test]
async fn production_probe_success_unavailable_repeat_and_recovery_share_one_incident() {
    assert_production_probe_success_unavailable_repeat_and_recovery_share_one_incident().await;
}

#[tokio::test]
async fn production_probe_persistence_failure_and_unavailable_runtime_never_emit_generic_errors() {
    for runtime_available in [true, false] {
        let harness = ProbeHarness::new(runtime_available);
        if runtime_available {
            harness.persistence.fail.store(true, Ordering::SeqCst);
        }
        let (base_url, _, server) = local_probe_server(vec![ProbeReply::Unavailable]);
        harness.credentials.set_base_url("alpha", &base_url);

        assert!(matches!(
            harness.probe().await,
            Err(crate::error::AppError::Observed("环境检测失败"))
        ));
        server.join().expect("probe server exits");

        assert!(harness.records().is_empty());
        assert_eq!(harness.event_count("notification:changed"), 0);
        assert_eq!(harness.event_count("error:occurred"), 0);
        assert!(!super::super::bootstrap_failure_needs_generic_error(
            &crate::error::AppError::Observed("环境检测失败")
        ));
        assert!(!harness.status.read().await.reachable);
    }
}

#[tokio::test]
async fn production_probe_completion_after_account_switch_is_a_full_noop() {
    let harness = Arc::new(ProbeHarness::new(true));
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (base_url, _, server) = local_probe_server(vec![ProbeReply::DelayedSuccess {
        started: started_tx,
        release: release_rx,
    }]);
    harness.credentials.set_base_url("alpha", &base_url);
    let probing = {
        let harness = Arc::clone(&harness);
        tokio::spawn(async move { harness.probe().await })
    };
    tokio::task::spawn_blocking(move || started_rx.recv().expect("probe reached server"))
        .await
        .unwrap();

    let mutation = harness.lifecycle.mutation_guard().await;
    harness.config.write().await.active_account_id = "beta".into();
    harness.lifecycle.advance_session_epoch();
    harness
        .credentials
        .set_base_url("beta", "https://new.example.test");
    release_tx.send(()).expect("release stale probe");
    drop(mutation);

    assert!(probing.await.unwrap().is_ok());
    server.join().expect("probe server exits");
    assert_eq!(
        serde_json::to_value(&*harness.status.read().await).unwrap(),
        serde_json::to_value(initial_environment_status()).unwrap()
    );
    assert!(harness.records().is_empty());
    assert!(harness.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn production_probe_completion_after_same_account_url_change_is_a_full_noop() {
    let harness = Arc::new(ProbeHarness::new(true));
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (base_url, _, server) = local_probe_server(vec![ProbeReply::DelayedSuccess {
        started: started_tx,
        release: release_rx,
    }]);
    harness.credentials.set_base_url("alpha", &base_url);
    let probing = {
        let harness = Arc::clone(&harness);
        tokio::spawn(async move { harness.probe().await })
    };
    tokio::task::spawn_blocking(move || started_rx.recv().expect("probe reached server"))
        .await
        .unwrap();

    let mutation = harness.lifecycle.mutation_guard().await;
    harness
        .credentials
        .set_base_url("alpha", "https://replacement.example.test");
    release_tx.send(()).expect("release stale probe");
    drop(mutation);

    assert!(probing.await.unwrap().is_ok());
    server.join().expect("probe server exits");
    assert_eq!(
        serde_json::to_value(&*harness.status.read().await).unwrap(),
        serde_json::to_value(initial_environment_status()).unwrap()
    );
    assert!(harness.records().is_empty());
    assert!(harness.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn production_probe_keeps_two_custom_environment_incidents_independent() {
    let harness = ProbeHarness::new(true);
    let (first_url, _, first_server) =
        local_probe_server(vec![ProbeReply::Unavailable, ProbeReply::Success]);
    let (second_url, _, second_server) =
        local_probe_server(vec![ProbeReply::Unavailable, ProbeReply::Success]);

    for base_url in [&first_url, &second_url, &first_url, &second_url] {
        harness.credentials.set_base_url("alpha", base_url);
        let _ = harness.probe().await;
    }
    first_server.join().expect("first probe server exits");
    second_server.join().expect("second probe server exits");

    let records = harness.records();
    assert_eq!(records.len(), 4);
    assert_eq!(
        records
            .iter()
            .filter(|record| record.kind == NotificationKind::EnvironmentUnavailable)
            .count(),
        2
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.kind == NotificationKind::EnvironmentRecovered)
            .count(),
        2
    );
    let serialized = serde_json::to_string(&records).unwrap();
    assert!(!serialized.contains("127.0.0.1"));
    assert_eq!(harness.event_count("error:occurred"), 0);
}

#[tokio::test]
async fn production_probe_rejects_unsafe_legacy_url_without_network_or_raw_output() {
    let harness = ProbeHarness::new(true);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind sentinel listener");
    listener
        .set_nonblocking(true)
        .expect("make sentinel listener nonblocking");
    let address = listener.local_addr().unwrap();
    let private = "raw-user:raw-password";
    harness.credentials.set_base_url(
        "alpha",
        format!("http://{private}@{address}/private?token=raw-secret#fragment"),
    );

    assert!(matches!(
        harness.probe().await,
        Err(crate::error::AppError::Observed("环境检测失败"))
    ));
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    assert_eq!(harness.event_count("error:occurred"), 0);
    let serialized = serde_json::to_string(&harness.events.lock().unwrap().clone()).unwrap();
    let persisted = serde_json::to_string(&harness.records()).unwrap();
    for forbidden in [private, "raw-secret", "/private", "token="] {
        assert!(!serialized.contains(forbidden));
        assert!(!persisted.contains(forbidden));
    }
}
