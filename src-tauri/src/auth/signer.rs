use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct Signer {
    api_key: String,
    api_secret: String,
}

impl Signer {
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self { api_key, api_secret }
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn sign(&self, payload: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(self.api_secret.as_bytes()).expect("HMAC key length");
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    pub fn prepare_headers(
        &self,
        timestamp_ms: u64,
        recv_window_ms: u64,
        signature_payload: &str,
    ) -> Vec<(String, String)> {
        let timestamp = timestamp_ms.to_string();
        let recv_window = recv_window_ms.to_string();
        let payload = format!(
            "{}{}{}{}",
            timestamp, self.api_key, recv_window, signature_payload
        );
        let signature = self.sign(&payload);
        vec![
            ("Access-Key".to_string(), self.api_key.clone()),
            ("Access-Sign".to_string(), signature),
            ("Access-Timestamp".to_string(), timestamp),
            ("Recv-Window".to_string(), recv_window),
        ]
    }

    pub fn ws_auth_sign(&self, timestamp: &str) -> String {
        self.sign(&format!("{}{}", timestamp, self.api_key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_is_deterministic() {
        let signer = Signer::new("test_key".to_string(), "test_secret".to_string());
        let sig1 = signer.sign("payload");
        let sig2 = signer.sign("payload");
        assert_eq!(sig1, sig2);
        assert_eq!(sig1.len(), 64);
    }

    #[test]
    fn ws_auth_payload_format() {
        let signer = Signer::new("mykey".to_string(), "mysecret".to_string());
        let sig = signer.ws_auth_sign("1234567890");
        assert_eq!(sig.len(), 64);
    }
}
