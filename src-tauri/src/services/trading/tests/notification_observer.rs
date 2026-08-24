use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::super::*;
use crate::api::response::classify_create_order_failure;
use crate::models::config::{ApiCredential, AppConfig, RiskConfig};
use crate::models::notification::{ListNotificationsRequest, NotificationFilter, NotificationKind};
use crate::models::trading::{OrderStatus, SubmissionContext, TradingFailureKind};
use crate::services::notification::{
    NotificationEmitter, NotificationRuntime, NotificationService, OrderObservationOrigin,
    ViewContext,
};
use crate::storage::notification_store::{NotificationFileV1, NotificationPersistence};
use crate::storage::{NotificationStore, RiskUsageStore};

const NOW_MS: u64 = 1_784_606_400_000;

struct ObserverHarness {
    observer: OrderNotificationObserver,
    service: Arc<NotificationService>,
    path: PathBuf,
}

impl ObserverHarness {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir()
            .join(format!(
                "easiflux-order-observer-{label}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4(),
            ))
            .join("notifications.v1.json");
        Self::load(path)
    }

    fn load(path: PathBuf) -> Self {
        let emitter: NotificationEmitter = Arc::new(|_| Ok(()));
        let service = Arc::new(
            NotificationService::load(
                NotificationStore::with_path(path.clone()),
                &["alpha".into(), "beta".into()],
                NOW_MS,
                emitter,
            )
            .unwrap(),
        );
        let runtime = Arc::new(NotificationRuntime::Available(Arc::clone(&service)));
        Self {
            observer: OrderNotificationObserver::new(runtime),
            service,
            path,
        }
    }

    async fn records(
        &self,
        account_id: &str,
    ) -> Vec<crate::models::notification::NotificationRecord> {
        self.service
            .list(
                ViewContext::account(account_id).unwrap(),
                ListNotificationsRequest {
                    account_id: Some(account_id.into()),
                    filter: NotificationFilter::All,
                    cursor: None,
                    limit: 100,
                },
                NOW_MS + 10_000,
            )
            .await
            .unwrap()
            .items
    }
}

impl Drop for ObserverHarness {
    fn drop(&mut self) {
        if let Some(root) = self.path.parent() {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

fn order(id: &str, status: OrderStatus) -> Order {
    Order {
        order_id: id.into(),
        symbol: "BTCUSDT".into(),
        side: "Buy".into(),
        order_type: "Limit".into(),
        price: "100".into(),
        qty: "1".into(),
        status,
        order_link_id: None,
        filled_qty: "0".into(),
        avg_price: "0".into(),
    }
}

fn market_request(order_link_id: Option<&str>) -> PlaceOrderRequest {
    PlaceOrderRequest {
        symbol: "BTCUSDT".into(),
        side: "Buy".into(),
        order_type: "Market".into(),
        qty: "1".into(),
        position_idx: 0,
        price: None,
        time_in_force: None,
        order_link_id: order_link_id.map(str::to_owned),
        reduce_only: None,
    }
}

fn read_http_request(socket: &mut std::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 2048];
    loop {
        let count = socket.read(&mut buffer).expect("read API request");
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end + 4]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length: ")
                    .or_else(|| line.strip_prefix("content-length: "))
            })
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if bytes.len() >= header_end + 4 + content_length {
            break;
        }
    }
    String::from_utf8(bytes).expect("test request is UTF-8")
}

async fn api_client_for_responses(
    responses_after_initial_time: Vec<serde_json::Value>,
) -> (
    Arc<ApiClient>,
    std::thread::JoinHandle<()>,
    Arc<Mutex<Vec<String>>>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let address = listener.local_addr().expect("test server address");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&requests);
    let server = std::thread::spawn(move || {
        let responses = std::iter::once(serde_json::json!({
            "code": 0,
            "data": {"time": "1782850580"}
        }))
        .chain(responses_after_initial_time);
        for payload in responses {
            let response_body = serde_json::to_string(&payload).unwrap();
            let (mut socket, _) = listener.accept().expect("accept API request");
            captured
                .lock()
                .unwrap()
                .push(read_http_request(&mut socket));
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                response_body.len()
            );
            socket
                .write_all(response.as_bytes())
                .expect("write API response");
        }
    });
    let client = Arc::new(ApiClient::new());
    client
        .set_credential(ApiCredential {
            api_key: "test-key".into(),
            api_secret: "test-secret".into(),
            base_url: format!("http://{address}"),
            label: "test".into(),
        })
        .await;
    (client, server, requests)
}

fn request_json(request: &str) -> serde_json::Value {
    let (_, body) = request
        .split_once("\r\n\r\n")
        .expect("HTTP request contains a body delimiter");
    serde_json::from_str(body).expect("request body is JSON")
}

struct FailOncePersistence {
    fail: AtomicBool,
    attempts: AtomicUsize,
}

