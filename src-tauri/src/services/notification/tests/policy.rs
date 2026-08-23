use crate::models::notification::{
    AccountNotificationSection, ClientNotificationFailedStep, ClientNotificationKind,
    CreateClientNotificationRequest, NotificationAction, NotificationCategory, NotificationChannel,
    NotificationEnvironment, NotificationKind, NotificationScalar, NotificationScope,
    NotificationSeverity, RiskViolationCode,
};
use crate::services::notification::policy::{PolicyContext, RiskViolationInput};
use crate::services::notification::NotificationPolicy;

fn context(source: &str) -> PolicyContext {
    PolicyContext::new("alpha", 4, source).unwrap()
}

#[test]
fn first_wave_policy_maps_every_approved_event_to_controlled_account_content() {
    let policy = NotificationPolicy::default();
    let filled = policy
        .order_filled(context("order-filled"), "order-1")
        .unwrap();
    let canceled = policy
        .order_canceled(context("order-canceled"), "order-1")
        .unwrap();
    let rejected = policy
        .order_rejected(context("order-rejected"), Some("order-1"), "submission-1")
        .unwrap();
    let expired = policy.session_expired(context("session-edge")).unwrap();

    assert_eq!(filled.kind, NotificationKind::OrderFilled);
    assert_eq!(filled.category, NotificationCategory::Trading);
    assert_eq!(filled.severity, NotificationSeverity::Success);
    assert_eq!(canceled.kind, NotificationKind::OrderCanceled);
    assert_eq!(canceled.severity, NotificationSeverity::Info);
    assert_eq!(rejected.kind, NotificationKind::OrderRejected);
    assert_eq!(rejected.severity, NotificationSeverity::Error);
    assert_eq!(expired.kind, NotificationKind::AccountSessionExpired);
    assert_eq!(expired.severity, NotificationSeverity::Warning);

    for input in [filled, canceled, rejected, expired] {
        assert_eq!(
            input.scope,
            NotificationScope::Account {
                account_id: "alpha".into()
            }
        );
        input.validate().unwrap();
    }
}

#[test]
fn risk_block_maps_to_controlled_warning_and_risk_settings_action() {
    let policy = NotificationPolicy::default();
    let input = policy
        .risk_order_blocked(
            context("submission-1"),
            RiskViolationInput {
                code: RiskViolationCode::MaxOrderQty,
                limit: Some(100.0),
            },
        )
        .unwrap();

    assert_eq!(input.kind, NotificationKind::RiskOrderBlocked);
    assert_eq!(input.category, NotificationCategory::RiskAccount);
    assert_eq!(input.severity, NotificationSeverity::Warning);
    assert_eq!(
        input.action,
        Some(NotificationAction::OpenAccountSettings {
            account_section: AccountNotificationSection::Risk,
        })
    );
    assert_eq!(
        input.content.params.get("violationCode"),
        Some(&NotificationScalar::String("maxOrderQty".into()))
    );
    assert!(!serde_json::to_string(&input)
        .unwrap()
        .contains("raw server body"));
}

#[test]
fn client_bridge_is_closed_and_maps_only_controlled_fields() {
    let policy = NotificationPolicy::default();
    let recovery = policy
        .client_account_failure(CreateClientNotificationRequest {
            account_id: "alpha".into(),
            session_epoch: 4,
            attempt_id: "0b102d04-848c-4c84-a644-033383850c71".into(),
            kind: ClientNotificationKind::AccountRecoveryFailed,
            failed_steps: vec![
                ClientNotificationFailedStep::Config,
                ClientNotificationFailedStep::Connection,
            ],
        })
        .unwrap();
    assert_eq!(recovery.kind, NotificationKind::AccountRecoveryFailed);
    assert_eq!(recovery.severity, NotificationSeverity::Error);
    assert_eq!(
        recovery.action,
        Some(NotificationAction::OpenAccountSettings {
            account_section: AccountNotificationSection::Api,
        })
    );

    let reconciliation = policy
        .client_account_failure(CreateClientNotificationRequest {
            account_id: "alpha".into(),
            session_epoch: 4,
            attempt_id: "5f99306a-385a-44e2-a830-e062b1bb4f54".into(),
            kind: ClientNotificationKind::AccountReconciliationFailed,
            failed_steps: vec![ClientNotificationFailedStep::Profiles],
        })
        .unwrap();
    assert_eq!(
        reconciliation.kind,
        NotificationKind::AccountReconciliationFailed
    );
    assert_eq!(reconciliation.severity, NotificationSeverity::Critical);
    let wire = serde_json::to_value(recovery).unwrap();
    assert!(wire.get("fallbackTitle").is_none());
    assert!(wire.get("rawError").is_none());
}

