use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::super::*;
use crate::models::config::{ApiCredential, RiskConfig};
use crate::models::market::Ticker;
use crate::models::trading::TradingFailureKind;
use crate::services::notification::{NotificationAvailability, NotificationService};
use crate::storage::{NotificationStore, RiskUsageStore};

struct QuoteServer {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl QuoteServer {
    fn new(quote_status: u16, quote: Value) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let captured = Arc::clone(&requests);
        let finished = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            while !finished.load(Ordering::SeqCst) {
                let (mut socket, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("accept quote request: {error}"),
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let count = socket.read(&mut buffer).unwrap();
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    let Some(header_end) = bytes.windows(4).position(|value| value == b"\r\n\r\n")
                    else {
                        continue;
                    };
                    let header = String::from_utf8_lossy(&bytes[..header_end]);
                    let body_len = header
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= header_end + 4 + body_len {
                        break;
                    }
                }
                let request = String::from_utf8(bytes).unwrap();
                let line = request.lines().next().unwrap();
                let (status, payload) = if line.contains(endpoints::TICKER) {
                    (quote_status, quote.clone())
                } else if line.contains(endpoints::SERVER_TIME) {
                    (200, json!({"code": 0, "data": {"time": "1782850580"}}))
                } else {
                    // Never accept an order: these tests must not write real trade logs.
                    (200, json!({"code": 0, "data": {"orderStatus": "Rejected"}}))
                };
                captured.lock().unwrap().push(request);
                let body = payload.to_string();
                let reply = format!("HTTP/1.1 {status} TEST\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                socket.write_all(reply.as_bytes()).unwrap();
            }
        });
        Self {
            base_url,
            requests,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for QuoteServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let result = self.worker.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}

async fn place_with_quote(
    server: &QuoteServer,
    risk_enabled: bool,
    cached_price: Option<&str>,
    order_type: &str,
    reduce_only: bool,
) -> AppResult<Order> {
    place_with_quote_and_notifications(
        server,
        risk_enabled,
        cached_price,
        order_type,
        reduce_only,
        false,
    )
    .await
}

async fn place_with_quote_and_notifications(
    server: &QuoteServer,
    risk_enabled: bool,
    cached_price: Option<&str>,
    order_type: &str,
    reduce_only: bool,
    notifications_enabled: bool,
) -> AppResult<Order> {
    let api = Arc::new(ApiClient::new());
    api.set_credential(ApiCredential {
        api_key: "test-key".into(),
        api_secret: "test-secret".into(),
        base_url: server.base_url.clone(),
        label: "quote-test".into(),
    })
    .await;
    let path = std::env::temp_dir().join(format!("easiflux-quote-{}.toml", uuid::Uuid::new_v4()));
    let risk = Arc::new(tokio::sync::RwLock::new(RiskService::with_store(
        RiskConfig {
            enabled: risk_enabled,
            ..Default::default()
        },
        RiskUsageStore::with_path(path.clone()),
    )));
    let cache = Arc::new(CacheStore::new());
    if let Some(price) = cached_price {
        cache.set_ticker(Ticker {
            symbol: "BTCUSDT".into(),
            last_price: price.into(),
            ..Default::default()
        });
    }
    let emitter = EventEmitter::new_test(Arc::new(Mutex::new(Vec::new())));
    let notification_path = path.with_extension("json");
    let notification_runtime = if notifications_enabled {
        NotificationRuntime::Available(Arc::new(
            NotificationService::load(
                NotificationStore::with_path(notification_path.clone()),
                &["quote-account".into()],
                api.time_sync().timestamp_ms(),
                Arc::new(|_| Ok(())),
            )
            .unwrap(),
        ))
    } else {
        NotificationRuntime::Unavailable(NotificationAvailability::new("TEST", "unavailable"))
    };
    let service = TradingService::new(
        Arc::clone(&api),
        Arc::clone(&risk),
        Arc::new(TradeLogStore::new()),
        cache,
        emitter.clone(),
        Arc::new(TimeService::new(api.time_sync(), Arc::clone(&api), emitter)),
        Arc::new(AnalyticsService::new(api)),
        Arc::new(notification_runtime),
    );
    let request = PlaceOrderRequest {
        symbol: "BTCUSDT".into(),
        side: "Sell".into(),
        order_type: order_type.into(),
        qty: "1".into(),
        position_idx: 0,
        price: Some("100".into()),
        time_in_force: None,
        order_link_id: None,
        reduce_only: Some(reduce_only),
    };
    let result = service
        .place_order(
            SubmissionContext {
                account_id: "quote-account".into(),
                session_epoch: 1,
                submission_id: uuid::Uuid::new_v4().to_string(),
            },
            request,
        )
        .await;
    assert_eq!(
        risk.read()
            .await
            .status(service.time.now_ms())
            .occupied_orders,
        risk_enabled.then_some(0)
    );
    for suffix in ["", ".bak", ".tmp"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        let _ = std::fs::remove_file(format!("{}{suffix}", notification_path.display()));
    }
    result
}

#[tokio::test]
async fn notified_missing_quote_keeps_an_actionable_order_panel_message() {
    let server = QuoteServer::new(500, json!({"message": "unavailable"}));
    let result =
        place_with_quote_and_notifications(&server, true, Some("100"), "Limit", false, true).await;
    match result {
        Err(AppError::Notified {
            code,
            message,
            notification_id,
            ..
        }) => {
            assert_eq!(code, "RISK_ORDER_BLOCKED");
            assert!(!notification_id.is_empty());
            assert_eq!(message, "无法获取有效的最新市价，请稍后重试");
        }
        other => panic!("missing quote should retain its notification receipt: {other:?}"),
    }
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn limit_order_fetches_quote_from_active_api_when_cache_is_empty() {
    let server = QuoteServer::new(
        200,
        json!({"code": 0, "data": {"symbol": "BTCUSDT", "lastPrice": "100"}}),
    );
    let result = place_with_quote(&server, true, None, "Limit", false).await;
    assert!(
        matches!(result, Err(AppError::TradingFailure(ref failure)) if failure.kind == TradingFailureKind::Rejected),
        "valid fresh quote should reach order submission: {result:?}"
    );
    let requests = server.requests.lock().unwrap();
    assert!(requests[0].starts_with(&format!("GET {}?symbol=BTCUSDT ", endpoints::TICKER)));
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.starts_with("POST "))
            .count(),
        1
    );
}