impl NotificationPersistence for FailOncePersistence {
    fn save(&self, _file: &NotificationFileV1) -> AppResult<()> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        if self.fail.swap(false, Ordering::SeqCst) {
            Err(AppError::Storage("private-store-detail".into()))
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn observer_save_failure_is_swallowed_and_same_terminal_can_retry() {
    let persistence = Arc::new(FailOncePersistence {
        fail: AtomicBool::new(true),
        attempts: AtomicUsize::new(0),
    });
    let service = Arc::new(NotificationService::from_snapshot(
        NotificationFileV1::empty(),
        Arc::clone(&persistence),
        Arc::new(|_| Ok(())),
        NOW_MS,
    ));
    let observer = OrderNotificationObserver::new(Arc::new(NotificationRuntime::Available(
        Arc::clone(&service),
    )));
    let filled = order("order-save-retry-1", OrderStatus::Filled);

    assert!(observer
        .observe(
            "alpha",
            1,
            &filled,
            OrderObservationOrigin::Command,
            None,
            NOW_MS,
        )
        .await
        .is_none());
    assert_eq!(persistence.attempts.load(Ordering::SeqCst), 1);
    assert_eq!(service.revision().await, "0");
    assert!(observer
        .observe(
            "alpha",
            1,
            &filled,
            OrderObservationOrigin::Command,
            None,
            NOW_MS + 1,
        )
        .await
        .is_some());
    assert_eq!(persistence.attempts.load(Ordering::SeqCst), 2);
    assert_eq!(
        service
            .list(
                ViewContext::account("alpha").unwrap(),
                ListNotificationsRequest {
                    account_id: Some("alpha".into()),
                    filter: NotificationFilter::All,
                    cursor: None,
                    limit: 100,
                },
                NOW_MS + 2,
            )
            .await
            .unwrap()
            .items
            .len(),
        1
    );
}

#[tokio::test]
async fn accepted_terminal_order_stays_successful_when_notification_save_fails() {
    let persistence = Arc::new(FailOncePersistence {
        fail: AtomicBool::new(true),
        attempts: AtomicUsize::new(0),
    });
    let service = Arc::new(NotificationService::from_snapshot(
        NotificationFileV1::empty(),
        Arc::clone(&persistence),
        Arc::new(|_| Ok(())),
        NOW_MS,
    ));
    let observer = OrderNotificationObserver::new(Arc::new(NotificationRuntime::Available(
        Arc::clone(&service),
    )));
    let risk_path = std::env::temp_dir().join(format!(
        "easiflux-accepted-terminal-notification-failure-risk-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
        RiskConfig {
            max_daily_orders: 1,
            ..Default::default()
        },
        RiskUsageStore::with_path(risk_path.clone()),
    )));
    let (api, server, _) = api_client_for_responses(vec![json!({
        "code": 0,
        "data": {
            "order_id": "order-accepted-save-failure-1",
            "order_status": "Filled"
        }
    })])
    .await;
    let context = SubmissionContext {
        submission_id: uuid::Uuid::new_v4().to_string(),
        account_id: "alpha".into(),
        session_epoch: 1,
    };
    let trade_log_effects = Arc::new(AtomicUsize::new(0));
    let event_effects = Arc::new(AtomicUsize::new(0));
    let analytics_effects = Arc::new(AtomicUsize::new(0));
    let counted_trade_log = Arc::clone(&trade_log_effects);
    let counted_event = Arc::clone(&event_effects);
    let counted_analytics = Arc::clone(&analytics_effects);

    let result = execute_place_order(
        api.as_ref(),
        &risk,
        &observer,
        &context,
        market_request(None),
        None,
        NOW_MS,
        move |_| async move {
            counted_trade_log.fetch_add(1, Ordering::SeqCst);
            counted_event.fetch_add(1, Ordering::SeqCst);
            counted_analytics.fetch_add(1, Ordering::SeqCst);
        },
    )
    .await;
    server.join().expect("test server exits");

    assert!(matches!(
        result,
        Ok(Order {
            status: OrderStatus::Filled,
            ..
        })
    ));
    assert_eq!(trade_log_effects.load(Ordering::SeqCst), 1);
    assert_eq!(event_effects.load(Ordering::SeqCst), 1);
    assert_eq!(analytics_effects.load(Ordering::SeqCst), 1);
    assert_eq!(persistence.attempts.load(Ordering::SeqCst), 1);
    assert_eq!(service.revision().await, "0");
    assert_eq!(
        RiskUsageStore::with_path(risk_path.clone())
            .load()
            .unwrap()
            .unwrap()
            .occupied_orders,
        1
    );

    let _ = std::fs::remove_file(risk_path);
}

#[tokio::test]
async fn confirmed_rejection_persists_when_release_save_fails_without_leaking_detail() {
    let harness = ObserverHarness::new("rejection-with-release-failure");
    let risk_root = std::env::temp_dir().join(format!(
        "easiflux-risk-release-failure-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let risk_path = risk_root.join("risk_usage.toml");
    let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
        RiskConfig {
            max_daily_orders: 1,
            ..Default::default()
        },
        RiskUsageStore::with_path(risk_path.clone()),
    )));
    let request = PlaceOrderRequest {
        symbol: "BTCUSDT".into(),
        side: "Buy".into(),
        order_type: "Market".into(),
        qty: "1".into(),
        position_idx: 0,
        price: None,
        time_in_force: None,
        order_link_id: None,
        reduce_only: None,
    };
    let reservation = risk
        .read()
        .await
        .reserve_order(&request, None, NOW_MS)
        .unwrap();
    let private_detail = PathBuf::from(format!("{}.tmp", risk_path.display()));
    std::fs::create_dir(&private_detail).unwrap();
    let context = SubmissionContext {
        submission_id: uuid::Uuid::new_v4().to_string(),
        account_id: "alpha".into(),
        session_epoch: 1,
    };

    let error = handle_failed_submission(
        &risk,
        &harness.observer,
        &reservation,
        &context,
        &context.submission_id,
        AppError::TradingFailure(crate::models::trading::TradingFailure::rejected()),
        NOW_MS + 1,
    )
    .await;

