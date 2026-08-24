use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::api::response::AuthFailureKind;
use crate::api::ApiClient;
use crate::models::config::{ApiCredential, AppConfig, ConnectionStatus};
use crate::models::notification::{
    NotificationChannel, NotificationKind, NotificationScalar, NotificationScope,
};
use crate::models::trading::SessionContext;
use crate::services::notification::{
    NotificationEmitter, NotificationRuntime, NotificationService,
};
use crate::storage::notification_store::{NotificationFileV1, NotificationPersistence};

use super::{
    deliver_notified_connection_error, notified_connection_error, ConnectionObservationSource,
    SessionNotificationObserver,
};

const NOW: u64 = 1_700_000_000_000;

#[derive(Default)]
struct MemoryPersistence {
    files: Mutex<Vec<NotificationFileV1>>,
    fail_next: AtomicBool,
}

impl NotificationPersistence for MemoryPersistence {
    fn save(&self, file: &NotificationFileV1) -> crate::error::AppResult<()> {
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(crate::error::AppError::Storage(
                "raw base=https://user:secret@example.test/?token=private".into(),
            ));
        }
        self.files.lock().unwrap().push(file.clone());
        Ok(())
    }
}

struct Harness {
    observer: SessionNotificationObserver,
    persistence: Arc<MemoryPersistence>,
    events: Arc<Mutex<Vec<crate::models::notification::NotificationChangedEvent>>>,
    diagnostics: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    config: Arc<tokio::sync::RwLock<AppConfig>>,
    lifecycle: Arc<crate::services::AccountLifecycleCoordinator>,
}

fn harness() -> Harness {
    harness_from_file(NotificationFileV1::empty())
}

fn harness_from_file(file: NotificationFileV1) -> Harness {
    let persistence = Arc::new(MemoryPersistence::default());
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&events);
    let emitter: NotificationEmitter = Arc::new(move |event| {
        captured.lock().unwrap().push(event.clone());
        Ok(())
    });
    let service = Arc::new(NotificationService::from_snapshot(
        file,
        Arc::clone(&persistence),
        emitter,
        NOW,
    ));
    let mut app_config = AppConfig::default();
    app_config.active_account_id = "alpha".into();
    let config = Arc::new(tokio::sync::RwLock::new(app_config));
    let lifecycle = Arc::new(crate::services::AccountLifecycleCoordinator::new());
    let diagnostics = Arc::new(Mutex::new(Vec::new()));
    let observer = SessionNotificationObserver::new(
        Arc::new(NotificationRuntime::Available(service)),
        Arc::clone(&config),
        Arc::clone(&lifecycle),
        crate::events::EventEmitter::new_test(Arc::clone(&diagnostics)),
    );
    Harness {
        observer,
        persistence,
        events,
        diagnostics,
        config,
        lifecycle,
    }
}

#[tokio::test]
async fn durable_session_expiry_commit_owns_one_log_only_diagnostic() {
    let harness = harness();
    let notification_id = harness
        .observer
        .observe_auth_failure(&context("alpha", 0), AuthFailureKind::SessionExpired, NOW)
        .await
        .expect("durable session notification returns its marker");

    assert_eq!(harness.persistence.files.lock().unwrap().len(), 1);
    assert_eq!(harness.events.lock().unwrap().len(), 1);
    let diagnostics = harness.diagnostics.lock().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].0, "log:entry");
    assert_eq!(
        diagnostics[0].1["message"],
        "NOTIFIED_SESSION_FAILURE:AUTH_SESSION_EXPIRED"
    );
    assert!(diagnostics.iter().all(|(name, _)| name != "error:occurred"));
    drop(diagnostics);

    let explicit_error = notified_connection_error(
        crate::error::AppError::AuthFailure(AuthFailureKind::SessionExpired),
        Some(notification_id),
    );
    let _ = deliver_notified_connection_error(
        &crate::events::EventEmitter::new_test(Arc::clone(&harness.diagnostics)),
        explicit_error,
    );
    assert_eq!(harness.diagnostics.lock().unwrap().len(), 1);
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

