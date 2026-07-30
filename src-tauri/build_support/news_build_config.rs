use std::fmt;
use std::net::Ipv4Addr;

use url::{Host, Url};

const MAX_API_TOKEN_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsBuildConfig {
    pub api_base_url: String,
    pub source_epoch: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsBuildConfigError {
    MissingConfiguration,
    PartialConfiguration,
    InvalidUrl,
    InvalidSourceEpoch,
    InvalidApiToken,
}

impl fmt::Display for NewsBuildConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let category = match self {
            Self::MissingConfiguration => "missing configuration",
            Self::PartialConfiguration => "partial configuration",
            Self::InvalidUrl => "invalid URL",
            Self::InvalidSourceEpoch => "invalid source epoch",
            Self::InvalidApiToken => "invalid API token",
        };
        write!(formatter, "news build configuration error: {category}")
    }
}

impl std::error::Error for NewsBuildConfigError {}

pub fn validate_news_build_config(
    profile: &str,
    base_url: Option<&str>,
    source_epoch: Option<&str>,
    api_token: Option<&str>,
) -> Result<Option<NewsBuildConfig>, NewsBuildConfigError> {
    match (base_url, source_epoch, api_token) {
        (None, None, None) if profile != "release" => Ok(None),
        (None, None, None) => Err(NewsBuildConfigError::MissingConfiguration),
        (Some(base_url), Some(source_epoch), Some(api_token)) => {
            let api_base_url = normalize_url(profile, base_url)?;
            if !is_valid_source_epoch(source_epoch) {
                return Err(NewsBuildConfigError::InvalidSourceEpoch);
            }
            if !is_valid_api_token(api_token) {
                return Err(NewsBuildConfigError::InvalidApiToken);
            }

            Ok(Some(NewsBuildConfig {
                api_base_url,
                source_epoch: source_epoch.to_owned(),
            }))
        }
        _ => Err(NewsBuildConfigError::PartialConfiguration),
    }
}

fn normalize_url(profile: &str, base_url: &str) -> Result<String, NewsBuildConfigError> {
    let mut url = Url::parse(base_url).map_err(|_| NewsBuildConfigError::InvalidUrl)?;

    if url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host().is_none()
    {
        return Err(NewsBuildConfigError::InvalidUrl);
    }

    match url.scheme() {
        "https" => {}
        "http" if profile != "release" && is_loopback_host(&url) => {}
        _ => return Err(NewsBuildConfigError::InvalidUrl),
    }

    if !url.path().ends_with('/') {
        let normalized_path = format!("{}/", url.path());
        url.set_path(&normalized_path);
    }

    Ok(url.into())
}

fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(host)) => host == "localhost",
        Some(Host::Ipv4(address)) => address == Ipv4Addr::new(127, 0, 0, 1),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

fn is_valid_source_epoch(source_epoch: &str) -> bool {
    (1..=64).contains(&source_epoch.len())
        && source_epoch
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn is_valid_api_token(api_token: &str) -> bool {
    if api_token.len() > MAX_API_TOKEN_BYTES {
        return false;
    }
    let end = api_token
        .as_bytes()
        .iter()
        .rposition(|byte| !matches!(byte, b'\r' | b'\n'))
        .map_or(0, |index| index + 1);
    let value = &api_token[..end];
    if value.is_empty() || value.chars().any(char::is_control) {
        return false;
    }

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