    assert!(matches!(error, AppError::Notified { .. }));
    let records = harness.records("alpha").await;
    assert_eq!(records.len(), 1);
    assert!(records[0].source_event_id.as_ref().unwrap().len() <= 256);
    assert_eq!(
        RiskUsageStore::with_path(risk_path.clone())
            .load()
            .unwrap()
            .unwrap()
            .occupied_orders,
        1
    );
    let rendered = serde_json::to_string(&error).unwrap();
    assert!(!rendered.contains(private_detail.to_string_lossy().as_ref()));
    assert!(!rendered.contains("risk_usage"));

    std::fs::remove_dir(&private_detail).unwrap();
    std::fs::remove_dir_all(&risk_root).unwrap();
}

#[tokio::test]
async fn undocumented_list_rejection_is_ambiguous_without_notification_or_success_side_effects() {
    let harness = ObserverHarness::new("list-rejection-full-boundary");
    let risk_path = std::env::temp_dir().join(format!(
        "easiflux-list-rejection-risk-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
        RiskConfig {
            max_daily_orders: 1,
            ..Default::default()
        },
        RiskUsageStore::with_path(risk_path.clone()),
    )));
    let (api, server, _) = api_client_for_responses(vec![json!({
        "code": 0,
        "data": {
            "items": [{"orderId": "123", "orderStatus": "Rejected"}]
        }
    })])
    .await;
    let context = SubmissionContext {
        submission_id: uuid::Uuid::new_v4().to_string(),
        account_id: "alpha".into(),
        session_epoch: 1,
    };
    let success_effects = Arc::new(AtomicUsize::new(0));
    let success_effects_for_call = Arc::clone(&success_effects);

    let result = execute_place_order(
        api.as_ref(),
        &risk,
        &harness.observer,
        &context,
        PlaceOrderRequest {
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Market".into(),
            qty: "1".into(),
            position_idx: 0,
            price: None,
            time_in_force: None,
            order_link_id: None,
            reduce_only: None,
        },
        None,
        NOW_MS,
        move |_| async move {
            success_effects_for_call.fetch_add(1, Ordering::SeqCst);
        },
    )
    .await;
    server.join().expect("test server exits");

    assert!(matches!(result, Err(AppError::Internal(_))));
    assert_eq!(success_effects.load(Ordering::SeqCst), 0);
    assert!(harness.records("alpha").await.is_empty());
    assert!(risk
        .read()
        .await
        .reserve_order(
            &PlaceOrderRequest {
                symbol: "BTCUSDT".into(),
                side: "Buy".into(),
                order_type: "Market".into(),
                qty: "1".into(),
                position_idx: 0,
                price: None,
                time_in_force: None,
                order_link_id: None,
                reduce_only: None,
            },
            None,
            NOW_MS + 1,
        )
        .is_err());

    let _ = std::fs::remove_file(risk_path);
}

