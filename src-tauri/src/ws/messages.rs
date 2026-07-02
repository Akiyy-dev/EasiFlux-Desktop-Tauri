use serde_json::{json, Value};

use crate::auth::Signer;

pub fn build_subscribe_message(topics: &[String]) -> Value {
    json!({
        "op": "subscribe",
        "args": topics,
    })
}

pub fn build_ping_message() -> Value {
    json!({ "op": "ping" })
}

pub fn build_auth_message(signer: &Signer, expires_ms: u64) -> Value {
    json!({
        "op": "auth",
        "args": [
            signer.api_key(),
            expires_ms,
            signer.sign_ws_auth(expires_ms),
        ]
    })
}

pub fn default_auth_expires_ms(now_ms: u64) -> u64 {
    now_ms + 60_000
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Signer;

    #[test]
    fn subscribe_message_format() {
        let msg = build_subscribe_message(&["tickers-100.BTCUSDT".to_string()]);
        assert_eq!(msg["op"], "subscribe");
        assert_eq!(msg["args"][0], "tickers-100.BTCUSDT");
    }

    #[test]
    fn auth_message_is_array_args() {
        let signer = Signer::new("key".into(), "secret".into());
        let msg = build_auth_message(&signer, 1_662_350_400_000);
        assert_eq!(msg["op"], "auth");
        assert!(msg["args"].is_array());
        assert_eq!(msg["args"][0], "key");
    }
}
