use std::fmt;
use std::time::{Duration, SystemTime};

use reqwest::header::HeaderValue;
use reqwest::StatusCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsFetchErrorKind {
    Transient,
    CredentialInvalid,
    Contract,
}

#[derive(Clone, PartialEq, Eq)]
pub struct NewsFetchError {
    kind: NewsFetchErrorKind,
    retry_after: Option<Duration>,
}

impl NewsFetchError {
    pub(crate) fn new(kind: NewsFetchErrorKind) -> Self {
        Self {
            kind,
            retry_after: None,
        }
    }

    pub(crate) fn with_retry_after(
        kind: NewsFetchErrorKind,
        retry_after: Option<Duration>,
    ) -> Self {
        Self { kind, retry_after }
    }

    pub fn kind(&self) -> NewsFetchErrorKind {
        self.kind
    }

    pub fn retry_after(&self) -> Option<Duration> {
        self.retry_after
    }

    fn category(&self) -> &'static str {
        match self.kind {
            NewsFetchErrorKind::Transient => "transient",
            NewsFetchErrorKind::CredentialInvalid => "credential invalid",
            NewsFetchErrorKind::Contract => "contract",
        }
    }
}

impl fmt::Debug for NewsFetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NewsFetchError")
            .field("kind", &self.kind)
            .field("retry_after", &self.retry_after)
            .finish()
    }
}

impl fmt::Display for NewsFetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "news fetch failed: {}", self.category())
    }
}

impl std::error::Error for NewsFetchError {}

pub(super) fn classify_status(status: StatusCode) -> NewsFetchErrorKind {
    if status == StatusCode::UNAUTHORIZED {
        NewsFetchErrorKind::CredentialInvalid
    } else if status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
    {
        NewsFetchErrorKind::Transient
    } else {
        NewsFetchErrorKind::Contract
    }
}

pub(super) fn parse_retry_after(header: &HeaderValue) -> Option<Duration> {
    let value = header.to_str().ok()?;
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let retry_at = httpdate::parse_http_date(value).ok()?;
    retry_at
        .duration_since(SystemTime::now())
        .ok()
        .filter(|duration| !duration.is_zero())
}
