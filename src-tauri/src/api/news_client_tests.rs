use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use url::Url;

use super::{
    NewsFetchErrorKind, NewsPageFetcher, TgForwarderNewsClient, ValidatedNewsMessage,
    ValidatedNewsPage,
};
use crate::storage::NewsApiToken;

const FAKE_TOKEN: &str = "fake-news-token-value";
const VALID_TIME: &str = "2024-01-01T00:00:00Z";

#[tokio::test]
async fn sends_exact_public_get_query_and_bearer_and_normalizes_unsorted_items() {
    let server = TestServer::spawn(TestResponse::json(
        200,
        json!({
            "ok": true,
            "data": {
                "items": [
                    wire_item(5, "2024-01-01T00:00:02Z", "five"),
                    wire_item(3, VALID_TIME, "three"),
                    wire_item(4, "2024-01-01T00:00:01Z", "four")
                ],
                "next_cursor": 5,
                "has_more": true
            },
            "meta": {"request_id": "ignored"},
            "unknown_wrapper": "ignored"
        }),
    ));
    let client = client_for(&server);

    let page = client
        .fetch_after(&token(), 2, 3)
        .await
        .expect("valid response");

    assert_eq!(
        page,
        ValidatedNewsPage {
            items: vec![
                validated(3, 1_704_067_200_000, "three"),
                validated(4, 1_704_067_201_000, "four"),
                validated(5, 1_704_067_202_000, "five"),
            ],
            next_cursor: 5,
            has_more: true,
        }
    );
    let request = server.request();
    assert_eq!(
        request.lines().next(),
        Some("GET /api/public/v1/messages?cursor=2&limit=3 HTTP/1.1")
    );
    let authorization = request
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("authorization:"))
        .expect("authorization header");
    assert_eq!(
        authorization
            .split_once(':')
            .expect("header separator")
            .1
            .trim(),
        format!("Bearer {FAKE_TOKEN}")
    );
}

#[tokio::test]
async fn accepts_empty_terminal_page_and_unknown_upstream_item_fields() {
    let item_server = TestServer::spawn(TestResponse::json(
        200,
        json!({
            "ok": true,
            "data": {
                "items": [{
                    "delivery_id": 8,
                    "created_at": VALID_TIME,
                    "text": "safe",
                    "source_backend": "must be ignored",
                    "source_chat_id": 123,
                    "media": [{"url": "ignored"}],
                    "user": {"name": "ignored"}
                }],
                "next_cursor": 8,
                "has_more": false,
                "unknown_page": true
            }
        }),
    ));
    let item_page = client_for(&item_server)
        .fetch_after(&token(), 7, 1)
        .await
        .expect("unknown fields are ignored");
    assert_eq!(
        item_page.items,
        vec![validated(8, 1_704_067_200_000, "safe")]
    );

    let empty_server = TestServer::spawn(TestResponse::json(
        200,
        json!({
            "ok": true,
            "data": {"items": [], "next_cursor": 8, "has_more": false}
        }),
    ));
    let empty_page = client_for(&empty_server)
        .fetch_after(&token(), 8, 100)
        .await
        .expect("valid empty page");
    assert_eq!(
        empty_page,
        ValidatedNewsPage {
            items: vec![],
            next_cursor: 8,
            has_more: false,
        }
    );
}

#[tokio::test]
async fn trait_is_object_safe_and_rejects_invalid_request_bounds_without_network() {
    let base_url = Url::parse("http://127.0.0.1:9/").expect("base URL");
    let client = TgForwarderNewsClient::new(base_url).expect("build client");
    let fetcher: Box<dyn NewsPageFetcher> = Box::new(client);

    for (cursor, limit) in [(-1, 1), (0, 0), (0, 101)] {
        let error = fetcher
            .fetch_after(&token(), cursor, limit)
            .await
            .expect_err("invalid request bounds");
        assert_eq!(error.kind(), NewsFetchErrorKind::Contract);
    }
}

