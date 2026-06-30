use serde_json::Value;

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

pub fn is_success_response(payload: &Value) -> bool {
    if let Some(code) = payload.get("code") {
        if let Some(n) = code.as_i64() {
            return n == 0;
        }
        if let Some(s) = code.as_str() {
            return s == "0";
        }
    }
    true
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
    fn extract_list_from_nested() {
        let payload = json!({"data": {"list": [{"id": 1}, {"id": 2}]}});
        let items = extract_list(&payload);
        assert_eq!(items.len(), 2);
    }
}
