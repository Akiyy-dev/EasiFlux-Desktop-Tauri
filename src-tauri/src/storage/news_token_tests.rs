use super::*;
use keyring::credential::CredentialPersistence;
use keyring::mock::MockCredential;
use keyring::{Entry, Error as KeyringError};

fn token(value: &[u8]) -> NewsApiToken {
    NewsApiToken::parse(value).expect("valid news token")
}

fn mock_store() -> KeyringNewsTokenStore {
    let entry = Entry::new_with_credential(Box::<MockCredential>::default());
    KeyringNewsTokenStore::from_entry(entry)
}

#[test]
fn credential_identity_is_isolated_from_trading_credentials() {
    assert_eq!(NEWS_KEYRING_SERVICE, "easiflux_desktop_tauri_news");
    assert_eq!(NEWS_KEYRING_ENTRY, "news_api_token");
}

#[test]
fn debug_is_always_redacted() {
    let value = token(b"plain-secret-token");

    assert_eq!(format!("{value:?}"), "NewsApiToken(<redacted>)");
}

#[test]
fn parser_accepts_non_prefixed_tokens_and_removes_only_final_line_endings() {
    let value = token(b"plain-token\r\n\n");

    assert_eq!(value.as_str(), "plain-token");
    assert_eq!(
        NewsApiToken::parse(b" plain-token ").unwrap_err(),
        NewsTokenError::InvalidBearerHeader
    );
}

#[test]
fn parser_accepts_the_maximum_size() {
    let input = vec![b'a'; MAX_NEWS_TOKEN_BYTES];

    assert_eq!(token(&input).as_str().len(), MAX_NEWS_TOKEN_BYTES);
}

#[test]
fn parser_rejects_empty_or_line_ending_only_input() {
    assert_eq!(NewsApiToken::parse(b"").unwrap_err(), NewsTokenError::Empty);
    assert_eq!(
        NewsApiToken::parse(b"\r\n").unwrap_err(),
        NewsTokenError::Empty
    );
}

#[test]
fn parser_rejects_input_over_the_byte_limit() {
    let input = vec![b'a'; MAX_NEWS_TOKEN_BYTES + 1];

    assert_eq!(
        NewsApiToken::parse(&input).unwrap_err(),
        NewsTokenError::TooLarge
    );
}

#[test]
fn parser_rejects_ascii_and_unicode_control_characters() {
    assert_eq!(
        NewsApiToken::parse(b"abc\tdef").unwrap_err(),
        NewsTokenError::ControlCharacter
    );
    assert_eq!(
        NewsApiToken::parse("abc\u{0085}def".as_bytes()).unwrap_err(),
        NewsTokenError::ControlCharacter
    );
}

#[test]
fn parser_rejects_invalid_utf8_and_non_bearer_header_values() {
    assert_eq!(
        NewsApiToken::parse(&[0xff]).unwrap_err(),
        NewsTokenError::InvalidBearerHeader
    );
    assert_eq!(
        NewsApiToken::parse("tökén".as_bytes()).unwrap_err(),
        NewsTokenError::InvalidBearerHeader
    );
    assert_eq!(
        NewsApiToken::parse(b"=").unwrap_err(),
        NewsTokenError::InvalidBearerHeader
    );
}

#[test]
fn injected_entry_supports_set_load_overwrite_delete_and_missing_entry() {
    let store = mock_store();
    assert!(store.load().expect("load missing entry").is_none());

    store.set(&token(b"first")).expect("set first token");
    assert_eq!(
        store.load().expect("load first token").unwrap().as_str(),
        "first"
    );

    store.set(&token(b"second")).expect("overwrite token");
    assert_eq!(
        store.load().expect("load second token").unwrap().as_str(),
        "second"
    );

    store.delete().expect("delete token");
    assert!(store.load().expect("load deleted entry").is_none());
    store.delete().expect("delete missing entry is idempotent");
}

#[test]
fn backend_errors_have_stable_category_only_text() {
    let store = mock_store();
    let mock = store
        .entry
        .get_credential()
        .downcast_ref::<MockCredential>()
        .expect("mock credential");
    mock.set_error(KeyringError::Invalid(
        "raw backend detail".into(),
        "raw secret detail".into(),
    ));

    let error = store.load().unwrap_err();
    let display = error.to_string();
    assert_eq!(display, "keyring failure");
    assert!(!display.contains("backend"));
    assert!(!display.contains("secret"));
}

#[test]
fn configured_native_default_persists_until_explicit_delete() {
    assert!(matches!(
        keyring::default::default_credential_builder().persistence(),
        CredentialPersistence::UntilDelete
    ));
}

#[test]
fn store_contract_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<KeyringNewsTokenStore>();
}