fn scalar_str(value: &NotificationScalar) -> Option<&str> {
    match value {
        NotificationScalar::String(value) => Some(value),
        NotificationScalar::Bool(_) | NotificationScalar::Number(_) => None,
    }
}

#[tokio::test]
async fn api_and_private_websocket_edges_publish_one_incident_and_matching_recovery_each() {
    let harness = harness();
    let context = context("alpha", 0);

    for source in [
        ConnectionObservationSource::Api,
        ConnectionObservationSource::PrivateWebsocket,
    ] {
        let unavailable = harness
            .observer
            .observe_connection_status(&context, source, ConnectionStatus::Error, NOW)
            .await;
        assert!(unavailable.is_some());
        assert!(harness
            .observer
            .observe_connection_status(&context, source, ConnectionStatus::Error, NOW + 1)
            .await
            .is_none());
        let recovered = harness
            .observer
            .observe_connection_status(&context, source, ConnectionStatus::Connected, NOW + 2)
            .await;
        assert!(recovered.is_some());
        assert!(harness
            .observer
            .observe_connection_status(&context, source, ConnectionStatus::Connected, NOW + 3)
            .await
            .is_none());
    }

    let records = records(&harness);
    assert_eq!(records.len(), 4);
    for channel in [NotificationChannel::Api, NotificationChannel::Websocket] {
        let unavailable = records
            .iter()
            .find(|record| {
                record.kind == NotificationKind::ConnectionUnavailable
                    && record.content.params.get("channel").and_then(scalar_str)
                        == Some(match channel {
                            NotificationChannel::Api => "api",
                            NotificationChannel::Websocket => "websocket",
                        })
            })
            .unwrap();
        let recovered = records
            .iter()
            .find(|record| {
                record.kind == NotificationKind::ConnectionRecovered
                    && record.content.params.get("channel").and_then(scalar_str)
                        == unavailable
                            .content
                            .params
                            .get("channel")
                            .and_then(scalar_str)
            })
            .unwrap();
        assert_eq!(
            unavailable.dedupe_key.split(':').nth(2),
            recovered.dedupe_key.split(':').nth(2)
        );
    }
}