#[tokio::test]
async fn noncanonical_create_order_codes_are_ambiguous_and_keep_the_reservation() {
    for (label, payload) in [
        (
            "missing",
            json!({"data": {"order_id": "123", "order_status": "New"}}),
        ),
        (
            "string-zero",
            json!({"code": "0", "data": {"order_id": "123", "order_status": "New"}}),
        ),
        (
            "numeric-200",
            json!({"code": 200, "data": {"order_id": "123", "order_status": "New"}}),
        ),
        (
            "success-string",
            json!({"code": "SUCCESS", "data": {"order_id": "123", "order_status": "New"}}),
        ),
        (
            "boolean",
            json!({"code": true, "data": {"order_id": "123", "order_status": "New"}}),
        ),
        (
            "object",
            json!({"code": {"value": 0}, "data": {"order_id": "123", "order_status": "New"}}),
        ),
        (
            "unknown-numeric",
            json!({"code": 901234, "data": {"order_id": "123", "order_status": "New"}}),
        ),
    ] {
        let harness = ObserverHarness::new(&format!("ambiguous-code-{label}"));
        let risk_path = std::env::temp_dir().join(format!(
            "easiflux-ambiguous-code-risk-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
            RiskConfig {
                max_daily_orders: 1,
                ..Default::default()
            },
            RiskUsageStore::with_path(risk_path.clone()),
        )));
        let (api, server, _) = api_client_for_responses(vec![payload]).await;
        let context = SubmissionContext {
            submission_id: uuid::Uuid::new_v4().to_string(),
            account_id: "alpha".into(),
            session_epoch: 1,
        };
        let success_effects = Arc::new(AtomicUsize::new(0));
        let success_effects_for_call = Arc::clone(&success_effects);

        let result = execute_place_order(
            api.as_ref(),
            &risk,
            &harness.observer,
            &context,
            market_request(None),
            None,
            NOW_MS,
            move |_| async move {
                success_effects_for_call.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await;
        server.join().expect("test server exits");

        assert!(
            matches!(result, Err(AppError::Internal(_))),
            "{label} must be ambiguous: {result:?}"
        );
        assert_eq!(success_effects.load(Ordering::SeqCst), 0, "{label}");
        assert!(harness.records("alpha").await.is_empty(), "{label}");
        assert!(
            risk.read()
                .await
                .reserve_order(&market_request(None), None, NOW_MS + 1)
                .is_err(),
            "{label} must keep its reservation"
        );
        assert_eq!(
            RiskUsageStore::with_path(risk_path.clone())
                .load()
                .unwrap()
                .unwrap()
                .occupied_orders,
            1,
            "{label} must keep its reservation on disk"
        );

        let _ = std::fs::remove_file(risk_path);
    }
}

#[tokio::test]
async fn generic_provider_timestamp_and_sign_failures_keep_quota_and_publish_nothing() {
    let cases = [
        (
            "generic-provider",
            vec![json!({
                "code": 26200010,
                "message": "private-provider-detail"
            })],
        ),
        (
            "timestamp",
            vec![
                json!({"code": 26200002, "message": "private-timestamp-detail"}),
                json!({"code": 0, "data": {"time": "1782850580"}}),
                json!({"code": 26200002, "message": "private-timestamp-detail"}),
            ],
        ),
        (
            "sign",
            vec![
                json!({"code": 26200003, "message": "private-sign-detail"}),
                json!({"code": 0, "data": {"time": "1782850580"}}),
                json!({"code": 26200003, "message": "private-sign-detail"}),
            ],
        ),
    ];

    for (label, responses) in cases {
        let harness = ObserverHarness::new(&format!("ambiguous-{label}"));
        let risk_path = std::env::temp_dir().join(format!(
            "easiflux-ambiguous-{label}-risk-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
            RiskConfig {
                max_daily_orders: 1,
                ..Default::default()
            },
            RiskUsageStore::with_path(risk_path.clone()),
        )));
        let (api, server, _) = api_client_for_responses(responses).await;
        let context = SubmissionContext {
            submission_id: uuid::Uuid::new_v4().to_string(),
            account_id: "alpha".into(),
            session_epoch: 1,
        };

        let result = execute_place_order(
            api.as_ref(),
            &risk,
            &harness.observer,
            &context,
            market_request(None),
            None,
            NOW_MS,
            |_| async {},
        )
        .await;
        server.join().expect("test server exits");

        assert!(
            matches!(result, Err(AppError::Trading(_))),
            "{label}: {result:?}"
        );
        let rendered = serde_json::to_string(&result.unwrap_err()).unwrap();
        assert!(!rendered.contains("private-"), "{label}: {rendered}");
        assert!(harness.records("alpha").await.is_empty(), "{label}");
        assert!(
            risk.read()
                .await
                .reserve_order(&market_request(None), None, NOW_MS + 1)
                .is_err(),
            "{label} must keep its reservation in memory"
        );
        assert_eq!(
            RiskUsageStore::with_path(risk_path.clone())
                .load()
                .unwrap()
                .unwrap()
                .occupied_orders,
            1,
            "{label} must keep its reservation on disk"
        );

        let _ = std::fs::remove_file(risk_path);
    }
}

#[tokio::test]
async fn missing_api_credential_keeps_quota_and_publishes_nothing() {
    let harness = ObserverHarness::new("ambiguous-auth");
    let risk_path = std::env::temp_dir().join(format!(
        "easiflux-ambiguous-auth-risk-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
        RiskConfig {
            max_daily_orders: 1,
            ..Default::default()
        },
        RiskUsageStore::with_path(risk_path.clone()),
    )));
    let (api, server, _) = api_client_for_responses(Vec::new()).await;
    api.clear_credential().await;
    let context = SubmissionContext {
        submission_id: uuid::Uuid::new_v4().to_string(),
        account_id: "alpha".into(),
        session_epoch: 1,
    };

    let result = execute_place_order(
        api.as_ref(),
        &risk,
        &harness.observer,
        &context,
        market_request(None),
        None,
        NOW_MS,
        |_| async {},
    )
    .await;
    server.join().expect("test server exits");

    assert!(matches!(result, Err(AppError::Auth(_))), "{result:?}");
    assert!(harness.records("alpha").await.is_empty());
    assert!(risk
        .read()
        .await
        .reserve_order(&market_request(None), None, NOW_MS + 1)
        .is_err());
    assert_eq!(
        RiskUsageStore::with_path(risk_path.clone())
            .load()
            .unwrap()
            .unwrap()
            .occupied_orders,
        1
    );

    let _ = std::fs::remove_file(risk_path);
}

#[tokio::test]
async fn caller_order_link_is_preserved_outbound_and_returned_while_submission_stays_independent() {
    let harness = ObserverHarness::new("caller-order-link");
    let risk_path = std::env::temp_dir().join(format!(
        "easiflux-caller-link-risk-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
        RiskConfig {
            max_daily_orders: 1,
            ..Default::default()
        },
        RiskUsageStore::with_path(risk_path.clone()),
    )));
    let caller_link = "client-link_2026.08";
    let (api, server, requests) = api_client_for_responses(vec![json!({
        "code": 0,
        "data": {
            "order_id": "order-caller-link-1",
            "order_status": "New",
            "order_link_id": caller_link,
        }
    })])
    .await;
    let context = SubmissionContext {
        submission_id: uuid::Uuid::new_v4().to_string(),
        account_id: "alpha".into(),
        session_epoch: 1,
    };
    let success_effects = Arc::new(AtomicUsize::new(0));
    let counted_effects = Arc::clone(&success_effects);

    let result = execute_place_order(
        api.as_ref(),
        &risk,
        &harness.observer,
        &context,
        market_request(Some(caller_link)),
        None,
        NOW_MS,
        move |_| async move {
            counted_effects.fetch_add(1, Ordering::SeqCst);
        },
    )
    .await
    .unwrap();
    server.join().expect("test server exits");

    let requests = requests.lock().unwrap();
    let outbound = request_json(
        requests
            .iter()
            .find(|request| request.starts_with("POST "))
            .expect("create-order request was sent"),
    );
    assert_eq!(outbound["order_link_id"], caller_link);
    assert_ne!(outbound["order_link_id"], context.submission_id);
    assert_eq!(result.order_link_id.as_deref(), Some(caller_link));
    assert_eq!(success_effects.load(Ordering::SeqCst), 1);

    let _ = std::fs::remove_file(risk_path);
}

