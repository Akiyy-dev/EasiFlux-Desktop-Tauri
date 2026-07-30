#[path = "../build_support/news_build_config.rs"]
mod news_build_config;

use news_build_config::validate_news_build_config;

const VALID_TOKEN: &str = "news-token_123";

#[test]
fn permits_absent_configuration_for_non_release_profiles() {
    for profile in ["debug", "test"] {
        assert_eq!(
            validate_news_build_config(profile, None, None, None).unwrap(),
            None,
            "{profile} should permit omitted news configuration"
        );
    }
}

#[test]
fn rejects_partial_configuration() {
    let cases = [
        ("debug", Some("https://news.example.test"), None, None),
        ("test", None, Some("source-20260730"), None),
        ("debug", None, None, Some(VALID_TOKEN)),
        (
            "release",
            Some("https://news.example.test"),
            Some("source-20260730"),
            None,
        ),
        (
            "release",
            Some("https://news.example.test"),
            None,
            Some(VALID_TOKEN),
        ),
        ("release", None, Some("source-20260730"), Some(VALID_TOKEN)),
    ];

    for (profile, base_url, source_epoch, api_token) in cases {
        assert!(
            validate_news_build_config(profile, base_url, source_epoch, api_token).is_err(),
            "{profile} should reject partial configuration"
        );
    }
}

#[test]
fn accepts_http_only_for_loopback_hosts_outside_release() {
    let cases = [
        ("http://localhost/news", "http://localhost/news/"),
        ("http://127.0.0.1:8787", "http://127.0.0.1:8787/"),
        ("http://[::1]/api", "http://[::1]/api/"),
    ];

    for (base_url, normalized_url) in cases {
        let config =
            validate_news_build_config("debug", Some(base_url), Some("epoch_1"), Some(VALID_TOKEN))
                .expect("loopback HTTP should be accepted")
                .expect("configured values should be emitted");
        assert_eq!(config.api_base_url, normalized_url);
    }
}

#[test]
fn rejects_non_loopback_http_and_all_release_http() {
    for (profile, base_url) in [
        ("debug", "http://news.example.test"),
        ("release", "http://localhost/news"),
    ] {
        assert!(
            validate_news_build_config(
                profile,
                Some(base_url),
                Some("epoch_1"),
                Some(VALID_TOKEN),
            )
            .is_err(),
            "{profile} should reject {base_url}"
        );
    }
}

#[test]
fn release_requires_complete_https_configuration() {
    assert!(validate_news_build_config("release", None, None, None).is_err());

    let config = validate_news_build_config(
        "release",
        Some("https://news.example.test/v1"),
        Some("release-20260730"),
        Some(VALID_TOKEN),
    )
    .expect("release HTTPS configuration should be accepted")
    .expect("configured release values should be emitted");
    assert_eq!(config.api_base_url, "https://news.example.test/v1/");
    assert_eq!(config.source_epoch, "release-20260730");
}

#[test]
fn rejects_url_credentials_query_and_fragment() {
    for base_url in [
        "https://user:password@news.example.test/",
        "https://news.example.test/?token=secret",
        "https://news.example.test/#fragment",
    ] {
        assert!(
            validate_news_build_config(
                "debug",
                Some(base_url),
                Some("epoch_1"),
                Some(VALID_TOKEN),
            )
            .is_err(),
            "must reject unsafe URL {base_url}"
        );
    }
}

#[test]
fn validates_source_epoch() {
    for epoch in ["A", "source.2026_07-30", &"a".repeat(64)] {
        assert!(
            validate_news_build_config(
                "debug",
                Some("https://news.example.test"),
                Some(epoch),
                Some(VALID_TOKEN),
            )
            .is_ok(),
            "should accept {epoch:?}"
        );
    }

    for epoch in ["", "contains space", "contains/slash", &"a".repeat(65)] {
        assert!(
            validate_news_build_config(
                "debug",
                Some("https://news.example.test"),
                Some(epoch),
                Some(VALID_TOKEN),
            )
            .is_err(),
            "should reject {epoch:?}"
        );
    }
}

#[test]
fn normalizes_base_url_with_a_trailing_slash() {
    let config = validate_news_build_config(
        "debug",
        Some("https://news.example.test/api/v1"),
        Some("epoch_1"),
        Some(VALID_TOKEN),
    )
    .unwrap()
    .unwrap();

    assert_eq!(config.api_base_url, "https://news.example.test/api/v1/");
}

#[test]
fn error_display_is_category_only_and_redacts_rejected_input() {
    let rejected_url = "https://user:secret@news.example.test/?token=top-secret";
    let error = validate_news_build_config(
        "release",
        Some(rejected_url),
        Some("epoch_1"),
        Some(VALID_TOKEN),
    )
    .expect_err("unsafe release URL should fail validation");

    assert_eq!(
        error.to_string(),
        "news build configuration error: invalid URL"
    );
    assert!(!error.to_string().contains(rejected_url));
    assert!(!error.to_string().contains("secret"));
    assert!(!error.to_string().contains("top-secret"));
}

#[test]
fn validates_tokens_with_the_runtime_bearer_rules() {
    for token in [
        "plain-token",
        "abc.DEF_123~+/==",
        "line-ending-token\r\n",
        &"a".repeat(16 * 1024),
    ] {
        assert!(
            validate_news_build_config(
                "debug",
                Some("https://news.example.test"),
                Some("epoch_1"),
                Some(token),
            )
            .is_ok(),
            "should accept a valid bearer token"
        );
    }

    for token in [
        "",
        " token ",
        "abc=def",
        "=",
        "abc\tdef",
        "t\u{00f6}k\u{00e9}n",
        &"a".repeat(16 * 1024 + 1),
    ] {
        assert!(
            validate_news_build_config(
                "debug",
                Some("https://news.example.test"),
                Some("epoch_1"),
                Some(token),
            )
            .is_err(),
            "should reject an invalid bearer token"
        );
    }
}

#[test]
fn token_validation_errors_are_category_only_and_never_expose_input() {
    let rejected_token = "super-secret token=do-not-print";
    let error = validate_news_build_config(
        "release",
        Some("https://news.example.test"),
        Some("epoch_1"),
        Some(rejected_token),
    )
    .expect_err("invalid release token should fail validation");
    let display = error.to_string();
    let debug = format!("{error:?}");

    assert_eq!(display, "news build configuration error: invalid API token");
    for secret in [rejected_token, "super-secret", "do-not-print"] {
        assert!(!display.contains(secret));
        assert!(!debug.contains(secret));
    }
}

#[test]
fn build_script_tracks_inputs_without_reprinting_secret_values() {
    let build_script = include_str!("../build.rs");

    for variable in [
        "EASIFLUX_NEWS_API_BASE_URL",
        "EASIFLUX_NEWS_SOURCE_EPOCH",
        "EASIFLUX_NEWS_API_TOKEN",
    ] {
        assert!(
            build_script.contains(&format!("cargo:rerun-if-env-changed={variable}")),
            "build script should track {variable}"
        );
    }
    assert!(
        !build_script.contains("cargo:rustc-env=EASIFLUX_NEWS"),
        "option_env! should read the inherited build environment directly"
    );
}