#[tokio::test]
async fn connecting_user_disconnect_and_public_websocket_noise_are_suppressed() {
    let harness = harness();
    let context = context("alpha", 0);

    for (source, status) in [
        (
            ConnectionObservationSource::Api,
            ConnectionStatus::Connecting,
        ),
        (
            ConnectionObservationSource::Api,
            ConnectionStatus::Disconnected,
        ),
        (
            ConnectionObservationSource::PrivateWebsocket,
            ConnectionStatus::Connecting,
        ),
        (
            ConnectionObservationSource::PrivateWebsocket,
            ConnectionStatus::Disconnected,
        ),
        (
            ConnectionObservationSource::PublicWebsocket,
            ConnectionStatus::Error,
        ),
        (
            ConnectionObservationSource::PublicWebsocket,
            ConnectionStatus::Connected,
        ),
    ] {
        assert!(harness
            .observer
            .observe_connection_status(&context, source, status, NOW)
            .await
            .is_none());
    }

    assert!(harness.persistence.files.lock().unwrap().is_empty());
    assert!(harness.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn one_channel_recovery_does_not_close_the_other_channel_incident() {
    let harness = harness();
    let context = context("alpha", 0);

    harness
        .observer
        .observe_connection_status(
            &context,
            ConnectionObservationSource::Api,
            ConnectionStatus::Error,
            NOW,
        )
        .await;
    harness
        .observer
        .observe_connection_status(
            &context,
            ConnectionObservationSource::PrivateWebsocket,
            ConnectionStatus::Error,
            NOW + 1,
        )
        .await;
    harness
        .observer
        .observe_connection_status(
            &context,
            ConnectionObservationSource::Api,
            ConnectionStatus::Connected,
            NOW + 2,
        )
        .await;
    assert!(harness
        .observer
        .observe_connection_status(
            &context,
            ConnectionObservationSource::PrivateWebsocket,
            ConnectionStatus::Error,
            NOW + 3,
        )
        .await
        .is_none());
    assert!(harness
        .observer
        .observe_connection_status(
            &context,
            ConnectionObservationSource::PrivateWebsocket,
            ConnectionStatus::Connected,
            NOW + 4,
        )
        .await
        .is_some());
}

#[tokio::test]
async fn public_websocket_success_never_recovers_a_private_websocket_incident() {
    let harness = harness();
    let context = context("alpha", 0);
    assert!(harness
        .observer
        .observe_connection_status(
            &context,
            ConnectionObservationSource::PrivateWebsocket,
            ConnectionStatus::Error,
            NOW,
        )
        .await
        .is_some());
    assert!(harness
        .observer
        .observe_connection_status(
            &context,
            ConnectionObservationSource::PublicWebsocket,
            ConnectionStatus::Connected,
            NOW + 1,
        )
        .await
        .is_none());
    assert!(harness
        .observer
        .observe_connection_status(
            &context,
            ConnectionObservationSource::PrivateWebsocket,
            ConnectionStatus::Error,
            NOW + 2,
        )
        .await
        .is_none());
    assert_eq!(records(&harness).len(), 1);
}

#[tokio::test]
async fn old_epoch_and_wrong_account_connection_edges_do_not_mutate_notifications() {
    let harness = harness();
    harness.lifecycle.advance_session_epoch();

    for context in [context("alpha", 0), context("beta", 1)] {
        assert!(harness
            .observer
            .observe_connection_status(
                &context,
                ConnectionObservationSource::Api,
                ConnectionStatus::Error,
                NOW,
            )
            .await
            .is_none());
    }

    harness.config.write().await.active_account_id = "beta".into();
    assert!(harness
        .observer
        .observe_connection_status(
            &context("alpha", 1),
            ConnectionObservationSource::PrivateWebsocket,
            ConnectionStatus::Error,
            NOW + 1,
        )
        .await
        .is_none());
    assert!(harness.persistence.files.lock().unwrap().is_empty());
}

#[tokio::test]
async fn only_typed_session_expired_auth_failure_can_publish_a_session_notification() {
    let harness = harness();
    let context = context("alpha", 0);

    for failure in [
        AuthFailureKind::MissingCredential,
        AuthFailureKind::CredentialStorage,
        AuthFailureKind::SigningConfiguration,
        AuthFailureKind::Timestamp,
        AuthFailureKind::Signature,
        AuthFailureKind::AccessDenied,
        AuthFailureKind::RateLimited,
        AuthFailureKind::Other,
    ] {
        assert!(harness
            .observer
            .observe_auth_failure(&context, failure, NOW)
            .await
            .is_none());
    }
    let notification_id = harness
        .observer
        .observe_auth_failure(&context, AuthFailureKind::SessionExpired, NOW + 1)
        .await
        .expect("first session edge returns its durable marker");
    let replay_id = harness
        .observer
        .observe_auth_failure(&context, AuthFailureKind::SessionExpired, NOW + 2)
        .await
        .expect("source replay returns the same durable marker");
    assert_eq!(replay_id, notification_id);
    assert_eq!(
        records(&harness)[0].kind,
        NotificationKind::AccountSessionExpired
    );
    assert_eq!(harness.persistence.files.lock().unwrap().len(), 1);
    assert_eq!(harness.events.lock().unwrap().len(), 1);
    assert_eq!(harness.diagnostics.lock().unwrap().len(), 1);
    assert_eq!(
        records(&harness)[0].scope,
        NotificationScope::Account {
            account_id: "alpha".into()
        }
    );
}

#[tokio::test]
async fn private_api_session_expiry_commits_once_and_returns_only_the_real_record_id() {
    use std::io::{Read, Write};

    let harness = harness();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test listener");
    let address = listener.local_addr().expect("test listener address");
    let server = std::thread::spawn(move || {
        for body in [
            r#"{"code":0,"data":{"time":"1782850580"}}"#,
            r#"{"code":26200003,"message":"provider detail"}"#,
            r#"{"code":26200003,"message":"different provider detail"}"#,
        ] {
            let (mut socket, _) = listener.accept().expect("accept test request");
            let mut request = [0_u8; 2048];
            let _ = socket.read(&mut request).expect("read test request");
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket
                .write_all(response.as_bytes())
                .expect("write test response");
        }
    });

    let client = ApiClient::new();
    client
        .set_credential_for_session(
            ApiCredential {
                label: "Alpha".into(),
                api_key: "key".into(),
                api_secret: "secret".into(),
                base_url: format!("http://{address}"),
            },
            context("alpha", 0),
        )
        .await;
    let observer = harness.observer.clone();
    client.set_auth_failure_observer(Arc::new(move |context, failure| {
        let observer = observer.clone();
        Box::pin(async move { observer.observe_auth_failure(&context, failure, NOW).await })
    }));

    let first = client
        .private_get("/private/test", Vec::new())
        .await
        .expect_err("documented session expiry must fail");
    let committed_id = match first {
        crate::error::AppError::Notified {
            code,
            message,
            notification_id,
            cause,
        } => {
            assert_eq!(code, "AUTH_SESSION_EXPIRED");
            assert_eq!(message, "账户会话已失效");
            assert_eq!(
                cause,
                Some(crate::error::NotificationCause::AuthFailure(
                    AuthFailureKind::SessionExpired
                ))
            );
            notification_id
        }
        other => panic!("first expiry must return committed notification ID: {other:?}"),
    };
    let repeated = client
        .private_get("/private/test", Vec::new())
        .await
        .expect_err("repeated session expiry must fail");
    server.join().expect("test server exits");

    assert!(matches!(
        repeated,
        crate::error::AppError::Notified {
            code: "AUTH_SESSION_EXPIRED",
            notification_id,
            ..
        } if notification_id == committed_id
    ));
    assert_eq!(harness.persistence.files.lock().unwrap().len(), 1);
    assert_eq!(harness.events.lock().unwrap().len(), 1);
    let records = records(&harness);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, committed_id);
    assert_eq!(records[0].kind, NotificationKind::AccountSessionExpired);
    assert_eq!(harness.diagnostics.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn concurrent_and_restart_session_replay_keep_one_commit_and_one_diagnostic() {
    let first_run = harness();
    let context = context("alpha", 0);
    let (left, right) = tokio::join!(
        first_run
            .observer
            .observe_auth_failure(&context, AuthFailureKind::SessionExpired, NOW,),
        first_run
            .observer
            .observe_auth_failure(&context, AuthFailureKind::SessionExpired, NOW,),
    );
    let left = left.expect("first concurrent observation returns a marker");
    let right = right.expect("replayed concurrent observation returns a marker");
    assert_eq!(left, right);
    assert_eq!(first_run.persistence.files.lock().unwrap().len(), 1);
    assert_eq!(first_run.events.lock().unwrap().len(), 1);
    assert_eq!(first_run.diagnostics.lock().unwrap().len(), 1);

    let persisted = first_run.persistence.files.lock().unwrap()[0].clone();
    let restarted = harness_from_file(persisted);
    let restart_id = restarted
        .observer
        .observe_auth_failure(&context, AuthFailureKind::SessionExpired, NOW + 1)
        .await
        .expect("restart replay returns the persisted marker");
    assert_eq!(restart_id, left);
    assert!(restarted.persistence.files.lock().unwrap().is_empty());
    assert!(restarted.events.lock().unwrap().is_empty());
    assert!(restarted.diagnostics.lock().unwrap().is_empty());
}

#[tokio::test]
async fn guarded_post_commit_activation_recovers_incidents_while_mutation_lock_is_held() {
    let harness = harness();
    let context = context("alpha", 0);
    for source in [
        ConnectionObservationSource::Api,
        ConnectionObservationSource::PrivateWebsocket,
    ] {
        assert!(harness
            .observer
            .observe_connection_status(&context, source, ConnectionStatus::Error, NOW)
            .await
            .is_some());
    }

    let mutation = harness.lifecycle.mutation_guard().await;
    for source in [
        ConnectionObservationSource::Api,
        ConnectionObservationSource::PrivateWebsocket,
    ] {
        let recovery = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            harness.observer.observe_connection_status_guarded(
                &context,
                source,
                ConnectionStatus::Connected,
                NOW + 1,
            ),
        )
        .await
        .expect("guarded activation must not reacquire the lifecycle lock");
        assert!(recovery.is_some());
    }
    drop(mutation);

    assert_eq!(records(&harness).len(), 4);
}

#[tokio::test]
async fn prospective_target_is_suppressed_while_restored_current_session_owns_incidents() {
    let harness = harness();
    let former = context("alpha", 0);
    let prospective = context("beta", 1);
    for source in [
        ConnectionObservationSource::Api,
        ConnectionObservationSource::PrivateWebsocket,
    ] {
        assert!(harness
            .observer
            .observe_connection_status(&former, source, ConnectionStatus::Error, NOW)
            .await
            .is_some());
    }

    let mutation = harness.lifecycle.mutation_guard().await;
    harness.config.write().await.active_account_id = "beta".into();
    tokio::time::timeout(std::time::Duration::from_millis(100), async {
        for source in [
            ConnectionObservationSource::Api,
            ConnectionObservationSource::PrivateWebsocket,
        ] {
            assert!(harness
                .observer
                .observe_connection_status_guarded(
                    &prospective,
                    source,
                    ConnectionStatus::Error,
                    NOW + 1,
                )
                .await
                .is_none());
        }
        assert!(harness
            .observer
            .observe_auth_failure_guarded(&prospective, AuthFailureKind::SessionExpired, NOW + 1,)
            .await
            .is_none());

        harness.config.write().await.active_account_id = "alpha".into();
        for source in [
            ConnectionObservationSource::Api,
            ConnectionObservationSource::PrivateWebsocket,
        ] {
            assert!(harness
                .observer
                .observe_connection_status_guarded(
                    &former,
                    source,
                    ConnectionStatus::Connected,
                    NOW + 2,
                )
                .await
                .is_some());
        }

        let connection_id = harness
            .observer
            .observe_connection_status_guarded(
                &former,
                ConnectionObservationSource::Api,
                ConnectionStatus::Error,
                NOW + 3,
            )
            .await
            .expect("restored current account owns its connection failure");
        let connection_error = notified_connection_error(
            crate::error::AppError::Connection("sanitized".into()),
            Some(connection_id.clone()),
        );
        assert_eq!(
            serde_json::to_value(connection_error).unwrap(),
            serde_json::json!({
                "code": "CONNECTION_UNAVAILABLE",
                "message": "交易连接暂时不可用",
                "notificationId": connection_id,
            })
        );

        let auth_id = harness
            .observer
            .observe_auth_failure_guarded(&former, AuthFailureKind::SessionExpired, NOW + 4)
            .await
            .expect("restored current account owns its session expiry");
        let auth_error = notified_connection_error(
            crate::error::AppError::AuthFailure(AuthFailureKind::SessionExpired),
            Some(auth_id.clone()),
        );
        assert_eq!(
            serde_json::to_value(auth_error).unwrap(),
            serde_json::json!({
                "code": "AUTH_SESSION_EXPIRED",
                "message": "账户会话已失效",
                "notificationId": auth_id,
            })
        );
    })
    .await
    .expect("rollback observations must not reacquire the lifecycle lock");
    drop(mutation);

    assert!(harness
        .observer
        .observe_connection_status(
            &prospective,
            ConnectionObservationSource::PrivateWebsocket,
            ConnectionStatus::Error,
            NOW + 5,
        )
        .await
        .is_none());
    let records = records(&harness);
    assert_eq!(records.len(), 6);
    assert!(records.iter().all(|record| {
        record.scope
            == NotificationScope::Account {
                account_id: "alpha".into(),
            }
    }));
    assert_eq!(
        records
            .iter()
            .filter(|record| record.kind == NotificationKind::AccountSessionExpired)
            .count(),
        1
    );
}

#[tokio::test]
async fn notification_persistence_failure_is_diagnostic_only_and_never_emits_or_recurses() {
    let harness = harness();
    harness.persistence.fail_next.store(true, Ordering::SeqCst);

    let notification_id = harness
        .observer
        .observe_connection_status(
            &context("alpha", 0),
            ConnectionObservationSource::Api,
            ConnectionStatus::Error,
            NOW,
        )
        .await;

    assert!(notification_id.is_none());
    assert!(harness.persistence.files.lock().unwrap().is_empty());
    assert!(harness.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn session_notification_persistence_failure_returns_no_marker_or_diagnostic() {
    let harness = harness();
    harness.persistence.fail_next.store(true, Ordering::SeqCst);

    let notification_id = harness
        .observer
        .observe_auth_failure(&context("alpha", 0), AuthFailureKind::SessionExpired, NOW)
        .await;

    assert!(notification_id.is_none());
    assert!(harness.persistence.files.lock().unwrap().is_empty());
    assert!(harness.events.lock().unwrap().is_empty());
    assert!(harness.diagnostics.lock().unwrap().is_empty());
}

#[test]
fn committed_incident_returns_a_controlled_error_with_only_the_committed_id() {
    let notification_id = uuid::Uuid::new_v4().to_string();
    let ordinary = crate::error::AppError::Connection("safe".into());

    let notified = notified_connection_error(ordinary.clone(), Some(notification_id.clone()));
    assert_eq!(
        serde_json::to_value(notified).unwrap(),
        serde_json::json!({
            "code": "CONNECTION_UNAVAILABLE",
            "message": "交易连接暂时不可用",
            "notificationId": notification_id.clone(),
        })
    );
    assert!(
        serde_json::to_value(notified_connection_error(ordinary, None))
            .unwrap()
            .is_string()
    );

    assert_eq!(
        serde_json::to_value(notified_connection_error(
            crate::error::AppError::AuthFailure(AuthFailureKind::SessionExpired),
            Some(notification_id.clone()),
        ))
        .unwrap(),
        serde_json::json!({
            "code": "AUTH_SESSION_EXPIRED",
            "message": "账户会话已失效",
            "notificationId": notification_id,
        }),
    );
}

#[test]
fn committed_connection_marker_owns_one_error_log_without_generic_event() {
    let sink = Arc::new(Mutex::new(Vec::new()));
    let emitter = crate::events::EventEmitter::new_test(Arc::clone(&sink));
    let notified = notified_connection_error(
        crate::error::AppError::Connection("apiKey=raw-secret".into()),
        Some("notification-connection-1".into()),
    );

    let result = deliver_notified_connection_error(&emitter, notified);

    assert!(matches!(result, crate::error::AppError::Notified { .. }));
    let events = sink.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "log:entry");
    assert_eq!(events[0].1["level"], "error");
    assert_eq!(
        events[0].1["message"],
        "NOTIFIED_CONNECTION_FAILURE:CONNECTION_UNAVAILABLE"
    );
    assert!(events.iter().all(|(name, _)| name != "error:occurred"));
    assert!(!events[0].1.to_string().contains("raw-secret"));
}