#[tokio::test]
async fn omitted_order_link_uses_one_generated_submission_id_across_timestamp_retry() {
    let harness = ObserverHarness::new("generated-link-retry");
    let risk_path = std::env::temp_dir().join(format!(
        "easiflux-generated-link-risk-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
        RiskConfig::default(),
        RiskUsageStore::with_path(risk_path.clone()),
    )));
    let context = SubmissionContext {
        submission_id: uuid::Uuid::new_v4().to_string(),
        account_id: "alpha".into(),
        session_epoch: 1,
    };
    let (api, server, requests) = api_client_for_responses(vec![
        json!({"code": 26200002, "message": "timestamp"}),
        json!({"code": 0, "data": {"time": "1782850580"}}),
        json!({
            "code": 0,
            "data": {
                "order_id": "order-generated-link-1",
                "order_status": "New",
                "order_link_id": context.submission_id,
            }
        }),
    ])
    .await;

    let result = execute_place_order(
        api.as_ref(),
        &risk,
        &harness.observer,
        &context,
        market_request(None),
        None,
        NOW_MS,
        |_| async {},
    )
    .await
    .unwrap();
    server.join().expect("test server exits");

    let post_bodies = requests
        .lock()
        .unwrap()
        .iter()
        .filter(|request| request.starts_with("POST "))
        .map(|request| request_json(request))
        .collect::<Vec<_>>();
    assert_eq!(post_bodies.len(), 2);
    assert!(post_bodies
        .iter()
        .all(|body| body["order_link_id"] == context.submission_id));
    assert_eq!(
        result.order_link_id.as_deref(),
        Some(context.submission_id.as_str())
    );

    let _ = std::fs::remove_file(risk_path);
}

#[tokio::test]
async fn caller_link_rejection_rebuilds_alias_and_absorbs_later_ws_terminal() {
    let harness = ObserverHarness::new("caller-link-restart-alias");
    let risk_path = std::env::temp_dir().join(format!(
        "easiflux-caller-link-rejection-risk-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
        RiskConfig::default(),
        RiskUsageStore::with_path(risk_path.clone()),
    )));
    let caller_link = "sk-secret:\nclient-1";
    let (api, server, requests) = api_client_for_responses(vec![json!({
        "code": 0,
        "data": {
            "order_id": "provider-rejected-caller-1",
            "orderStatus": "Rejected",
            "order_link_id": caller_link,
        }
    })])
    .await;
    let context = SubmissionContext {
        submission_id: uuid::Uuid::new_v4().to_string(),
        account_id: "alpha".into(),
        session_epoch: 1,
    };
    let success_effects = Arc::new(AtomicUsize::new(0));
    let counted_effects = Arc::clone(&success_effects);

    let result = execute_place_order(
        api.as_ref(),
        &risk,
        &harness.observer,
        &context,
        market_request(Some(caller_link)),
        None,
        NOW_MS,
        move |_| async move {
            counted_effects.fetch_add(1, Ordering::SeqCst);
        },
    )
    .await;
    server.join().expect("test server exits");
    assert!(matches!(result, Err(AppError::Notified { .. })));
    let post = requests
        .lock()
        .unwrap()
        .iter()
        .find(|request| request.starts_with("POST "))
        .map(|request| request_json(request))
        .unwrap();
    assert_eq!(post["order_link_id"], caller_link);
    assert_eq!(success_effects.load(Ordering::SeqCst), 0);
    let records = harness.records("alpha").await;
    assert_eq!(records.len(), 1);
    assert!(records[0].source_event_id.as_ref().unwrap().len() <= 256);
    assert_eq!(
        RiskUsageStore::with_path(risk_path.clone())
            .load()
            .unwrap()
            .unwrap()
            .occupied_orders,
        0
    );
    assert!(risk
        .read()
        .await
        .reserve_order(&market_request(None), None, NOW_MS + 1)
        .is_ok());
    let persisted = std::fs::read_to_string(&harness.path).unwrap();
    assert!(!persisted.contains(caller_link));
    assert!(!persisted.contains("sk-secret"));

    let path = harness.path.clone();
    std::mem::forget(harness);
    let restarted = ObserverHarness::load(path);
    let mut stream_order = order("order-caller-link-restart-1", OrderStatus::New);
    stream_order.order_link_id = Some(caller_link.into());
    restarted
        .observer
        .observe(
            "alpha",
            1,
            &stream_order,
            OrderObservationOrigin::Realtime,
            None,
            NOW_MS + 1,
        )
        .await;
    stream_order.status = OrderStatus::Filled;
    assert!(restarted
        .observer
        .observe(
            "alpha",
            1,
            &stream_order,
            OrderObservationOrigin::Realtime,
            None,
            NOW_MS + 2,
        )
        .await
        .is_none());
    assert_eq!(restarted.records("alpha").await.len(), 1);

    let _ = std::fs::remove_file(risk_path);
}

