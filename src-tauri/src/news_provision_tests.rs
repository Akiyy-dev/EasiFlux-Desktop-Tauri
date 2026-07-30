use super::*;
use crate::storage::{NewsApiToken, NewsTokenStore, NewsTokenStoreError};
use std::ffi::OsString;
use std::io::Cursor;
use std::sync::Mutex;

#[cfg(unix)]
fn non_unicode_argument() -> OsString {
    use std::os::unix::ffi::OsStringExt;

    OsString::from_vec(vec![0xff])
}

#[cfg(windows)]
fn non_unicode_argument() -> OsString {
    use std::os::windows::ffi::OsStringExt;

    OsString::from_wide(&[0xd800])
}

#[derive(Default)]
struct FakeStore {
    calls: Mutex<Vec<String>>,
    configured: Mutex<bool>,
    fail: Mutex<bool>,
}

impl FakeStore {
    fn with_configured(configured: bool) -> Self {
        Self {
            configured: Mutex::new(configured),
            ..Self::default()
        }
    }

    fn failing() -> Self {
        Self {
            fail: Mutex::new(true),
            ..Self::default()
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("calls lock").clone()
    }

    fn should_fail(&self) -> bool {
        *self.fail.lock().expect("fail lock")
    }
}

impl NewsTokenStore for FakeStore {
    fn load(&self) -> Result<Option<NewsApiToken>, NewsTokenStoreError> {
        self.calls.lock().expect("calls lock").push("load".into());
        if self.should_fail() {
            return Err(NewsTokenStoreError::Keyring);
        }
        if *self.configured.lock().expect("configured lock") {
            Ok(Some(NewsApiToken::parse(b"stored").expect("valid token")))
        } else {
            Ok(None)
        }
    }

    fn set(&self, token: &NewsApiToken) -> Result<(), NewsTokenStoreError> {
        self.calls
            .lock()
            .expect("calls lock")
            .push(format!("set:{}", token.as_str()));
        if self.should_fail() {
            return Err(NewsTokenStoreError::Keyring);
        }
        *self.configured.lock().expect("configured lock") = true;
        Ok(())
    }