#[tokio::test]
async fn limit_order_uses_fresh_quote_even_if_cache_would_allow_submission() {
    let server = QuoteServer::new(
        200,
        json!({"code": 0, "data": {"symbol": "BTCUSDT", "lastPrice": "200"}}),
    );
    let result = place_with_quote(&server, true, Some("100"), "Limit", false).await;
    assert!(
        matches!(result, Err(AppError::Risk(_))),
        "new quote must enforce deviation protection: {result:?}"
    );
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("GET "));
}

#[tokio::test]
async fn failed_or_invalid_remote_quotes_block_submission_despite_a_cached_price() {
    for (status, quote) in [
        (500, json!({"message": "unavailable"})),
        (200, json!({"code": 0, "data": []})),
        (
            200,
            json!({"code": 0, "data": {"symbol": "ETHUSDT", "lastPrice": "100"}}),
        ),
        (
            200,
            json!({"code": 0, "data": {"symbol": "BTCUSDT", "lastPrice": "0"}}),
        ),
        (
            200,
            json!({"code": 0, "data": {"symbol": "BTCUSDT", "lastPrice": "NaN"}}),
        ),
    ] {
        let server = QuoteServer::new(status, quote);
        let result = place_with_quote(&server, true, Some("100"), "Limit", true).await;
        assert!(
            matches!(result, Err(AppError::Risk(_))),
            "unusable quote must block limit close: {result:?}"
        );
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("GET "));
    }
}

#[tokio::test]
async fn market_close_and_disabled_risk_do_not_request_quotes() {
    for (enabled, order_type, closing) in [(true, "Market", true), (false, "Limit", false)] {
        let server = QuoteServer::new(500, json!({"message": "unavailable"}));
        let result = place_with_quote(&server, enabled, None, order_type, closing).await;
        assert!(
            matches!(result, Err(AppError::TradingFailure(ref failure)) if failure.kind == TradingFailureKind::Rejected)
        );
        let requests = server.requests.lock().unwrap();
        assert!(requests.iter().all(|request| !request
            .lines()
            .next()
            .unwrap()
            .contains(endpoints::TICKER)));
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.starts_with("POST "))
                .count(),
            1
        );
    }
}