#[tokio::test]
async fn oversized_secret_like_order_link_is_rejected_before_reservation_or_network() {
    let harness = ObserverHarness::new("oversized-link-rejected");
    let risk_path = std::env::temp_dir().join(format!(
        "easiflux-unsafe-link-risk-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
        RiskConfig::default(),
        RiskUsageStore::with_path(risk_path.clone()),
    )));
    let unsafe_link = format!("sk-secret\n{}", "x".repeat(300));
    let api = Arc::new(ApiClient::new());
    let context = SubmissionContext {
        submission_id: uuid::Uuid::new_v4().to_string(),
        account_id: "alpha".into(),
        session_epoch: 1,
    };

    let result = execute_place_order(
        api.as_ref(),
        &risk,
        &harness.observer,
        &context,
        market_request(Some(&unsafe_link)),
        None,
        NOW_MS,
        |_| async {},
    )
    .await;
    assert!(matches!(result, Err(AppError::Trading(_))));
    assert!(!result.unwrap_err().to_string().contains("sk-secret"));
    assert!(harness.records("alpha").await.is_empty());
    assert!(
        !risk_path.exists(),
        "validation must run before reservation"
    );

    let _ = std::fs::remove_file(risk_path);
}

#[tokio::test]
async fn buffered_new_filled_survives_snapshot_already_filled_and_notifies_once() {
    let harness = ObserverHarness::new("buffered-live-before-snapshot");
    let context = OrderStreamContext {
        account_id: "alpha".into(),
        session_epoch: 1,
    };
    let snapshot = order("order-buffered-live-1", OrderStatus::Filled);
    let pending = vec![
        order("order-buffered-live-1", OrderStatus::New),
        order("order-buffered-live-1", OrderStatus::Filled),
    ];

    harness
        .observer
        .seed_snapshot_then_replay(&context, &[snapshot], pending, NOW_MS)
        .await
        .unwrap();

    let records = harness.records("alpha").await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, NotificationKind::OrderFilled);
    assert_eq!(records[0].occurrence_count, 1);
}

#[tokio::test]
async fn buffered_nonterminal_without_live_terminal_accepts_snapshot_terminal_baseline() {
    for (label, pending_status) in [
        ("new", OrderStatus::New),
        ("partial", OrderStatus::PartiallyFilled),
    ] {
        let harness = ObserverHarness::new(&format!("buffered-{label}-snapshot-terminal"));
        let context = OrderStreamContext {
            account_id: "alpha".into(),
            session_epoch: 1,
        };
        let order_id = format!("order-buffered-{label}-1");
        harness
            .observer
            .seed_snapshot_then_replay(
                &context,
                &[order(&order_id, OrderStatus::Filled)],
                vec![order(&order_id, pending_status)],
                NOW_MS,
            )
            .await
            .unwrap();

        assert!(harness.records("alpha").await.is_empty());
        assert!(harness
            .observer
            .observe(
                "alpha",
                1,
                &order(&order_id, OrderStatus::Filled),
                OrderObservationOrigin::Realtime,
                None,
                NOW_MS + 2,
            )
            .await
            .is_none());
        assert!(harness.records("alpha").await.is_empty());
    }
}

#[tokio::test]
async fn standalone_buffered_terminal_does_not_override_snapshot_terminal_baseline() {
    let harness = ObserverHarness::new("standalone-buffered-terminal");
    let context = OrderStreamContext {
        account_id: "alpha".into(),
        session_epoch: 1,
    };
    let terminal = order("order-standalone-terminal-1", OrderStatus::Filled);
    harness
        .observer
        .seed_snapshot_then_replay(&context, &[terminal.clone()], vec![terminal], NOW_MS)
        .await
        .unwrap();

    assert!(harness.records("alpha").await.is_empty());
}

#[tokio::test]
async fn buffered_unknown_does_not_shadow_a_usable_snapshot_predecessor() {
    let harness = ObserverHarness::new("buffered-unknown-snapshot-new");
    let context = OrderStreamContext {
        account_id: "alpha".into(),
        session_epoch: 1,
    };
    let snapshot = order("order-buffered-unknown-1", OrderStatus::New);
    let unknown = order("order-buffered-unknown-1", OrderStatus::Unknown);
    harness
        .observer
        .seed_snapshot_then_replay(&context, &[snapshot], vec![unknown], NOW_MS)
        .await
        .unwrap();

    assert!(harness
        .observer
        .observe(
            "alpha",
            1,
            &order("order-buffered-unknown-1", OrderStatus::Filled),
            OrderObservationOrigin::Realtime,
            None,
            NOW_MS + 2,
        )
        .await
        .is_some());
    assert_eq!(harness.records("alpha").await.len(), 1);
}

#[tokio::test]
async fn snapshot_terminal_seeds_and_matching_command_notifies_once() {
    let harness = ObserverHarness::new("snapshot-command");
    let filled = order("order-100", OrderStatus::Filled);

    assert!(harness
        .observer
        .observe(
            "alpha",
            7,
            &filled,
            OrderObservationOrigin::Snapshot,
            None,
            NOW_MS
        )
        .await
        .is_none());
    assert!(harness.records("alpha").await.is_empty());

    assert!(harness
        .observer
        .observe(
            "alpha",
            7,
            &filled,
            OrderObservationOrigin::Command,
            None,
            NOW_MS + 1
        )
        .await
        .is_some());
    assert!(harness
        .observer
        .observe(
            "alpha",
            7,
            &filled,
            OrderObservationOrigin::Command,
            None,
            NOW_MS + 2
        )
        .await
        .is_none());
    let records = harness.records("alpha").await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, NotificationKind::OrderFilled);
}