    fn delete(&self) -> Result<(), NewsTokenStoreError> {
        self.calls.lock().expect("calls lock").push("delete".into());
        if self.should_fail() {
            return Err(NewsTokenStoreError::Keyring);
        }
        *self.configured.lock().expect("configured lock") = false;
        Ok(())
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn invoke(input: &[u8], arguments: &[&str], store: &FakeStore) -> (i32, String, String) {
    let mut reader = Cursor::new(input);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run(
        &args(arguments),
        &mut reader,
        store,
        &mut stdout,
        &mut stderr,
    );
    (
        code,
        String::from_utf8(stdout).expect("utf8 stdout"),
        String::from_utf8(stderr).expect("utf8 stderr"),
    )
}

#[test]
fn os_arguments_reject_non_unicode_and_non_ascii_without_panicking() {
    assert!(parse_os_arguments([non_unicode_argument()]).is_err());
    assert!(parse_os_arguments([OsString::from("státus")]).is_err());
    assert_eq!(
        parse_os_arguments([OsString::from("status")]).expect("ASCII status command"),
        vec!["status"]
    );
}

#[test]
fn set_reads_stdin_and_reports_configured_without_echoing_token() {
    let store = FakeStore::default();
    let secret = "never-print-this-token";

    let (code, stdout, stderr) = invoke(format!("{secret}\r\n").as_bytes(), &["set"], &store);

    assert_eq!(code, EXIT_SUCCESS);
    assert_eq!(stdout, "configured\n");
    assert!(stderr.is_empty());
    assert_eq!(store.calls(), vec![format!("set:{secret}")]);
    assert!(!stdout.contains(secret));
    assert!(!stderr.contains(secret));
}

#[test]
fn repeated_set_overwrites_without_load_or_delete() {
    let store = FakeStore::default();

    assert_eq!(invoke(b"first", &["set"], &store).0, EXIT_SUCCESS);
    assert_eq!(invoke(b"second", &["set"], &store).0, EXIT_SUCCESS);

    assert_eq!(store.calls(), vec!["set:first", "set:second"]);
}

#[test]
fn set_rejects_arguments_and_invalid_tokens_without_touching_store() {
    for (input, arguments) in [
        (&b"secret"[..], vec!["set", "secret"]),
        (&b""[..], vec!["set"]),
        (&b"bad\ttoken"[..], vec!["set"]),
        (&[0xff][..], vec!["set"]),
    ] {
        let store = FakeStore::default();
        let (code, stdout, stderr) = invoke(input, &arguments, &store);
        assert_eq!(code, EXIT_USAGE_OR_INVALID_TOKEN);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "invalid usage or token\n");
        assert!(store.calls().is_empty());
    }
}

#[test]
fn set_bounded_read_rejects_oversized_input() {
    let store = FakeStore::default();
    let input = vec![b'a'; crate::storage::news_token::MAX_NEWS_TOKEN_BYTES + 1];

    let (code, stdout, stderr) = invoke(&input, &["set"], &store);

    assert_eq!(code, EXIT_USAGE_OR_INVALID_TOKEN);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "invalid usage or token\n");
    assert!(store.calls().is_empty());
}

#[test]
fn bounded_secret_input_has_full_capacity_before_it_contains_token_bytes() {
    let mut reader = Cursor::new(b"token");

    let input = read_bounded_input(&mut reader).expect("bounded input");

    assert_eq!(input.as_slice(), b"token");
    assert!(
        input.capacity() > crate::storage::news_token::MAX_NEWS_TOKEN_BYTES,
        "bounded input must never reallocate while reading a maximum-size secret"
    );

    let limit = crate::storage::news_token::MAX_NEWS_TOKEN_BYTES + 1;
    let maximum_input = vec![b'a'; limit];
    let mut reader = Cursor::new(maximum_input);
    let input = read_bounded_input(&mut reader).expect("maximum bounded input");
    assert_eq!(input.len(), limit);
    assert_eq!(
        input.capacity(),
        limit,
        "reading through the bound must retain the original allocation"
    );
}

#[test]
fn status_reports_only_configured_or_not_configured() {
    let configured = FakeStore::with_configured(true);
    assert_eq!(
        invoke(b"ignored", &["status"], &configured),
        (EXIT_SUCCESS, "configured\n".into(), String::new())
    );
    assert_eq!(configured.calls(), vec!["load"]);

    let missing = FakeStore::default();
    assert_eq!(
        invoke(b"ignored", &["status"], &missing),
        (
            EXIT_NOT_CONFIGURED,
            "not-configured\n".into(),
            String::new()
        )
    );
    assert_eq!(missing.calls(), vec!["load"]);
}

#[test]
fn guarded_delete_requires_exact_service_and_entry() {
    let valid = FakeStore::with_configured(true);
    let (code, stdout, stderr) = invoke(
        b"ignored",
        &[
            "delete",
            "--service",
            "easiflux_desktop_tauri_news",
            "--entry",
            "news_api_token",
        ],
        &valid,
    );
    assert_eq!(
        (code, stdout, stderr),
        (EXIT_SUCCESS, "deleted\n".into(), String::new())
    );
    assert_eq!(valid.calls(), vec!["delete"]);

    for invalid in [
        vec!["delete"],
        vec![
            "delete",
            "--service",
            "easiflux_desktop_tauri",
            "--entry",
            "news_api_token",
        ],
        vec![
            "delete",
            "--service",
            "easiflux_desktop_tauri_news",
            "--entry",
            "account-id",
        ],
    ] {
        let store = FakeStore::with_configured(true);
        assert_eq!(
            invoke(b"ignored", &invalid, &store).0,
            EXIT_USAGE_OR_INVALID_TOKEN
        );
        assert!(store.calls().is_empty());
    }
}

#[test]
fn keyring_failures_use_exit_three_and_category_only_output() {
    for command in [
        vec!["set"],
        vec!["status"],
        vec![
            "delete",
            "--service",
            "easiflux_desktop_tauri_news",
            "--entry",
            "news_api_token",
        ],
    ] {
        let store = FakeStore::failing();
        let (code, stdout, stderr) = invoke(b"never-print-this-token", &command, &store);
        assert_eq!(code, EXIT_KEYRING_FAILURE);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "keyring failure\n");
        assert!(!stderr.contains("never-print-this-token"));
    }
}

#[test]
fn unknown_commands_return_usage_exit_without_touching_store() {
    for command in [vec![], vec!["unknown"], vec!["status", "extra"]] {
        let store = FakeStore::default();
        let (code, stdout, stderr) = invoke(b"ignored", &command, &store);
        assert_eq!(code, EXIT_USAGE_OR_INVALID_TOKEN);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "invalid usage or token\n");
        assert!(store.calls().is_empty());
    }
}
