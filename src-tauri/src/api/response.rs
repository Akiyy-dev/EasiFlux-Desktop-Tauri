use serde_json::Value;

use crate::models::trading::{OrderStatus, TradingFailureKind};

const SUCCESS_CODES: &[&str] = &["0", "200", "SUCCESS", "success"];

const LIST_KEYS: &[&str] = &[
    "list",
    "items",
    "records",
    "orders",
    "positions",
    "balances",
    "tickers",
    "kline",
    "klines",
    "fills",
    "rows",
    "result",
    "dataList",
    "orderList",
    "positionList",
    "order_list",
    "position_list",
];

#[derive(Debug, Clone)]
pub struct ListEnvelopeMeta {
    pub hint: String,
    pub raw_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CreateOrderOutcome<'a> {
    Accepted(&'a Value),
    Rejected,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFailureKind {
    SessionExpired,
    MissingCredential,
    CredentialStorage,
    SigningConfiguration,
    Timestamp,
    Signature,
    AccessDenied,
    RateLimited,
    Other,
}

pub fn classify_auth_failure(http_status: Option<u16>, payload: &Value) -> AuthFailureKind {
    if http_status == Some(403) {
        return AuthFailureKind::RateLimited;
    }
    match payload.get("code").and_then(Value::as_i64) {
        Some(26200003 | 20011005) => AuthFailureKind::SessionExpired,
        Some(26200002) => AuthFailureKind::Timestamp,
        Some(26200004) => AuthFailureKind::Signature,
        Some(26200005 | 26200008 | 26200010) => AuthFailureKind::AccessDenied,
        Some(26200006 | 26200018) => AuthFailureKind::RateLimited,
        _ => AuthFailureKind::Other,
    }
}

pub fn extract_data(payload: &Value) -> &Value {
    payload.get("data").unwrap_or(payload)
}

pub fn extract_list(payload: &Value) -> Vec<&Value> {
    extract_list_with_meta(payload).0
}

pub fn extract_list_with_meta(payload: &Value) -> (Vec<&Value>, ListEnvelopeMeta) {
    let data = extract_data(payload);
    if let Some(arr) = data.as_array() {
        let count = arr.len();
        return (
            arr.iter().collect(),
            ListEnvelopeMeta {
                hint: "data[]".into(),
                raw_count: count,
            },
        );
    }
    if let Some(obj) = data.as_object() {
        for key in LIST_KEYS {
            if let Some(arr) = obj.get(*key).and_then(|v| v.as_array()) {
                let count = arr.len();
                return (
                    arr.iter().collect(),
                    ListEnvelopeMeta {
                        hint: format!("data.{key}"),
                        raw_count: count,
                    },
                );
            }
        }
        if let Some((items, hint)) = find_best_object_array(data) {
            let count = items.len();
            return (
                items,
                ListEnvelopeMeta {
                    hint,
                    raw_count: count,
                },
            );
        }
        if is_entity_object(obj) {
            return (
                vec![data],
                ListEnvelopeMeta {
                    hint: "data(entity)".into(),
                    raw_count: 1,
                },
            );
        }
    }
    (
        vec![],
        ListEnvelopeMeta {
            hint: "empty".into(),
            raw_count: 0,
        },
    )
}

fn is_entity_object(obj: &serde_json::Map<String, Value>) -> bool {
    obj.contains_key("orderId")
        || obj.contains_key("order_id")
        || obj.contains_key("orderLinkId")
        || obj.contains_key("order_link_id")
        || (obj.contains_key("symbol") && (obj.contains_key("side") || obj.contains_key("size")))
        || (obj.contains_key("symbol")
            && (obj.contains_key("position_idx") || obj.contains_key("positionIdx")))
}

fn find_best_object_array<'a>(value: &'a Value) -> Option<(Vec<&'a Value>, String)> {
    let mut best: Option<(&'a Vec<Value>, String, usize)> = None;
    collect_object_arrays(value, "data", &mut best);
    best.map(|(arr, hint, _)| (arr.iter().collect(), hint))
}

fn collect_object_arrays<'a>(
    value: &'a Value,
    path: &str,
    best: &mut Option<(&'a Vec<Value>, String, usize)>,
) {
    match value {
        Value::Array(arr) => {
            let object_count = arr.iter().filter(|item| item.is_object()).count();
            if object_count == 0 {
                return;
            }
            let replace = best
                .as_ref()
                .map(|(_, _, count)| object_count > *count)
                .unwrap_or(true);
            if replace {
                *best = Some((arr, path.to_string(), object_count));
            }
        }
        Value::Object(map) => {
            for (key, child) in map {
                if matches!(
                    key.as_str(),
                    "symbol" | "interval" | "coin" | "cursor" | "total"
                ) {
                    continue;
                }
                collect_object_arrays(child, &format!("{path}.{key}"), best);
            }
        }
        _ => {}
    }
}

pub fn payload_has_content(payload: &Value) -> bool {
    if payload.is_null() {
        return false;
    }
    if let Some(obj) = payload.as_object() {
        return !obj.is_empty();
    }
    !payload.as_array().map(|a| a.is_empty()).unwrap_or(false)
}

pub fn first_object_keys(payload: &Value) -> Vec<String> {
    let items = extract_list(payload);
    items
        .first()
        .and_then(|v| v.as_object())
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default()
}

pub fn get_str(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(v) = value.get(*key) {
            if let Some(s) = v.as_str() {
                return Some(s.to_string());
            }
            if v.is_number() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn code_matches_success(code: &Value) -> bool {
    if let Some(n) = code.as_i64() {
        return n == 0 || n == 200;
    }
    if let Some(s) = code.as_str() {
        return SUCCESS_CODES.contains(&s);
    }
    false
}

pub fn is_success_response(payload: &Value) -> bool {
    for field in ["code", "errorCode", "status"] {
        if let Some(code) = payload.get(field) {
            return code_matches_success(code);
        }
    }
    true
}

pub fn response_code(payload: &Value) -> Option<String> {
    for field in ["code", "errorCode", "status"] {
        if let Some(code) = payload.get(field) {
            if let Some(n) = code.as_i64() {
                return Some(n.to_string());
            }
            if let Some(s) = code.as_str() {
                return Some(s.to_string());
            }
        }
    }
    None
}

pub fn is_auth_error(payload: &Value) -> bool {
    payload
        .get("code")
        .and_then(Value::as_i64)
        .is_some_and(|code| {
            matches!(
                code,
                26200002 | 26200003 | 26200004 | 26200005 | 26200008 | 26200010 | 20011005
            )
        })
}

pub fn is_rate_limit_error(payload: &Value) -> bool {
    matches!(
        payload.get("code").and_then(Value::as_i64),
        Some(26200006 | 26200018)
    )
}

pub fn is_timestamp_error(payload: &Value) -> bool {
    payload.get("code").and_then(Value::as_i64) == Some(26200002)
}

pub fn is_sign_error(payload: &Value) -> bool {
    payload.get("code").and_then(Value::as_i64) == Some(26200004)
}

pub fn error_message(payload: &Value) -> Option<String> {
    for key in ["msg", "message", "error", "detail", "errorMessage"] {
        if let Some(msg) = payload.get(key).and_then(|v| v.as_str()) {
            if !msg.is_empty() {
                return Some(msg.to_string());
            }
        }
    }
    None
}

/// Selects the exact create-order candidate consumed by the parser and
/// classifies it only beneath an explicit successful response envelope.
/// Provider codes and localized messages are intentionally not interpreted.
pub fn classify_create_order_outcome(payload: &Value) -> CreateOrderOutcome<'_> {
    let Some(code) = payload.get("code") else {
        return CreateOrderOutcome::Ambiguous;
    };
    if code.as_i64() != Some(0) {
        return CreateOrderOutcome::Ambiguous;
    }
    let Some(candidate) = payload.get("data").filter(|data| data.is_object()) else {
        return CreateOrderOutcome::Ambiguous;
    };
    let status = ["status", "orderStatus", "order_status"]
        .into_iter()
        .find_map(|key| candidate.get(key).and_then(Value::as_str));
    if status.is_some_and(|status| OrderStatus::from_raw(status) == OrderStatus::Rejected) {
        CreateOrderOutcome::Rejected
    } else if candidate
        .get("order_id")
        .and_then(Value::as_str)
        .is_some_and(|order_id| !order_id.trim().is_empty())
    {
        CreateOrderOutcome::Accepted(candidate)
    } else {
        CreateOrderOutcome::Ambiguous
    }
}

pub fn classify_create_order_failure(payload: &Value) -> Option<TradingFailureKind> {
    (classify_create_order_outcome(payload) == CreateOrderOutcome::Rejected)
        .then_some(TradingFailureKind::Rejected)
}

pub fn describe_data_shape(payload: &Value) -> (String, Vec<String>) {
    let data = extract_data(payload);
    match data {
        Value::Array(_arr) => ("array".into(), vec![]),
        Value::Object(obj) => ("object".into(), obj.keys().cloned().collect()),
        other => (format!("{}", other), vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unwrap_data_envelope() {
        let payload = json!({"data": {"symbol": "BTCUSDT"}});
        let data = extract_data(&payload);
        assert_eq!(data["symbol"], "BTCUSDT");
    }

    #[test]
    fn success_code_200() {
        assert!(is_success_response(&json!({"code": 200})));
        assert!(is_success_response(&json!({"code": "SUCCESS"})));
    }

    #[test]
    fn timestamp_error_detection() {
        assert!(is_timestamp_error(
            &json!({"code": 26200002, "msg": "timestamp"})
        ));
    }

    #[test]
    fn auth_failure_classification_uses_only_documented_exact_codes() {
        let cases = [
            (26200003, AuthFailureKind::SessionExpired),
            (20011005, AuthFailureKind::SessionExpired),
            (26200002, AuthFailureKind::Timestamp),
            (26200004, AuthFailureKind::Signature),
            (26200005, AuthFailureKind::AccessDenied),
            (26200008, AuthFailureKind::AccessDenied),
            (26200010, AuthFailureKind::AccessDenied),
            (26200006, AuthFailureKind::RateLimited),
            (26200018, AuthFailureKind::RateLimited),
        ];
        for (code, expected) in cases {
            assert_eq!(
                classify_auth_failure(None, &json!({"code": code, "message": "expired 已过期"})),
                expected,
                "code {code}",
            );
        }

        for payload in [
            json!({"code": 99999999, "message": "session expired"}),
            json!({"status": "SESSION_EXPIRED", "message": "会话已过期"}),
            json!({"status": 26200003, "message": "会话已过期"}),
            json!({"errorCode": 26200003, "message": "会话已过期"}),
            json!({"code": "26200003", "message": "会话已过期"}),
            json!({"message": "invalid api_key"}),
        ] {
            assert_eq!(
                classify_auth_failure(Some(401), &payload),
                AuthFailureKind::Other
            );
        }
        assert_ne!(
            classify_auth_failure(Some(403), &json!({"message": "expired"})),
            AuthFailureKind::SessionExpired,
        );
        for code in [26200003, 26200002, 26200004, 99999999] {
            assert_eq!(
                classify_auth_failure(Some(403), &json!({"code": code})),
                AuthFailureKind::RateLimited,
            );
        }
    }

    #[test]
    fn auth_failure_classification_keeps_transient_and_local_failures_distinct() {
        assert_eq!(
            classify_auth_failure(None, &json!({"code": 26200002})),
            AuthFailureKind::Timestamp
        );
        assert_eq!(
            classify_auth_failure(None, &json!({"code": 26200003})),
            AuthFailureKind::SessionExpired
        );
        assert_eq!(
            classify_auth_failure(None, &json!({"code": 26200006})),
            AuthFailureKind::RateLimited
        );
        assert_eq!(
            classify_auth_failure(Some(403), &json!({"message": "forbidden"})),
            AuthFailureKind::RateLimited
        );
        for status in [401, 429] {
            assert_eq!(
                classify_auth_failure(Some(status), &json!({"message": "session expired"})),
                AuthFailureKind::Other
            );
        }
    }

    #[test]
    fn extract_list_supports_rows_key() {
        let payload = json!({"data": {"rows": [{"symbol": "BTCUSDT"}]}});
        let (items, meta) = extract_list_with_meta(&payload);
        assert_eq!(items.len(), 1);
        assert_eq!(meta.hint, "data.rows");
    }

    #[test]
    fn extract_list_skips_empty_data_object() {
        let payload = json!({"code": 0, "data": {}});
        let (items, meta) = extract_list_with_meta(&payload);
        assert!(items.is_empty());
        assert_eq!(meta.hint, "empty");
    }

    #[test]
    fn extract_list_finds_nested_array() {
        let payload = json!({
            "code": 0,
            "data": {
                "page": 1,
                "result": {
                    "items": [{"orderId": "1", "symbol": "BTCUSDT"}]
                }
            }
        });
        let (items, meta) = extract_list_with_meta(&payload);
        assert_eq!(items.len(), 1);
        assert!(meta.hint.contains("items"));
    }
}