#[tokio::test]
async fn rejects_non_integer_nonpositive_overflow_duplicate_and_stale_item_ids() {
    let cases = [
        (json!("3"), json!(3), 0, "string id"),
        (json!(3.5), json!(3), 0, "float id"),
        (json!(0), json!(0), 0, "zero id"),
        (json!(-3), json!(-3), 0, "negative id"),
        (
            json!(9_223_372_036_854_775_808_u64),
            json!(1),
            0,
            "overflow id",
        ),
        (json!(4), json!(4), 4, "id not above cursor"),
    ];

    for (delivery_id, next_cursor, cursor, label) in cases {
        let body = json!({
            "ok": true,
            "data": {
                "items": [{"delivery_id": delivery_id, "created_at": VALID_TIME, "text": "x"}],
                "next_cursor": next_cursor,
                "has_more": false
            }
        });
        assert_contract(body, cursor, 1, label).await;
    }

    assert_contract(
        json!({
            "ok": true,
            "data": {
                "items": [wire_item(2, VALID_TIME, "a"), wire_item(2, VALID_TIME, "b")],
                "next_cursor": 2,
                "has_more": false
            }
        }),
        0,
        2,
        "duplicate id",
    )
    .await;
}

#[tokio::test]
async fn rejects_invalid_cursor_and_page_progress_rules() {
    let cursor_cases = [
        (json!("2"), "string cursor"),
        (json!(2.5), "float cursor"),
        (json!(-1), "negative cursor"),
        (json!(9_223_372_036_854_775_808_u64), "overflow cursor"),
    ];
    for (next_cursor, label) in cursor_cases {
        assert_contract(
            json!({
                "ok": true,
                "data": {"items": [wire_item(2, VALID_TIME, "x")], "next_cursor": next_cursor, "has_more": false}
            }),
            0,
            1,
            label,
        )
        .await;
    }

    let page_cases = [
        (
            json!({"items": [wire_item(2, VALID_TIME, "x")], "next_cursor": 3, "has_more": false}),
            0,
            1,
            "nonempty cursor differs from max",
        ),
        (
            json!({"items": [wire_item(2, VALID_TIME, "x")], "next_cursor": 1, "has_more": false}),
            1,
            1,
            "nonempty cursor stalls",
        ),
        (
            json!({"items": [], "next_cursor": 6, "has_more": false}),
            5,
            1,
            "empty cursor advances",
        ),
        (
            json!({"items": [], "next_cursor": 5, "has_more": true}),
            5,
            1,
            "empty page claims more",
        ),
        (
            json!({"items": [wire_item(2, VALID_TIME, "a"), wire_item(3, VALID_TIME, "b")], "next_cursor": 3, "has_more": false}),
            0,
            1,
            "page exceeds requested limit",
        ),
    ];
    for (data, cursor, limit, label) in page_cases {
        assert_contract(json!({"ok": true, "data": data}), cursor, limit, label).await;
    }
}

#[tokio::test]
async fn normalizes_rfc3339_offsets_to_utc_milliseconds_and_rejects_invalid_times() {
    let server = TestServer::spawn(TestResponse::json(
        200,
        json!({
            "ok": true,
            "data": {
                "items": [
                    wire_item(1, "2024-01-01T00:00:00Z", "utc"),
                    wire_item(2, "2024-01-01T08:00:00+08:00", "offset")
                ],
                "next_cursor": 2,
                "has_more": false
            }
        }),
    ));
    let page = client_for(&server)
        .fetch_after(&token(), 0, 2)
        .await
        .expect("valid RFC3339 times");
    assert_eq!(page.items[0].created_at_ms, 1_704_067_200_000);
    assert_eq!(page.items[1].created_at_ms, 1_704_067_200_000);

    for (created_at, label) in [
        (json!("2024-01-01T00:00:00"), "zone-less time"),
        (json!("not-a-time"), "invalid time"),
        (json!(null), "null time"),
        (json!(123), "non-string time"),
        (json!("+999999999-01-01T00:00:00Z"), "overflow time"),
    ] {
        assert_contract(
            json!({
                "ok": true,
                "data": {
                    "items": [{"delivery_id": 1, "created_at": created_at, "text": "x"}],
                    "next_cursor": 1,
                    "has_more": false
                }
            }),
            0,
            1,
            label,
        )
        .await;
    }
}