#[test]
fn connection_and_environment_policy_keep_incidents_independent() {
    let policy = NotificationPolicy::default();
    let api = policy
        .connection_unavailable(context("api-i1"), NotificationChannel::Api, "i1")
        .unwrap();
    let websocket = policy
        .connection_recovered(context("ws-i2"), NotificationChannel::Websocket, "i2")
        .unwrap();
    let environment = policy
        .environment_unavailable(
            context("environment-i3"),
            NotificationEnvironment::Production,
            "i3",
        )
        .unwrap();
    let recovered = policy
        .environment_recovered(
            context("environment-i3-recovered"),
            NotificationEnvironment::Production,
            "i3",
        )
        .unwrap();

    assert_eq!(api.kind, NotificationKind::ConnectionUnavailable);
    assert_eq!(websocket.kind, NotificationKind::ConnectionRecovered);
    assert_eq!(environment.kind, NotificationKind::EnvironmentUnavailable);
    assert_eq!(recovered.kind, NotificationKind::EnvironmentRecovered);
    assert_ne!(api.dedupe_key, websocket.dedupe_key);
    assert_ne!(environment.dedupe_key, recovered.dedupe_key);
}

#[test]
fn producer_context_requires_an_explicit_valid_account_scope() {
    let error = PolicyContext::new("", 4, "event-1").unwrap_err();
    assert_eq!(error.code(), "INVALID_NOTIFICATION_SCOPE");
}

#[test]
fn producer_context_rejects_free_text_and_secret_like_source_ids() {
    for source in [
        "raw server body",
        "bearer-secret",
        "token_api_value",
        "event:AKIAIOSFODNN7EXAMPLE",
        "event:eyJhbGciOiJIUzI1NiJ9",
        "attempt:ghp_xxxxxxxxxxxxxxxxxxxx",
    ] {
        let error = PolicyContext::new("alpha", 4, source).unwrap_err();
        assert_eq!(error.code(), "INVALID_NOTIFICATION_CONTENT");
    }
    let policy = NotificationPolicy;
    let error = policy
        .order_rejected(context("rejected-event"), Some("order-1"), "sk-secret")
        .unwrap_err();
    assert_eq!(error.code(), "INVALID_NOTIFICATION_CONTENT");

    let error = PolicyContext::new(
        "alpha",
        crate::models::notification::MAX_JAVASCRIPT_SAFE_INTEGER + 1,
        "event-1",
    )
    .unwrap_err();
    assert_eq!(error.code(), "INVALID_NOTIFICATION_CONTENT");
}

#[test]
fn policy_rejects_uncontrolled_incident_submission_and_dedupe_components() {
    let policy = NotificationPolicy;
    for incident_id in [
        "raw incident text".to_string(),
        "incident-secret-value".to_string(),
        "incident:edge".to_string(),
        "a".repeat(257),
    ] {
        let connection = policy
            .connection_unavailable(
                context("connection-edge"),
                NotificationChannel::Api,
                &incident_id,
            )
            .unwrap_err();
        assert_eq!(connection.code(), "INVALID_NOTIFICATION_CONTENT");

        let environment = policy
            .environment_unavailable(
                context("environment-edge"),
                NotificationEnvironment::Production,
                &incident_id,
            )
            .unwrap_err();
        assert_eq!(environment.code(), "INVALID_NOTIFICATION_CONTENT");
    }

    for submission_id in ["submission:1".to_string(), "s".repeat(257)] {
        let error = policy
            .order_rejected(context("rejected-event"), Some("order-1"), &submission_id)
            .unwrap_err();
        assert_eq!(error.code(), "INVALID_NOTIFICATION_CONTENT");
    }
}
