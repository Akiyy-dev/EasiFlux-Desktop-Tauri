use reqwest::header::HeaderValue;
use std::fmt;
use zeroize::Zeroizing;

pub const NEWS_KEYRING_SERVICE: &str = "easiflux_desktop_tauri_news";
pub const NEWS_KEYRING_ENTRY: &str = "news_api_token";
pub const MAX_NEWS_TOKEN_BYTES: usize = 16 * 1024;

pub struct NewsApiToken(Zeroizing<String>);

impl NewsApiToken {
    pub fn parse(input: &[u8]) -> Result<Self, NewsTokenError> {
        if input.len() > MAX_NEWS_TOKEN_BYTES {
            return Err(NewsTokenError::TooLarge);
        }

        let end = input
            .iter()
            .rposition(|byte| !matches!(byte, b'\r' | b'\n'))
            .map_or(0, |index| index + 1);
        let value =
            std::str::from_utf8(&input[..end]).map_err(|_| NewsTokenError::InvalidBearerHeader)?;

        if value.is_empty() {
            return Err(NewsTokenError::Empty);
        }
        if value.chars().any(char::is_control) {
            return Err(NewsTokenError::ControlCharacter);
        }
        if !is_bearer_token(value) || HeaderValue::from_str(value).is_err() {
            return Err(NewsTokenError::InvalidBearerHeader);
        }

        Ok(Self(Zeroizing::new(value.to_owned())))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

fn is_bearer_token(value: &str) -> bool {
    let mut padding = false;
    let mut token_character = false;
    let valid = value.bytes().all(|byte| {
        if byte == b'=' {
            padding = true;
            return true;
        }
        if padding {
            return false;
        }
        token_character = matches!(
            byte,
            b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'-'
                | b'.'
                | b'_'
                | b'~'
                | b'+'
                | b'/'
        );
        token_character
    });
    valid && token_character
}

impl fmt::Debug for NewsApiToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NewsApiToken(<redacted>)")
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum NewsTokenError {
    #[error("empty news token")]
    Empty,
    #[error("news token exceeds size limit")]
    TooLarge,
    #[error("news token contains a control character")]
    ControlCharacter,
    #[error("news token cannot form a bearer header")]
    InvalidBearerHeader,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum NewsTokenStoreError {
    #[error("keyring failure")]
    Keyring,
}

pub trait NewsTokenStore: Send + Sync {
    fn load(&self) -> Result<Option<NewsApiToken>, NewsTokenStoreError>;
    fn set(&self, token: &NewsApiToken) -> Result<(), NewsTokenStoreError>;
    fn delete(&self) -> Result<(), NewsTokenStoreError>;
}

pub struct KeyringNewsTokenStore {
    entry: keyring::Entry,
}

impl KeyringNewsTokenStore {
    pub fn new() -> Result<Self, NewsTokenStoreError> {
        keyring::Entry::new(NEWS_KEYRING_SERVICE, NEWS_KEYRING_ENTRY)
            .map(Self::from_entry)
            .map_err(|_| NewsTokenStoreError::Keyring)
    }

    pub(crate) fn from_entry(entry: keyring::Entry) -> Self {
        Self { entry }
    }
}

impl NewsTokenStore for KeyringNewsTokenStore {
    fn load(&self) -> Result<Option<NewsApiToken>, NewsTokenStoreError> {
        match self.entry.get_password() {
            Ok(value) => {
                let value = Zeroizing::new(value);
                NewsApiToken::parse(value.as_bytes())
                    .map(Some)
                    .map_err(|_| NewsTokenStoreError::Keyring)
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(NewsTokenStoreError::Keyring),
        }
    }

    fn set(&self, token: &NewsApiToken) -> Result<(), NewsTokenStoreError> {
        self.entry
            .set_password(token.as_str())
            .map_err(|_| NewsTokenStoreError::Keyring)
    }

    fn delete(&self) -> Result<(), NewsTokenStoreError> {
        match self.entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(NewsTokenStoreError::Keyring),
        }
    }
}

#[cfg(test)]
#[path = "news_token_tests.rs"]
mod tests;