#[tokio::test]
async fn realtime_nonterminal_to_each_terminal_notifies_once_in_frame_order() {
    for (suffix, initial, terminal, expected_kind) in [
        (
            "filled",
            OrderStatus::New,
            OrderStatus::Filled,
            NotificationKind::OrderFilled,
        ),
        (
            "canceled",
            OrderStatus::PartiallyFilled,
            OrderStatus::Cancelled,
            NotificationKind::OrderCanceled,
        ),
        (
            "rejected",
            OrderStatus::New,
            OrderStatus::Rejected,
            NotificationKind::OrderRejected,
        ),
    ] {
        let harness = ObserverHarness::new(suffix);
        let order_id = format!("order-{suffix}-1");
        let first = order(&order_id, initial);
        let final_order = order(&order_id, terminal);

        assert!(harness
            .observer
            .observe(
                "alpha",
                2,
                &first,
                OrderObservationOrigin::Realtime,
                None,
                NOW_MS
            )
            .await
            .is_none());
        assert!(harness
            .observer
            .observe(
                "alpha",
                2,
                &final_order,
                OrderObservationOrigin::Realtime,
                None,
                NOW_MS + 1,
            )
            .await
            .is_some());
        assert!(harness
            .observer
            .observe(
                "alpha",
                2,
                &final_order,
                OrderObservationOrigin::Realtime,
                None,
                NOW_MS + 2,
            )
            .await
            .is_none());
        let records = harness.records("alpha").await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, expected_kind);
    }
}

#[tokio::test]
async fn rest_ws_duplicates_and_final_mismatch_do_not_create_a_second_record() {
    let harness = ObserverHarness::new("duplicate-mismatch");
    let new = order("order-duplicate-1", OrderStatus::New);
    let filled = order("order-duplicate-1", OrderStatus::Filled);
    let canceled = order("order-duplicate-1", OrderStatus::Cancelled);

    harness
        .observer
        .observe(
            "alpha",
            3,
            &new,
            OrderObservationOrigin::Realtime,
            None,
            NOW_MS,
        )
        .await;
    assert!(harness
        .observer
        .observe(
            "alpha",
            3,
            &filled,
            OrderObservationOrigin::Command,
            None,
            NOW_MS + 1
        )
        .await
        .is_some());
    for (order, origin) in [
        (&filled, OrderObservationOrigin::Realtime),
        (&filled, OrderObservationOrigin::Snapshot),
        (&canceled, OrderObservationOrigin::Realtime),
        (&canceled, OrderObservationOrigin::Command),
    ] {
        assert!(harness
            .observer
            .observe("alpha", 3, order, origin, None, NOW_MS + 2)
            .await
            .is_none());
    }
    assert_eq!(harness.records("alpha").await.len(), 1);
}

#[tokio::test]
async fn partial_unknown_and_snapshot_rows_never_persist() {
    let harness = ObserverHarness::new("nonterminal");
    for (index, status, origin) in [
        (1, OrderStatus::New, OrderObservationOrigin::Command),
        (
            2,
            OrderStatus::PartiallyFilled,
            OrderObservationOrigin::Realtime,
        ),
        (3, OrderStatus::Unknown, OrderObservationOrigin::Command),
        (4, OrderStatus::Filled, OrderObservationOrigin::Snapshot),
    ] {
        assert!(harness
            .observer
            .observe(
                "alpha",
                1,
                &order(&format!("order-pending-{index}"), status),
                origin,
                None,
                NOW_MS + index,
            )
            .await
            .is_none());
    }
    assert!(harness.records("alpha").await.is_empty());
}

#[tokio::test]
async fn api_rejection_without_order_id_is_idempotent_across_restart_by_submission_id() {
    let harness = ObserverHarness::new("submission-restart");
    let context = SubmissionContext {
        submission_id: uuid::Uuid::new_v4().to_string(),
        account_id: "alpha".into(),
        session_epoch: 5,
    };
    let first_id = harness
        .observer
        .observe_rejection(&context, None, NOW_MS)
        .await
        .expect("confirmed rejection should persist");
    let path = harness.path.clone();
    // Keep the persisted fixture in place while simulating a process restart.
    std::mem::forget(harness);

    let restarted = ObserverHarness::load(path);
    assert!(restarted
        .observer
        .observe_rejection(&context, None, NOW_MS + 1)
        .await
        .is_none());
    let records = restarted.records("alpha").await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, first_id);
    assert_eq!(records[0].kind, NotificationKind::OrderRejected);
}

#[tokio::test]
async fn persisted_terminal_absorbs_a_different_terminal_after_restart() {
    let harness = ObserverHarness::new("restart-final-mismatch");
    let path = harness.path.clone();
    assert!(harness
        .observer
        .observe(
            "alpha",
            1,
            &order("order-restart-final-1", OrderStatus::Filled),
            OrderObservationOrigin::Command,
            None,
            NOW_MS,
        )
        .await
        .is_some());
    let revision_before = harness.service.revision().await;
    let disk_before = std::fs::read(&path).unwrap();
    std::mem::forget(harness);

    let restarted = ObserverHarness::load(path);
    assert!(restarted
        .observer
        .observe(
            "alpha",
            2,
            &order("order-restart-final-1", OrderStatus::Cancelled),
            OrderObservationOrigin::Command,
            None,
            NOW_MS + 1,
        )
        .await
        .is_none());
    let records = restarted.records("alpha").await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, NotificationKind::OrderFilled);
    assert_eq!(restarted.service.revision().await, revision_before);
    assert_eq!(std::fs::read(&restarted.path).unwrap(), disk_before);
}

