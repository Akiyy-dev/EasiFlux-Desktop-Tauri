use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use uuid::{Uuid, Version};

pub const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub const DEFAULT_NOTIFICATION_PAGE_LIMIT: u32 = 50;
pub const MAX_NOTIFICATION_PAGE_LIMIT: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettings {
    pub trading_toast: bool,
    pub risk_account_toast: bool,
    pub connection_system_toast: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            trading_toast: true,
            risk_account_toast: true,
            connection_system_toast: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationValidationError {
    code: &'static str,
    message: &'static str,
}

impl NotificationValidationError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }
}

impl Display for NotificationValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message)
    }
}

impl Error for NotificationValidationError {}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NotificationScope {
    Global,
    Account { account_id: String },
}

impl NotificationScope {
    pub fn validate(&self) -> Result<(), NotificationValidationError> {
        match self {
            Self::Global => Ok(()),
            Self::Account { account_id } if account_id.trim().is_empty() => {
                Err(NotificationValidationError::new(
                    "INVALID_NOTIFICATION_SCOPE",
                    "通知账户范围不能为空",
                ))
            }
            Self::Account { .. } => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationCategory {
    Trading,
    RiskAccount,
    ConnectionSystem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationKind {
    OrderFilled,
    OrderCanceled,
    OrderRejected,
    RiskOrderBlocked,
    AccountSessionExpired,
    AccountRecoveryFailed,
    AccountReconciliationFailed,
    ConnectionUnavailable,
    ConnectionRecovered,
    EnvironmentUnavailable,
    EnvironmentRecovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationSeverity {
    Success,
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NotificationScalar {
    String(String),
    Bool(bool),
    Number(f64),
}

impl NotificationScalar {
    fn validate(&self) -> Result<(), NotificationValidationError> {
        match self {
            Self::String(value) if !is_safe_scalar_string(value) => {
                return Err(NotificationValidationError::new(
                    "INVALID_NOTIFICATION_CONTENT",
                    "通知内容包含不安全字符串",
                ));
            }
            Self::Number(number) if !number.is_finite() => {
                return Err(NotificationValidationError::new(
                    "INVALID_NOTIFICATION_CONTENT",
                    "通知内容包含非有限数值",
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationMessageTemplate {
    OrderFilled,
    OrderCanceled,
    OrderRejected,
    RiskOrderBlocked,
    AccountSessionExpired,
    AccountRecoveryFailed,
    AccountReconciliationFailed,
    ConnectionUnavailable,
    ConnectionRecovered,
    EnvironmentUnavailable,
    EnvironmentRecovered,
}

impl NotificationMessageTemplate {
    fn from_message_key(value: &str) -> Option<Self> {
        Some(match value {
            "order.filled" => Self::OrderFilled,
            "order.canceled" => Self::OrderCanceled,
            "order.rejected" => Self::OrderRejected,
            "risk.orderBlocked" => Self::RiskOrderBlocked,
            "account.sessionExpired" => Self::AccountSessionExpired,
            "account.recoveryFailed" => Self::AccountRecoveryFailed,
            "account.reconciliationFailed" => Self::AccountReconciliationFailed,
            "connection.unavailable" => Self::ConnectionUnavailable,
            "connection.recovered" => Self::ConnectionRecovered,
            "environment.unavailable" => Self::EnvironmentUnavailable,
            "environment.recovered" => Self::EnvironmentRecovered,
            _ => return None,
        })
    }

    fn fallback(self) -> (&'static str, &'static str) {
        match self {
            Self::OrderFilled => ("订单已成交", "订单已完全成交，请前往交易页查看。"),
            Self::OrderCanceled => ("订单已取消", "订单已取消，请前往交易页查看。"),
            Self::OrderRejected => ("订单被拒绝", "订单请求被交易端拒绝，请检查订单参数。"),
            Self::RiskOrderBlocked => ("订单被风控拦截", "请检查风控设置"),
            Self::AccountSessionExpired => ("账户会话已失效", "请检查账户 API 设置。"),
            Self::AccountRecoveryFailed => ("账户恢复失败", "请检查账户设置后重试。"),
            Self::AccountReconciliationFailed => ("账户对账失败", "请检查账户数据后重试。"),
            Self::ConnectionUnavailable => {
                ("连接不可用", "交易连接暂时不可用，请检查网络或稍后重试。")
            }
            Self::ConnectionRecovered => ("连接已恢复", "交易连接已恢复。"),
            Self::EnvironmentUnavailable => ("环境不可达", "当前交易环境暂时不可达，请稍后重试。"),
            Self::EnvironmentRecovered => ("环境已恢复", "当前交易环境已恢复。"),
        }
    }

    fn accepts(self, key: NotificationParameterKey, value: &NotificationScalar) -> bool {
        match self {
            Self::OrderFilled | Self::OrderCanceled => {
                matches!(
                    (key, value),
                    (
                        NotificationParameterKey::OrderId,
                        NotificationScalar::String(_)
                    )
                )
            }
            Self::OrderRejected => matches!(
                (key, value),
                (
                    NotificationParameterKey::OrderId,
                    NotificationScalar::String(_)
                ) | (
                    NotificationParameterKey::SubmissionId,
                    NotificationScalar::String(_)
                )
            ),
            Self::RiskOrderBlocked => matches!(
                (key, value),
                (
                    NotificationParameterKey::Limit,
                    NotificationScalar::Number(_)
                ) | (
                    NotificationParameterKey::ViolationCode,
                    NotificationScalar::String(_)
                )
            ),
            Self::AccountSessionExpired => {
                matches!(
                    (key, value),
                    (
                        NotificationParameterKey::AccountId,
                        NotificationScalar::String(_)
                    )
                )
            }
            Self::AccountRecoveryFailed | Self::AccountReconciliationFailed => matches!(
                (key, value),
                (
                    NotificationParameterKey::AttemptId,
                    NotificationScalar::String(_)
                ) | (
                    NotificationParameterKey::FailedSteps,
                    NotificationScalar::String(_)
                )
            ),
            Self::ConnectionUnavailable | Self::ConnectionRecovered => {
                matches!(
                    (key, value),
                    (
                        NotificationParameterKey::Channel,
                        NotificationScalar::String(_)
                    )
                )
            }
            Self::EnvironmentUnavailable | Self::EnvironmentRecovered => {
                matches!(
                    (key, value),
                    (
                        NotificationParameterKey::Environment,
                        NotificationScalar::String(_)
                    )
                )
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum NotificationParameterKey {
    AccountId,
    AttemptId,
    Channel,
    Environment,
    FailedSteps,
    Limit,
    OrderId,
    SubmissionId,
    ViolationCode,
}

impl NotificationParameterKey {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "accountId" => Self::AccountId,
            "attemptId" => Self::AttemptId,
            "channel" => Self::Channel,
            "environment" => Self::Environment,
            "failedSteps" => Self::FailedSteps,
            "limit" => Self::Limit,
            "orderId" => Self::OrderId,
            "submissionId" => Self::SubmissionId,
            "violationCode" => Self::ViolationCode,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationContent {
    pub message_key: String,
    pub params: BTreeMap<String, NotificationScalar>,
    pub fallback_title: String,
    pub fallback_body: String,
}

impl NotificationContent {
    pub fn new<K, I>(
        message_key: impl Into<String>,
        params: I,
        fallback_title: impl Into<String>,
        fallback_body: impl Into<String>,
    ) -> Result<Self, NotificationValidationError>
    where
        K: Into<String>,
        I: IntoIterator<Item = (K, NotificationScalar)>,
    {
        let content = Self {
            message_key: message_key.into(),
            params: params
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
            fallback_title: fallback_title.into(),
            fallback_body: fallback_body.into(),
        };
        content.validate()?;
        Ok(content)
    }

    pub fn validate(&self) -> Result<(), NotificationValidationError> {
        let Some(template) = NotificationMessageTemplate::from_message_key(&self.message_key)
        else {
            return Err(NotificationValidationError::new(
                "INVALID_NOTIFICATION_CONTENT",
                "通知消息模板不受支持",
            ));
        };
        let (fallback_title, fallback_body) = template.fallback();
        if self.fallback_title != fallback_title || self.fallback_body != fallback_body {
            return Err(NotificationValidationError::new(
                "INVALID_NOTIFICATION_CONTENT",
                "通知兜底文本不匹配受控模板",
            ));
        }
        for (key, value) in &self.params {
            let Some(parameter_key) = NotificationParameterKey::parse(key) else {
                return Err(NotificationValidationError::new(
                    "INVALID_NOTIFICATION_CONTENT",
                    "通知参数名不受支持",
                ));
            };
            value.validate()?;
            if !template.accepts(parameter_key, value) {
                return Err(NotificationValidationError::new(
                    "INVALID_NOTIFICATION_CONTENT",
                    "通知参数类型不受支持",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationEntityType {
    Order,
    Account,
    Connection,
    Environment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationEntity {
    #[serde(rename = "type")]
    pub entity_type: NotificationEntityType,
    pub id: String,
}

impl NotificationEntity {
    fn validate(&self) -> Result<(), NotificationValidationError> {
        if self.id.trim().is_empty() {
            return Err(NotificationValidationError::new(
                "INVALID_NOTIFICATION_RECORD",
                "通知实体标识不能为空",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountNotificationSection {
    Api,
    Risk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NotificationAction {
    OpenTrading {
        order_id: Option<String>,
    },
    OpenAccountSettings {
        account_section: AccountNotificationSection,
    },
    OpenGeneralSettings,
}

impl NotificationAction {
    fn validate(&self) -> Result<(), NotificationValidationError> {
        if matches!(self, Self::OpenTrading { order_id: Some(order_id) } if !is_safe_scalar_string(order_id))
        {
            return Err(NotificationValidationError::new(
                "INVALID_NOTIFICATION_ACTION",
                "通知动作订单标识无效",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationRecord {
    pub id: String,
    pub scope: NotificationScope,
    pub category: NotificationCategory,
    pub kind: NotificationKind,
    pub severity: NotificationSeverity,
    pub content: NotificationContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<NotificationEntity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<NotificationAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_event_id: Option<String>,
    pub dedupe_key: String,
    pub occurrence_count: u32,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at_ms: Option<u64>,
}

impl NotificationRecord {
    pub fn validate(&self) -> Result<(), NotificationValidationError> {
        let id = Uuid::parse_str(&self.id).map_err(|_| invalid_record())?;
        if id.get_version() != Some(Version::Random) || id.hyphenated().to_string() != self.id {
            return Err(invalid_record());
        }
        self.scope.validate().map_err(|_| invalid_record())?;
        self.content.validate().map_err(|_| invalid_record())?;
        if self
            .entity
            .as_ref()
            .is_some_and(|entity| entity.validate().is_err())
            || self
                .action
                .as_ref()
                .is_some_and(|action| action.validate().is_err())
            || self
                .source_event_id
                .as_ref()
                .is_some_and(|id| id.trim().is_empty())
            || self.dedupe_key.trim().is_empty()
            || self.occurrence_count == 0
            || !is_safe_timestamp(self.created_at_ms)
            || !is_safe_timestamp(self.updated_at_ms)
            || self
                .read_at_ms
                .is_some_and(|value| !is_safe_timestamp(value))
            || self.updated_at_ms < self.created_at_ms
            || self.read_at_ms.is_some_and(|read_at_ms| {
                read_at_ms < self.created_at_ms || read_at_ms > self.updated_at_ms
            })
        {
            return Err(invalid_record());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationInput {
    pub scope: NotificationScope,
    pub category: NotificationCategory,
    pub kind: NotificationKind,
    pub severity: NotificationSeverity,
    pub content: NotificationContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<NotificationEntity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<NotificationAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_event_id: Option<String>,
    pub dedupe_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_epoch: Option<u64>,
}

impl NotificationInput {
    pub fn validate(&self) -> Result<(), NotificationValidationError> {
        self.scope.validate().map_err(|_| invalid_content())?;
        self.content.validate()?;
        if self
            .entity
            .as_ref()
            .is_some_and(|entity| entity.validate().is_err())
            || self
                .action
                .as_ref()
                .is_some_and(|action| action.validate().is_err())
            || self
                .source_event_id
                .as_ref()
                .is_some_and(|id| id.trim().is_empty())
            || self.dedupe_key.trim().is_empty()
        {
            return Err(invalid_content());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum NotificationFilter {
    #[default]
    All,
    Unread,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListNotificationsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default)]
    pub filter: NotificationFilter,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default = "default_notification_page_limit")]
    pub limit: u32,
}

impl ListNotificationsRequest {
    pub fn validate(&self) -> Result<(), NotificationValidationError> {
        if self
            .account_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
            || self
                .cursor
                .as_ref()
                .is_some_and(|cursor| cursor.trim().is_empty())
            || !(1..=MAX_NOTIFICATION_PAGE_LIMIT).contains(&self.limit)
        {
            return Err(NotificationValidationError::new(
                "INVALID_NOTIFICATION_REQUEST",
                "通知查询请求无效",
            ));
        }
        Ok(())
    }
}

fn default_notification_page_limit() -> u32 {
    DEFAULT_NOTIFICATION_PAGE_LIMIT
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPage {
    pub items: Vec<NotificationRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub unread_count: u64,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSummary {
    pub unread_count: u64,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationMutationResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification: Option<NotificationRecord>,
    pub affected_count: u64,
    pub affected_scopes: Vec<NotificationScope>,
    pub unread_count: u64,
    pub revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationChange {
    Created,
    Updated,
    Removed,
    Reset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationToastCandidate {
    pub id: String,
    pub scope: NotificationScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_epoch: Option<u64>,
    pub category: NotificationCategory,
    pub severity: NotificationSeverity,
    pub content: NotificationContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<NotificationAction>,
}

impl NotificationToastCandidate {
    fn validate(&self) -> Result<(), NotificationValidationError> {
        validate_notification_id(&self.id).map_err(|_| invalid_event())?;
        self.scope.validate().map_err(|_| invalid_event())?;
        self.content.validate().map_err(|_| invalid_event())?;
        if self
            .session_epoch
            .is_some_and(|epoch| !is_safe_timestamp(epoch))
            || self
                .action
                .as_ref()
                .is_some_and(|action| action.validate().is_err())
        {
            return Err(invalid_event());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationChangedEvent {
    pub previous_revision: String,
    pub revision: String,
    pub change: NotificationChange,
    pub affected_scopes: Vec<NotificationScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toast_candidate: Option<NotificationToastCandidate>,
}

impl NotificationChangedEvent {
    pub fn validate(&self) -> Result<(), NotificationValidationError> {
        if self.change != NotificationChange::Created && self.toast_candidate.is_some() {
            return Err(invalid_event());
        }
        if let Some(candidate) = &self.toast_candidate {
            candidate.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClientNotificationKind {
    AccountRecoveryFailed,
    AccountReconciliationFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClientNotificationFailedStep {
    Config,
    Profiles,
    Connection,
    Bootstrap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateClientNotificationRequest {
    pub account_id: String,
    pub session_epoch: u64,
    pub attempt_id: String,
    pub kind: ClientNotificationKind,
    pub failed_steps: Vec<ClientNotificationFailedStep>,
}

impl CreateClientNotificationRequest {
    pub fn validate(&self) -> Result<(), NotificationValidationError> {
        if self.account_id.trim().is_empty()
            || self.attempt_id.trim().is_empty()
            || self.failed_steps.is_empty()
        {
            return Err(NotificationValidationError::new(
                "INVALID_NOTIFICATION_REQUEST",
                "客户端通知请求无效",
            ));
        }
        Ok(())
    }
}

fn is_safe_timestamp(value: u64) -> bool {
    value <= MAX_JAVASCRIPT_SAFE_INTEGER
}

fn is_safe_scalar_string(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/' | b'@')
        })
}

fn validate_notification_id(id: &str) -> Result<(), NotificationValidationError> {
    let parsed = Uuid::parse_str(id).map_err(|_| invalid_record())?;
    if parsed.get_version() != Some(Version::Random) || parsed.hyphenated().to_string() != id {
        return Err(invalid_record());
    }
    Ok(())
}

fn invalid_record() -> NotificationValidationError {
    NotificationValidationError::new("INVALID_NOTIFICATION_RECORD", "通知记录无效")
}

fn invalid_content() -> NotificationValidationError {
    NotificationValidationError::new("INVALID_NOTIFICATION_CONTENT", "通知内容无效")
}

fn invalid_event() -> NotificationValidationError {
    NotificationValidationError::new("INVALID_NOTIFICATION_EVENT", "通知变更事件无效")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn account_scope_and_action_use_tagged_camel_case_wire_shapes() {
        assert_eq!(
            serde_json::to_value(NotificationScope::Account {
                account_id: "alpha".into()
            })
            .unwrap(),
            json!({ "type": "account", "accountId": "alpha" }),
        );
        assert_eq!(
            serde_json::to_value(NotificationAction::OpenTrading {
                order_id: Some("o-1".into())
            })
            .unwrap(),
            json!({ "type": "openTrading", "orderId": "o-1" }),
        );
    }

    #[test]
    fn content_validation_rejects_non_finite_or_uncontrolled_values() {
        let content = NotificationContent::new(
            "risk.orderBlocked",
            [("limit", NotificationScalar::Number(f64::NAN))],
            "订单被风控拦截",
            "请检查风控设置",
        );
        assert_eq!(content.unwrap_err().code(), "INVALID_NOTIFICATION_CONTENT");
    }

    #[test]
    fn content_validation_rejects_unapproved_templates_params_and_raw_fallback_text() {
        let unknown_param = NotificationContent::new(
            "risk.orderBlocked",
            [(
                "rawReason",
                NotificationScalar::String("server error body".into()),
            )],
            "订单被风控拦截",
            "请检查风控设置",
        );
        assert_eq!(
            unknown_param.unwrap_err().code(),
            "INVALID_NOTIFICATION_CONTENT"
        );

        let wrong_type = NotificationContent::new(
            "risk.orderBlocked",
            [("limit", NotificationScalar::String("100".into()))],
            "订单被风控拦截",
            "请检查风控设置",
        );
        assert_eq!(
            wrong_type.unwrap_err().code(),
            "INVALID_NOTIFICATION_CONTENT"
        );

        let raw_reason = NotificationContent::new(
            "risk.orderBlocked",
            [(
                "violationCode",
                NotificationScalar::String("raw server error body".into()),
            )],
            "订单被风控拦截",
            "请检查风控设置",
        );
        assert_eq!(
            raw_reason.unwrap_err().code(),
            "INVALID_NOTIFICATION_CONTENT"
        );

        let raw_fallback = NotificationContent::new(
            "risk.orderBlocked",
            [("limit", NotificationScalar::Number(100.0))],
            "订单被风控拦截",
            "服务器返回的原始错误文本",
        );
        assert_eq!(
            raw_fallback.unwrap_err().code(),
            "INVALID_NOTIFICATION_CONTENT"
        );
    }

    #[test]
    fn scope_validation_rejects_blank_account_ids() {
        assert_eq!(
            NotificationScope::Account {
                account_id: "  ".into()
            }
            .validate()
            .unwrap_err()
            .code(),
            "INVALID_NOTIFICATION_SCOPE"
        );
    }

    #[test]
    fn record_validation_rejects_invalid_uuid_zero_occurrences_and_invalid_times() {
        let mut record = valid_record();
        record.id = "not-a-uuid".into();
        assert_eq!(
            record.validate().unwrap_err().code(),
            "INVALID_NOTIFICATION_RECORD"
        );

        let mut record = valid_record();
        record.occurrence_count = 0;
        assert_eq!(
            record.validate().unwrap_err().code(),
            "INVALID_NOTIFICATION_RECORD"
        );

        let mut record = valid_record();
        record.updated_at_ms = record.created_at_ms - 1;
        assert_eq!(
            record.validate().unwrap_err().code(),
            "INVALID_NOTIFICATION_RECORD"
        );

        let mut record = valid_record();
        record.created_at_ms = MAX_JAVASCRIPT_SAFE_INTEGER + 1;
        assert_eq!(
            record.validate().unwrap_err().code(),
            "INVALID_NOTIFICATION_RECORD"
        );
    }

    #[test]
    fn every_controlled_enum_has_a_stable_wire_value() {
        assert_eq!(
            serde_json::to_value(NotificationScope::Global).unwrap(),
            json!({ "type": "global" })
        );
        assert_eq!(
            serde_json::to_value(NotificationCategory::Trading).unwrap(),
            json!("trading")
        );
        assert_eq!(
            serde_json::to_value(NotificationCategory::RiskAccount).unwrap(),
            json!("riskAccount")
        );
        assert_eq!(
            serde_json::to_value(NotificationCategory::ConnectionSystem).unwrap(),
            json!("connectionSystem")
        );
        assert_eq!(
            serde_json::to_value(NotificationKind::OrderFilled).unwrap(),
            json!("orderFilled")
        );
        assert_eq!(
            serde_json::to_value(NotificationKind::OrderCanceled).unwrap(),
            json!("orderCanceled")
        );
        assert_eq!(
            serde_json::to_value(NotificationKind::OrderRejected).unwrap(),
            json!("orderRejected")
        );
        assert_eq!(
            serde_json::to_value(NotificationKind::RiskOrderBlocked).unwrap(),
            json!("riskOrderBlocked")
        );
        assert_eq!(
            serde_json::to_value(NotificationKind::AccountSessionExpired).unwrap(),
            json!("accountSessionExpired")
        );
        assert_eq!(
            serde_json::to_value(NotificationKind::AccountRecoveryFailed).unwrap(),
            json!("accountRecoveryFailed")
        );
        assert_eq!(
            serde_json::to_value(NotificationKind::AccountReconciliationFailed).unwrap(),
            json!("accountReconciliationFailed")
        );
        assert_eq!(
            serde_json::to_value(NotificationKind::ConnectionUnavailable).unwrap(),
            json!("connectionUnavailable")
        );
        assert_eq!(
            serde_json::to_value(NotificationKind::ConnectionRecovered).unwrap(),
            json!("connectionRecovered")
        );
        assert_eq!(
            serde_json::to_value(NotificationKind::EnvironmentUnavailable).unwrap(),
            json!("environmentUnavailable")
        );
        assert_eq!(
            serde_json::to_value(NotificationKind::EnvironmentRecovered).unwrap(),
            json!("environmentRecovered")
        );
        assert_eq!(
            serde_json::to_value(NotificationSeverity::Success).unwrap(),
            json!("success")
        );
        assert_eq!(
            serde_json::to_value(NotificationSeverity::Info).unwrap(),
            json!("info")
        );
        assert_eq!(
            serde_json::to_value(NotificationSeverity::Warning).unwrap(),
            json!("warning")
        );
        assert_eq!(
            serde_json::to_value(NotificationSeverity::Error).unwrap(),
            json!("error")
        );
        assert_eq!(
            serde_json::to_value(NotificationSeverity::Critical).unwrap(),
            json!("critical")
        );
        assert_eq!(
            serde_json::to_value(NotificationEntityType::Order).unwrap(),
            json!("order")
        );
        assert_eq!(
            serde_json::to_value(NotificationEntityType::Account).unwrap(),
            json!("account")
        );
        assert_eq!(
            serde_json::to_value(NotificationEntityType::Connection).unwrap(),
            json!("connection")
        );
        assert_eq!(
            serde_json::to_value(NotificationEntityType::Environment).unwrap(),
            json!("environment")
        );
        assert_eq!(
            serde_json::to_value(AccountNotificationSection::Api).unwrap(),
            json!("api")
        );
        assert_eq!(
            serde_json::to_value(AccountNotificationSection::Risk).unwrap(),
            json!("risk")
        );
        assert_eq!(
            serde_json::to_value(NotificationFilter::All).unwrap(),
            json!("all")
        );
        assert_eq!(
            serde_json::to_value(NotificationFilter::Unread).unwrap(),
            json!("unread")
        );
        assert_eq!(
            serde_json::to_value(NotificationChange::Created).unwrap(),
            json!("created")
        );
        assert_eq!(
            serde_json::to_value(NotificationChange::Updated).unwrap(),
            json!("updated")
        );
        assert_eq!(
            serde_json::to_value(NotificationChange::Removed).unwrap(),
            json!("removed")
        );
        assert_eq!(
            serde_json::to_value(NotificationChange::Reset).unwrap(),
            json!("reset")
        );
        assert_eq!(
            serde_json::to_value(ClientNotificationKind::AccountRecoveryFailed).unwrap(),
            json!("accountRecoveryFailed")
        );
        assert_eq!(
            serde_json::to_value(ClientNotificationKind::AccountReconciliationFailed).unwrap(),
            json!("accountReconciliationFailed")
        );
        assert_eq!(
            serde_json::to_value(ClientNotificationFailedStep::Config).unwrap(),
            json!("config")
        );
        assert_eq!(
            serde_json::to_value(ClientNotificationFailedStep::Profiles).unwrap(),
            json!("profiles")
        );
        assert_eq!(
            serde_json::to_value(ClientNotificationFailedStep::Connection).unwrap(),
            json!("connection")
        );
        assert_eq!(
            serde_json::to_value(ClientNotificationFailedStep::Bootstrap).unwrap(),
            json!("bootstrap")
        );
    }

    #[test]
    fn non_created_changes_omit_toast_candidates() {
        let event = NotificationChangedEvent {
            previous_revision: "4".into(),
            revision: "5".into(),
            change: NotificationChange::Updated,
            affected_scopes: vec![NotificationScope::Global],
            notification_id: Some("n-1".into()),
            toast_candidate: None,
        };

        let value = serde_json::to_value(event).unwrap();
        assert!(value.get("toastCandidate").is_none());
    }

    #[test]
    fn created_events_reject_invalid_toast_candidates() {
        let mut candidate = valid_toast_candidate();
        candidate.id = "not-a-uuid".into();
        assert_invalid_candidate(candidate);

        let mut candidate = valid_toast_candidate();
        candidate.scope = NotificationScope::Account {
            account_id: " ".into(),
        };
        assert_invalid_candidate(candidate);

        let mut candidate = valid_toast_candidate();
        candidate.content = NotificationContent {
            message_key: "risk.orderBlocked".into(),
            params: [(
                "rawReason".into(),
                NotificationScalar::String("safe".into()),
            )]
            .into_iter()
            .collect(),
            fallback_title: "订单被风控拦截".into(),
            fallback_body: "请检查风控设置".into(),
        };
        assert_invalid_candidate(candidate);

        let mut candidate = valid_toast_candidate();
        candidate.action = Some(NotificationAction::OpenTrading {
            order_id: Some(" ".into()),
        });
        assert_invalid_candidate(candidate);

        let mut candidate = valid_toast_candidate();
        candidate.session_epoch = Some(MAX_JAVASCRIPT_SAFE_INTEGER + 1);
        assert_invalid_candidate(candidate);
    }

    fn assert_invalid_candidate(candidate: NotificationToastCandidate) {
        let event = NotificationChangedEvent {
            previous_revision: "4".into(),
            revision: "5".into(),
            change: NotificationChange::Created,
            affected_scopes: vec![NotificationScope::Global],
            notification_id: Some("n-1".into()),
            toast_candidate: Some(candidate),
        };
        assert_eq!(
            event.validate().unwrap_err().code(),
            "INVALID_NOTIFICATION_EVENT"
        );
    }

    fn valid_toast_candidate() -> NotificationToastCandidate {
        NotificationToastCandidate {
            id: "0b102d04-848c-4c84-a644-033383850c71".into(),
            scope: NotificationScope::Account {
                account_id: "alpha".into(),
            },
            session_epoch: Some(4),
            category: NotificationCategory::RiskAccount,
            severity: NotificationSeverity::Warning,
            content: NotificationContent::new(
                "risk.orderBlocked",
                [("limit", NotificationScalar::Number(100.0))],
                "订单被风控拦截",
                "请检查风控设置",
            )
            .unwrap(),
            action: Some(NotificationAction::OpenAccountSettings {
                account_section: AccountNotificationSection::Risk,
            }),
        }
    }

    fn valid_record() -> NotificationRecord {
        NotificationRecord {
            id: "0b102d04-848c-4c84-a644-033383850c71".into(),
            scope: NotificationScope::Account {
                account_id: "alpha".into(),
            },
            category: NotificationCategory::RiskAccount,
            kind: NotificationKind::RiskOrderBlocked,
            severity: NotificationSeverity::Warning,
            content: NotificationContent::new(
                "risk.orderBlocked",
                [("limit", NotificationScalar::Number(100.0))],
                "订单被风控拦截",
                "请检查风控设置",
            )
            .unwrap(),
            entity: None,
            action: Some(NotificationAction::OpenAccountSettings {
                account_section: AccountNotificationSection::Risk,
            }),
            source_event_id: Some("submission-1".into()),
            dedupe_key: "alpha:submission-1:risk".into(),
            occurrence_count: 1,
            created_at_ms: 1_700_000_000_000,
            updated_at_ms: 1_700_000_000_000,
            read_at_ms: None,
        }
    }
}