#[tokio::test]
async fn validates_text_type_whitespace_and_utf8_byte_limit_without_truncating() {
    let whitespace_server = TestServer::spawn(TestResponse::json(
        200,
        json!({
            "ok": true,
            "data": {
                "items": [wire_item(1, VALID_TIME, " \r\n\t")],
                "next_cursor": 1,
                "has_more": false
            }
        }),
    ));
    let whitespace_page = client_for(&whitespace_server)
        .fetch_after(&token(), 0, 1)
        .await
        .expect("whitespace-only text is normalized");
    assert_eq!(whitespace_page.items[0].text, "");

    let at_limit = "é".repeat((256 * 1024) / 2);
    let at_limit_server = TestServer::spawn(TestResponse::json(
        200,
        json!({
            "ok": true,
            "data": {
                "items": [wire_item(1, VALID_TIME, &at_limit)],
                "next_cursor": 1,
                "has_more": false
            }
        }),
    ));
    let page = client_for(&at_limit_server)
        .fetch_after(&token(), 0, 1)
        .await
        .expect("text at byte limit");
    assert_eq!(page.items[0].text.len(), 256 * 1024);

    let over_limit = format!("{at_limit}x");
    assert_contract(
        json!({
            "ok": true,
            "data": {
                "items": [wire_item(1, VALID_TIME, &over_limit)],
                "next_cursor": 1,
                "has_more": false
            }
        }),
        0,
        1,
        "text over byte limit",
    )
    .await;

    for (item, label) in [
        (
            json!({"delivery_id": 1, "created_at": VALID_TIME}),
            "missing text",
        ),
        (wire_item_value(1, VALID_TIME, json!(null)), "null text"),
        (
            wire_item_value(1, VALID_TIME, json!(123)),
            "non-string text",
        ),
    ] {
        assert_contract(
            json!({
                "ok": true,
                "data": {"items": [item], "next_cursor": 1, "has_more": false}
            }),
            0,
            1,
            label,
        )
        .await;
    }
}

#[tokio::test]
async fn rejects_invalid_wrapper_json_and_streamed_body_over_eight_mib() {
    for (response, label) in [
        (
            TestResponse::json(200, json!({"ok": false, "data": null})),
            "ok false",
        ),
        (TestResponse::json(200, json!({"ok": true})), "missing data"),
        (TestResponse::raw(200, b"not-json".to_vec()), "non-json"),
    ] {
        let server = TestServer::spawn(response);
        let error = client_for(&server)
            .fetch_after(&token(), 0, 1)
            .await
            .expect_err(label);
        assert_eq!(error.kind(), NewsFetchErrorKind::Contract, "{label}");
    }

    let oversized = vec![b'x'; 8 * 1024 * 1024 + 1];
    let server = TestServer::spawn(TestResponse::chunked(200, oversized));
    let error = client_for(&server)
        .fetch_after(&token(), 0, 1)
        .await
        .expect_err("streaming body cap");
    assert_eq!(error.kind(), NewsFetchErrorKind::Contract);
}

#[tokio::test]
async fn maps_statuses_and_network_failures_to_stable_error_kinds() {
    for (status, expected) in [
        (401, NewsFetchErrorKind::CredentialInvalid),
        (408, NewsFetchErrorKind::Transient),
        (422, NewsFetchErrorKind::Contract),
        (400, NewsFetchErrorKind::Contract),
        (429, NewsFetchErrorKind::Transient),
        (500, NewsFetchErrorKind::Transient),
        (503, NewsFetchErrorKind::Transient),
    ] {
        let server = TestServer::spawn(TestResponse::raw(status, b"unsafe body".to_vec()));
        let error = client_for(&server)
            .fetch_after(&token(), 0, 1)
            .await
            .expect_err("status error");
        assert_eq!(error.kind(), expected, "HTTP {status}");
    }

    let client = TgForwarderNewsClient::with_timeout_policy(
        Url::parse("http://127.0.0.1:9/").expect("base URL"),
        Duration::from_millis(50),
        Duration::from_millis(100),
    )
    .expect("build client");
    let error = client
        .fetch_after(&token(), 0, 1)
        .await
        .expect_err("connection failure");
    assert_eq!(error.kind(), NewsFetchErrorKind::Transient);
}

