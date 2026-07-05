use serde_json::Value;

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
        return (
            vec![data],
            ListEnvelopeMeta {
                hint: "data(object)".into(),
                raw_count: 1,
            },
        );
    }
    (
        vec![],
        ListEnvelopeMeta {
            hint: "empty".into(),
            raw_count: 0,
        },
    )
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
    if let Some(code) = response_code(payload) {
        if matches!(
            code.as_str(),
            "26200002" | "26200003" | "26200004" | "26200005" | "26200010" | "20011005"
        ) {
            return true;
        }
    }
    let msg = error_message(payload).unwrap_or_default().to_lowercase();
    msg.contains("timestamp") || msg.contains("recv_window")
}

pub fn is_rate_limit_error(payload: &Value) -> bool {
    if let Some(code) = response_code(payload) {
        if matches!(code.as_str(), "26200006" | "26200018" | "10200616") {
            return true;
        }
    }
    false
}

pub fn is_timestamp_error(payload: &Value) -> bool {
    if let Some(code) = response_code(payload) {
        if code == "26200002" {
            return true;
        }
    }
    let msg = error_message(payload).unwrap_or_default().to_lowercase();
    msg.contains("timestamp") || msg.contains("recv_window")
}

pub fn is_sign_error(payload: &Value) -> bool {
    if let Some(code) = response_code(payload) {
        if matches!(code.as_str(), "26200003" | "26200004" | "26200005" | "20011005") {
            return true;
        }
    }
    let msg = error_message(payload).unwrap_or_default().to_lowercase();
    msg.contains("error sign") || msg.contains("invalid signature") || msg.contains("sign!")
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
        assert!(is_timestamp_error(&json!({"code": 26200002, "msg": "timestamp"})));
    }

    #[test]
    fn extract_list_supports_rows_key() {
        let payload = json!({"data": {"rows": [{"symbol": "BTCUSDT"}]}});
        let (items, meta) = extract_list_with_meta(&payload);
        assert_eq!(items.len(), 1);
        assert_eq!(meta.hint, "data.rows");
    }

    #[test]
    fn extract_list_supports_position_list_key() {
        let payload = json!({"data": {"positionList": [{"symbol": "ETHUSDT"}]}});
        let (items, meta) = extract_list_with_meta(&payload);
        assert_eq!(items.len(), 1);
        assert_eq!(meta.hint, "data.positionList");
    }
}
