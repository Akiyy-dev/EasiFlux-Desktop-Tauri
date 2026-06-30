use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("认证失败: {0}")]
    Auth(String),
    #[error("连接错误: {0}")]
    Connection(String),
    #[error("交易错误: {0}")]
    Trading(String),
    #[error("风控拒绝: {0}")]
    Risk(String),
    #[error("配置错误: {0}")]
    Config(String),
    #[error("未连接")]
    NotConnected,
    #[error("内部错误: {0}")]
    Internal(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

impl From<reqwest::Error> for AppError {
    fn from(value: reqwest::Error) -> Self {
        AppError::Connection(value.to_string())
    }
}

impl From<keyring::Error> for AppError {
    fn from(value: keyring::Error) -> Self {
        AppError::Config(value.to_string())
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