#[tokio::test]
async fn submission_rejection_and_later_order_stream_are_one_absolute_event() {
    let harness = ObserverHarness::new("submission-order-alias");
    let context = SubmissionContext {
        submission_id: uuid::Uuid::new_v4().to_string(),
        account_id: "alpha".into(),
        session_epoch: 1,
    };
    assert!(harness
        .observer
        .observe_rejection(&context, Some(&context.submission_id), NOW_MS)
        .await
        .is_some());
    let path = harness.path.clone();
    std::mem::forget(harness);
    let restarted = ObserverHarness::load(path);
    let mut stream_order = order("order-alias-1", OrderStatus::New);
    stream_order.order_link_id = Some(context.submission_id.clone());
    assert!(restarted
        .observer
        .observe(
            "alpha",
            1,
            &stream_order,
            OrderObservationOrigin::Realtime,
            None,
            NOW_MS + 1,
        )
        .await
        .is_none());
    stream_order.status = OrderStatus::Rejected;
    assert!(restarted
        .observer
        .observe(
            "alpha",
            1,
            &stream_order,
            OrderObservationOrigin::Realtime,
            None,
            NOW_MS + 2,
        )
        .await
        .is_none());
    let records = restarted.records("alpha").await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].occurrence_count, 1);
}

#[tokio::test]
async fn ws_rejection_without_submission_uses_real_order_id_identity() {
    let harness = ObserverHarness::new("rejected-real-order-id");
    let rejected = order("order-real-identity-1", OrderStatus::Rejected);

    assert!(harness
        .observer
        .observe(
            "alpha",
            1,
            &rejected,
            OrderObservationOrigin::Command,
            None,
            NOW_MS,
        )
        .await
        .is_some());
    let records = harness.records("alpha").await;
    assert_eq!(records.len(), 1);
    let source = records[0].source_event_id.as_deref().unwrap();
    assert!(source.starts_with("order:terminal:o"));
    assert!(!source.contains("order-real-identity-1"));
    assert_eq!(
        records[0].dedupe_key,
        "alpha:order-real-identity-1:rejected"
    );
}

#[tokio::test]
async fn stale_or_wrong_account_ws_context_is_dropped_before_persistence() {
    let harness = ObserverHarness::new("session-switch");
    let coordinator = AccountLifecycleCoordinator::new();
    let config = tokio::sync::RwLock::new(AppConfig {
        active_account_id: "alpha".into(),
        accounts: vec!["alpha".into(), "beta".into()],
        ..Default::default()
    });
    let alpha_context = OrderStreamContext {
        account_id: "alpha".into(),
        session_epoch: coordinator.current_session_epoch(),
    };

    {
        let _guard = coordinator.mutation_guard().await;
        config.write().await.active_account_id = "beta".into();
        coordinator.advance_session_epoch();
    }

    assert!(harness
        .observer
        .observe_realtime_for_session(
            &coordinator,
            &config,
            &alpha_context,
            &order("order-stale-1", OrderStatus::Filled),
            NOW_MS,
        )
        .await
        .is_none());
    let wrong_account_current_epoch = OrderStreamContext {
        account_id: "alpha".into(),
        session_epoch: coordinator.current_session_epoch(),
    };
    assert!(harness
        .observer
        .observe_realtime_for_session(
            &coordinator,
            &config,
            &wrong_account_current_epoch,
            &order("order-wrong-account-1", OrderStatus::Filled),
            NOW_MS + 1,
        )
        .await
        .is_none());
    {
        let _guard = coordinator.mutation_guard().await;
        config.write().await.active_account_id = "alpha".into();
        coordinator.advance_session_epoch();
    }
    assert!(harness
        .observer
        .observe_realtime_for_session(
            &coordinator,
            &config,
            &alpha_context,
            &order("order-stale-a-b-a", OrderStatus::Filled),
            NOW_MS + 2,
        )
        .await
        .is_none());
    assert!(harness.records("alpha").await.is_empty());
    assert!(harness.records("beta").await.is_empty());
}

#[test]
fn create_order_rejection_classification_is_structural_and_fail_closed() {
    assert_eq!(
        classify_create_order_failure(&json!({
            "code": 0,
            "message": "multilingual provider text",
            "data": { "order_status": "Rejected" }
        })),
        Some(TradingFailureKind::Rejected),
    );
    for payload in [
        json!({"code": 901234, "message": "订单被拒绝"}),
        json!({"code": 901234, "data": {"orderStatus": "Rejected"}}),
        json!({"code": 101253, "message": "insufficient margin"}),
        json!({"data": {"orderStatus": "Rejected"}}),
        json!({"status": "failed", "message": "rejected"}),
        json!({"code": 429, "message": "rate limit"}),
        json!({"code": 500, "message": "server error"}),
        json!({"code": 26200003, "data": {"orderStatus": "Rejected"}}),
        json!({"code": 429, "data": {"orderStatus": "Rejected"}}),
        json!({"code": 500, "data": {"orderStatus": "Rejected"}}),
        json!({}),
    ] {
        assert_eq!(classify_create_order_failure(&payload), None);
    }
}
