use serde_json::Value;

const SUCCESS_CODES: &[&str] = &["0", "200", "SUCCESS", "success"];

pub fn extract_data(payload: &Value) -> &Value {
    payload.get("data").unwrap_or(payload)
}

pub fn extract_list(payload: &Value) -> Vec<&Value> {
    let data = extract_data(payload);
    if let Some(arr) = data.as_array() {
        return arr.iter().collect();
    }
    if let Some(obj) = data.as_object() {
        for key in [
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
        ] {
            if let Some(arr) = obj.get(key).and_then(|v| v.as_array()) {
                return arr.iter().collect();
            }
        }
        return vec![data];
    }
    vec![]
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
}
