use serde::Serialize;
use std::fmt::Display;
use thiserror::Error;

use crate::api::response::AuthFailureKind;
use crate::models::trading::TradingFailure;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationCause {
    AuthFailure(AuthFailureKind),
}

#[derive(Debug, Clone, Error)]
pub enum AppError {
    #[error("认证失败: {0}")]
    Auth(String),
    #[error("认证失败")]
    AuthFailure(AuthFailureKind),
    #[error("连接错误: {0}")]
    Connection(String),
    #[error("交易错误: {0}")]
    Trading(String),
    #[error("交易错误: {0}")]
    TradingFailure(TradingFailure),
    #[error("风控拒绝: {0}")]
    Risk(String),
    #[error("配置错误: {0}")]
    Config(String),
    #[error("存储错误: {0}")]
    Storage(String),
    #[error("未连接")]
    NotConnected,
    #[error("内部错误: {0}")]
    Internal(String),
    #[error("{message}")]
    Notified {
        code: &'static str,
        message: &'static str,
        notification_id: String,
        cause: Option<NotificationCause>,
    },
    #[error("{0}")]
    Observed(&'static str),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Notified {
                code,
                message,
                notification_id,
                ..
            } => {
                use serde::ser::SerializeStruct;
                let mut value = serializer.serialize_struct("CommandError", 3)?;
                value.serialize_field("code", code)?;
                value.serialize_field("message", message)?;
                value.serialize_field("notificationId", notification_id)?;
                value.end()
            }
            _ => serializer.serialize_str(&self.to_string()),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

impl From<reqwest::Error> for AppError {
    fn from(_value: reqwest::Error) -> Self {
        AppError::Connection("网络请求失败".into())
    }
}

impl From<keyring::Error> for AppError {
    fn from(value: keyring::Error) -> Self {
        map_keyring_error(value)
    }
}

pub(crate) fn credential_backend_error(_reason: impl Display) -> AppError {
    AppError::Auth("系统凭据存储不可用，请稍后重试".into())
}

pub(crate) fn map_keyring_error(error: keyring::Error) -> AppError {
    tracing::warn!(
        error_kind = keyring_error_kind(&error),
        "系统凭据存储操作失败"
    );
    credential_backend_error(error)
}

fn keyring_error_kind(error: &keyring::Error) -> &'static str {
    match error {
        keyring::Error::PlatformFailure(_) => "platform_failure",
        keyring::Error::NoStorageAccess(_) => "no_storage_access",
        keyring::Error::NoEntry => "no_entry",
        keyring::Error::BadEncoding(_) => "bad_encoding",
        keyring::Error::TooLong(_, _) => "attribute_too_long",
        keyring::Error::Invalid(_, _) => "invalid_attribute",
        keyring::Error::Ambiguous(_) => "ambiguous_entry",
        _ => "unknown",
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        AppError::Internal(value.to_string())
    }
}

impl From<toml::de::Error> for AppError {
    fn from(value: toml::de::Error) -> Self {
        AppError::Config(value.to_string())
    }
}

impl From<toml::ser::Error> for AppError {
    fn from(value: toml::ser::Error) -> Self {
        AppError::Config(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_private_detail_is_hidden(error: &AppError, private_detail: &str) {
        assert!(matches!(error, AppError::Auth(_)));
        for rendered in [
            error.to_string(),
            error.user_message(),
            serde_json::to_string(error).unwrap(),
        ] {
            assert!(!rendered.contains(private_detail));
            assert!(!rendered.contains("raw-key"));
            assert!(!rendered.contains("raw-secret"));
        }
    }

    #[test]
    fn keyring_backend_detail_is_never_exposed_to_the_ui() {
        const PRIVATE_DETAIL: &str =
            "synthetic backend failure: credential=raw api_key=raw-key secret=raw-secret";
        let backend = std::io::Error::other(PRIVATE_DETAIL);

        let error = AppError::from(keyring::Error::PlatformFailure(Box::new(backend)));

        assert_private_detail_is_hidden(&error, PRIVATE_DETAIL);
    }

    #[test]
    fn credential_backend_mapper_ignores_display_detail() {
        const PRIVATE_DETAIL: &str = "backend api_key=raw-key secret=raw-secret";

        let error = credential_backend_error(PRIVATE_DETAIL);

        assert_private_detail_is_hidden(&error, PRIVATE_DETAIL);
    }

    #[test]
    fn notified_command_error_has_a_stable_object_shape_while_ordinary_errors_stay_strings() {
        let notification_id = uuid::Uuid::new_v4().to_string();
        let notified = AppError::Notified {
            code: "ORDER_REJECTED",
            message: "订单请求被交易端拒绝",
            notification_id: notification_id.clone(),
            cause: None,
        };
        assert_eq!(
            serde_json::to_value(notified).unwrap(),
            serde_json::json!({
                "code": "ORDER_REJECTED",
                "message": "订单请求被交易端拒绝",
                "notificationId": notification_id,
            })
        );
        assert!(serde_json::to_value(AppError::Connection("safe".into()))
            .unwrap()
            .is_string());
    }
}