#[tokio::test]
async fn disables_same_origin_and_cross_origin_redirects() {
    for location in [
        "/api/public/v1/messages?cursor=0&limit=1",
        "http://127.0.0.1:9/stolen",
    ] {
        let server =
            TestServer::spawn(TestResponse::raw(302, Vec::new()).with_header("Location", location));
        let error = client_for(&server)
            .fetch_after(&token(), 0, 1)
            .await
            .expect_err("redirect must not be followed");
        assert_eq!(error.kind(), NewsFetchErrorKind::Contract);
    }
}

#[tokio::test]
async fn preserves_valid_retry_after_seconds_and_http_date_only_for_429_or_503() {
    let seconds_server =
        TestServer::spawn(TestResponse::raw(429, Vec::new()).with_header("Retry-After", "17"));
    let seconds_error = client_for(&seconds_server)
        .fetch_after(&token(), 0, 1)
        .await
        .expect_err("429");
    assert_eq!(seconds_error.retry_after(), Some(Duration::from_secs(17)));

    let date_server = TestServer::spawn(
        TestResponse::raw(503, Vec::new())
            .with_header("Retry-After", "Wed, 21 Oct 2099 07:28:00 GMT"),
    );
    let date_error = client_for(&date_server)
        .fetch_after(&token(), 0, 1)
        .await
        .expect_err("503");
    assert!(date_error.retry_after().is_some());

    for retry_after in ["invalid", "Sun, 06 Nov 1994 08:49:37 GMT"] {
        let server = TestServer::spawn(
            TestResponse::raw(429, Vec::new()).with_header("Retry-After", retry_after),
        );
        let error = client_for(&server)
            .fetch_after(&token(), 0, 1)
            .await
            .expect_err("429");
        assert_eq!(error.retry_after(), None);
    }

    let unauthorized =
        TestServer::spawn(TestResponse::raw(401, Vec::new()).with_header("Retry-After", "17"));
    let unauthorized_error = client_for(&unauthorized)
        .fetch_after(&token(), 0, 1)
        .await
        .expect_err("401");
    assert_eq!(unauthorized_error.retry_after(), None);
}

#[tokio::test]
async fn total_request_timeout_is_transient_and_error_output_redacts_sensitive_inputs() {
    let delayed = TestServer::spawn(
        TestResponse::json(
            200,
            json!({
                "ok": true,
                "data": {"items": [], "next_cursor": 0, "has_more": false}
            }),
        )
        .with_delay(Duration::from_millis(150)),
    );
    let client = TgForwarderNewsClient::with_timeout_policy(
        delayed.base_url(),
        Duration::from_millis(30),
        Duration::from_millis(40),
    )
    .expect("build client");
    let error = client
        .fetch_after(&token(), 0, 1)
        .await
        .expect_err("total timeout");
    assert_eq!(error.kind(), NewsFetchErrorKind::Transient);

    let secret_body = "raw-upstream-secret-body";
    let server = TestServer::spawn(TestResponse::raw(422, secret_body.as_bytes().to_vec()));
    let error = client_for(&server)
        .fetch_after(&token(), 0, 1)
        .await
        .expect_err("contract status");
    for rendered in [format!("{error}"), format!("{error:?}")] {
        assert!(!rendered.contains(FAKE_TOKEN));
        assert!(!rendered.contains(secret_body));
        assert!(!rendered.contains("Authorization"));
        assert!(!rendered.contains(server.base_url().as_str()));
        assert!(!rendered.to_ascii_lowercase().contains("keyring"));
    }
}

async fn assert_contract(body: Value, cursor: i64, limit: usize, label: &str) {
    let server = TestServer::spawn(TestResponse::json(200, body));
    let error = client_for(&server)
        .fetch_after(&token(), cursor, limit)
        .await
        .expect_err(label);
    assert_eq!(error.kind(), NewsFetchErrorKind::Contract, "{label}");
}

fn client_for(server: &TestServer) -> TgForwarderNewsClient {
    TgForwarderNewsClient::new(server.base_url()).expect("build news client")
}

fn token() -> NewsApiToken {
    NewsApiToken::parse(FAKE_TOKEN.as_bytes()).expect("valid fake token")
}

fn wire_item(delivery_id: i64, created_at: &str, text: &str) -> Value {
    wire_item_value(delivery_id, created_at, json!(text))
}

fn wire_item_value(delivery_id: i64, created_at: &str, text: Value) -> Value {
    json!({"delivery_id": delivery_id, "created_at": created_at, "text": text})
}

fn validated(delivery_id: i64, created_at_ms: i64, text: &str) -> ValidatedNewsMessage {
    ValidatedNewsMessage {
        delivery_id,
        created_at_ms,
        text: text.to_owned(),
    }
}

struct TestResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    delay: Duration,
    chunked: bool,
}

impl TestResponse {
    fn json(status: u16, body: Value) -> Self {
        Self::raw(
            status,
            serde_json::to_vec(&body).expect("serialize test response"),
        )
        .with_header("Content-Type", "application/json")
    }

    fn raw(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body,
            delay: Duration::ZERO,
            chunked: false,
        }
    }

    fn chunked(status: u16, body: Vec<u8>) -> Self {
        Self {
            chunked: true,
            ..Self::raw(status, body)
        }
    }

    fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

struct TestServer {
    base_url: Url,
    request_rx: mpsc::Receiver<String>,
    handle: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn spawn(response: TestResponse) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind isolated test server");
        let address = listener.local_addr().expect("test server address");
        let (request_tx, request_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 2048];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).expect("read test request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            request_tx
                .send(String::from_utf8_lossy(&request).into_owned())
                .expect("publish test request");
            thread::sleep(response.delay);

            let reason = match response.status {
                200 => "OK",
                302 => "Found",
                400 => "Bad Request",
                401 => "Unauthorized",
                408 => "Request Timeout",
                422 => "Unprocessable Entity",
                429 => "Too Many Requests",
                500 => "Internal Server Error",
                503 => "Service Unavailable",
                _ => "Test Status",
            };
            let framing = if response.chunked {
                "Transfer-Encoding: chunked\r\n".to_owned()
            } else {
                format!("Content-Length: {}\r\n", response.body.len())
            };
            let extra_headers = response
                .headers
                .iter()
                .map(|(name, value)| format!("{name}: {value}\r\n"))
                .collect::<String>();
            let head = format!(
                "HTTP/1.1 {} {}\r\nConnection: close\r\n{}{}\r\n",
                response.status, reason, framing, extra_headers
            );
            if stream.write_all(head.as_bytes()).is_err() {
                return;
            }
            if response.chunked {
                for chunk in response.body.chunks(64 * 1024) {
                    if write!(stream, "{:X}\r\n", chunk.len()).is_err()
                        || stream.write_all(chunk).is_err()
                        || stream.write_all(b"\r\n").is_err()
                    {
                        return;
                    }
                }
                let _ = stream.write_all(b"0\r\n\r\n");
            } else {
                let _ = stream.write_all(&response.body);
            }
        });

        Self {
            base_url: Url::parse(&format!("http://{address}/")).expect("test base URL"),
            request_rx,
            handle: Some(handle),
        }
    }

    fn base_url(&self) -> Url {
        self.base_url.clone()
    }

    fn request(&self) -> String {
        self.request_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("captured request")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.join().expect("test server thread");
        }
    }
}
