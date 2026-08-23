use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use uuid::{Uuid, Version};

pub const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub const DEFAULT_NOTIFICATION_PAGE_LIMIT: u32 = 50;
pub const MAX_NOTIFICATION_PAGE_LIMIT: u32 = 100;
pub(crate) const MAX_NOTIFICATION_SOURCE_EVENT_ID_BYTES: usize = 256;
pub(crate) const MAX_NOTIFICATION_DEDUPE_KEY_BYTES: usize = 512;

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
            Self::Account { account_id } if AccountId::parse(account_id).is_none() => Err(
                NotificationValidationError::new("INVALID_NOTIFICATION_SCOPE", "通知账户范围无效"),
            ),
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
        if matches!(self, Self::Number(number) if !number.is_finite()) {
            return Err(NotificationValidationError::new(
                "INVALID_NOTIFICATION_CONTENT",
                "通知内容包含非有限数值",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskViolationCode {
    InvalidQuantity,
    NonPositiveQuantity,
    MaxOrderQty,
    MissingLimitPrice,
    InvalidLimitPrice,
    NonPositiveLimitPrice,
    MaxPriceDeviation,
    DailyOrderLimit,
    LedgerUnavailable,
}

impl RiskViolationCode {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "invalidQuantity" => Self::InvalidQuantity,
            "nonPositiveQuantity" => Self::NonPositiveQuantity,
            "maxOrderQty" => Self::MaxOrderQty,
            "missingLimitPrice" => Self::MissingLimitPrice,
            "invalidLimitPrice" => Self::InvalidLimitPrice,
            "nonPositiveLimitPrice" => Self::NonPositiveLimitPrice,
            "maxPriceDeviation" => Self::MaxPriceDeviation,
            "dailyOrderLimit" => Self::DailyOrderLimit,
            "ledgerUnavailable" => Self::LedgerUnavailable,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationChannel {
    Api,
    Websocket,
}

impl NotificationChannel {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "api" => Self::Api,
            "websocket" => Self::Websocket,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationEnvironment {
    Production,
    Development,
    Unknown,
}

impl NotificationEnvironment {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "production" => Self::Production,
            "development" => Self::Development,
            "unknown" => Self::Unknown,
            _ => return None,
        })
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

    fn accepts(self, key: NotificationParameterKey) -> bool {
        match self {
            Self::OrderFilled | Self::OrderCanceled => key == NotificationParameterKey::OrderId,
            Self::OrderRejected => matches!(
                key,
                NotificationParameterKey::OrderId | NotificationParameterKey::SubmissionId
            ),
            Self::RiskOrderBlocked => matches!(
                key,
                NotificationParameterKey::Limit | NotificationParameterKey::ViolationCode
            ),
            Self::AccountSessionExpired => key == NotificationParameterKey::AccountId,
            Self::AccountRecoveryFailed | Self::AccountReconciliationFailed => matches!(
                key,
                NotificationParameterKey::AttemptId | NotificationParameterKey::FailedSteps
            ),
            Self::ConnectionUnavailable | Self::ConnectionRecovered => {
                key == NotificationParameterKey::Channel
            }
            Self::EnvironmentUnavailable | Self::EnvironmentRecovered => {
                key == NotificationParameterKey::Environment
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

    fn accepts(self, value: &NotificationScalar) -> bool {
        match (self, value) {
            (Self::AccountId, NotificationScalar::String(value)) => {
                AccountId::parse(value).is_some()
            }
            (Self::AttemptId, NotificationScalar::String(value)) => {
                AttemptId::parse(value).is_some()
            }
            (Self::Channel, NotificationScalar::String(value)) => {
                NotificationChannel::parse(value).is_some()
            }
            (Self::Environment, NotificationScalar::String(value)) => {
                NotificationEnvironment::parse(value).is_some()
            }
            (Self::FailedSteps, NotificationScalar::String(value)) => {
                ClientNotificationFailedStep::is_normalized_list(value)
            }
            (Self::Limit, NotificationScalar::Number(value)) => *value >= 0.0,
            (Self::OrderId, NotificationScalar::String(value)) => OrderId::parse(value).is_some(),
            (Self::SubmissionId, NotificationScalar::String(value)) => {
                SubmissionId::parse(value).is_some()
            }
            (Self::ViolationCode, NotificationScalar::String(value)) => {
                RiskViolationCode::parse(value).is_some()
            }
            _ => false,
        }
    }
}

struct AccountId;

impl AccountId {
    fn parse(value: &str) -> Option<()> {
        (!value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            && !looks_like_secret(value))
        .then_some(())
    }
}

struct OrderId;

impl OrderId {
    fn parse(value: &str) -> Option<()> {
        (!value.is_empty()
            && value.len() <= 64
            && value.bytes().any(|byte| byte.is_ascii_digit())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            && !looks_like_secret(value))
        .then_some(())
    }
}

struct SubmissionId;

impl SubmissionId {
    fn parse(value: &str) -> Option<()> {
        is_generated_uuid(value).then_some(())
    }
}

struct AttemptId;

impl AttemptId {
    fn parse(value: &str) -> Option<()> {
        is_generated_uuid(value).then_some(())
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
            if !template.accepts(parameter_key) || !parameter_key.accepts(value) {
                return Err(NotificationValidationError::new(
                    "INVALID_NOTIFICATION_CONTENT",
                    "通知参数值不受支持",
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
        if matches!(self, Self::OpenTrading { order_id: Some(order_id) } if OrderId::parse(order_id).is_none())
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
                .is_some_and(|id| !is_safe_notification_source_event_id(id))
            || !is_safe_notification_dedupe_key(&self.dedupe_key)
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
                .is_some_and(|id| !is_safe_notification_source_event_id(id))
            || !is_safe_notification_dedupe_key(&self.dedupe_key)
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

impl ClientNotificationFailedStep {
    fn is_normalized_list(value: &str) -> bool {
        let mut previous = None;
        for step in value.split(',') {
            let rank = match step {
                "config" => 0,
                "profiles" => 1,
                "connection" => 2,
                "bootstrap" => 3,
                _ => return false,
            };
            if previous.is_some_and(|prior| rank <= prior) {
                return false;
            }
            previous = Some(rank);
        }
        previous.is_some()
    }
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
        if AccountId::parse(&self.account_id).is_none()
            || AttemptId::parse(&self.attempt_id).is_none()
            || !is_safe_timestamp(self.session_epoch)
            || self.failed_steps.is_empty()
            || self
                .failed_steps
                .iter()
                .enumerate()
                .any(|(index, step)| self.failed_steps[..index].contains(step))
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

pub(crate) fn is_safe_notification_source_event_id(value: &str) -> bool {
    is_safe_notification_identifier(value, MAX_NOTIFICATION_SOURCE_EVENT_ID_BYTES)
}

pub(crate) fn is_safe_notification_dedupe_key(value: &str) -> bool {
    is_safe_notification_identifier(value, MAX_NOTIFICATION_DEDUPE_KEY_BYTES)
}

fn is_safe_notification_identifier(value: &str, max_bytes: usize) -> bool {
    if value.is_empty()
        || value.len() > max_bytes
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
        || value.split(':').any(looks_like_secret)
    {
        return false;
    }
    !value
        .split([':', '-', '_'])
        .map(str::to_ascii_lowercase)
        .any(|segment| {
            matches!(
                segment.as_str(),
                "sk" | "secret" | "token" | "bearer" | "apikey"
            )
        })
}

fn looks_like_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("sk_")
        || lower.starts_with("sk-")
        || lower.starts_with("api_key")
        || lower.starts_with("api-key")
        || lower.starts_with("secret")
        || lower.starts_with("token")
        || lower.starts_with("bearer")
        || lower.starts_with("akia")
        || lower.starts_with("ghp_")
        || lower.starts_with("gho_")
        || lower.starts_with("ghu_")
        || lower.starts_with("ghs_")
        || lower.starts_with("github_pat_")
        || lower.starts_with("xoxb-")
        || lower.starts_with("xoxp-")
        || value.starts_with("eyJ")
}

fn is_generated_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|parsed| {
        parsed.get_version() == Some(Version::Random) && parsed.hyphenated().to_string() == value
    })
}

fn validate_notification_id(id: &str) -> Result<(), NotificationValidationError> {
    if !is_generated_uuid(id) {
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
    fn content_validation_rejects_secret_shaped_violation_codes() {
        assert_invalid_content(NotificationContent::new(
            "risk.orderBlocked",
            [(
                "violationCode",
                NotificationScalar::String("sk_live_51NQy6iF8dFx1Q".into()),
            )],
            "订单被风控拦截",
            "请检查风控设置",
        ));
    }

    #[test]
    fn content_validation_rejects_secret_shaped_connection_channels() {
        assert_invalid_content(NotificationContent::new(
            "connection.unavailable",
            [(
                "channel",
                NotificationScalar::String("sk_live_channel".into()),
            )],
            "连接不可用",
            "交易连接暂时不可用，请检查网络或稍后重试。",
        ));
    }

    #[test]
    fn content_validation_rejects_secret_shaped_failed_steps_and_environment_urls() {
        assert_invalid_content(NotificationContent::new(
            "account.recoveryFailed",
            [(
                "failedSteps",
                NotificationScalar::String("sk_live_steps".into()),
            )],
            "账户恢复失败",
            "请检查账户设置后重试。",
        ));
        assert_invalid_content(NotificationContent::new(
            "environment.unavailable",
            [(
                "environment",
                NotificationScalar::String("https://api.example.test/error".into()),
            )],
            "环境不可达",
            "当前交易环境暂时不可达，请稍后重试。",
        ));
    }

    #[test]
    fn content_validation_rejects_malformed_identifier_parameters() {
        assert_invalid_content(NotificationContent::new(
            "order.filled",
            [(
                "orderId",
                NotificationScalar::String("secret-order-id".into()),
            )],
            "订单已成交",
            "订单已完全成交，请前往交易页查看。",
        ));
        assert_invalid_content(NotificationContent::new(
            "order.rejected",
            [(
                "submissionId",
                NotificationScalar::String("not-a-generated-uuid".into()),
            )],
            "订单被拒绝",
            "订单请求被交易端拒绝，请检查订单参数。",
        ));
        assert_invalid_content(NotificationContent::new(
            "account.sessionExpired",
            [(
                "accountId",
                NotificationScalar::String("sk_live_account".into()),
            )],
            "账户会话已失效",
            "请检查账户 API 设置。",
        ));
        assert_invalid_content(NotificationContent::new(
            "account.reconciliationFailed",
            [(
                "attemptId",
                NotificationScalar::String("not-a-generated-uuid".into()),
            )],
            "账户对账失败",
            "请检查账户数据后重试。",
        ));
    }

    #[test]
    fn content_validation_accepts_only_controlled_semantic_parameter_values() {
        assert!(NotificationContent::new(
            "risk.orderBlocked",
            [(
                "violationCode",
                NotificationScalar::String("maxOrderQty".into())
            )],
            "订单被风控拦截",
            "请检查风控设置",
        )
        .is_ok());
        assert!(NotificationContent::new(
            "connection.recovered",
            [("channel", NotificationScalar::String("api".into()))],
            "连接已恢复",
            "交易连接已恢复。",
        )
        .is_ok());
        assert!(NotificationContent::new(
            "environment.recovered",
            [(
                "environment",
                NotificationScalar::String("production".into()),
            )],
            "环境已恢复",
            "当前交易环境已恢复。",
        )
        .is_ok());
        assert!(NotificationContent::new(
            "account.recoveryFailed",
            [(
                "failedSteps",
                NotificationScalar::String("config,connection".into()),
            )],
            "账户恢复失败",
            "请检查账户设置后重试。",
        )
        .is_ok());
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
    fn record_and_input_reject_unsafe_source_and_dedupe_identifiers() {
        for source_event_id in [
            "raw server body".to_string(),
            "event/token".to_string(),
            "event-token-value".to_string(),
            "eyJhbGciOiJIUzI1NiJ9".to_string(),
            "event:AKIAIOSFODNN7EXAMPLE".to_string(),
            "event:eyJhbGciOiJIUzI1NiJ9".to_string(),
            "attempt:ghp_xxxxxxxxxxxxxxxxxxxx".to_string(),
            "a".repeat(257),
        ] {
            let mut record = valid_record();
            record.source_event_id = Some(source_event_id.clone());
            assert_eq!(
                record.validate().unwrap_err().code(),
                "INVALID_NOTIFICATION_RECORD"
            );

            let input = NotificationInput {
                scope: record.scope,
                category: record.category,
                kind: record.kind,
                severity: record.severity,
                content: record.content,
                entity: record.entity,
                action: record.action,
                source_event_id: Some(source_event_id),
                dedupe_key: "alpha:submission-1:risk".into(),
                session_epoch: Some(1),
            };
            assert_eq!(
                input.validate().unwrap_err().code(),
                "INVALID_NOTIFICATION_CONTENT"
            );
        }

        for dedupe_key in [
            "raw dedupe text".to_string(),
            "alpha:secret:value".to_string(),
            "alpha:AKIAIOSFODNN7EXAMPLE:risk".to_string(),
            "alpha:eyJhbGciOiJIUzI1NiJ9:risk".to_string(),
            "a".repeat(513),
        ] {
            let mut record = valid_record();
            record.dedupe_key = dedupe_key.clone();
            assert_eq!(
                record.validate().unwrap_err().code(),
                "INVALID_NOTIFICATION_RECORD"
            );

            let input = NotificationInput {
                scope: record.scope,
                category: record.category,
                kind: record.kind,
                severity: record.severity,
                content: record.content,
                entity: record.entity,
                action: record.action,
                source_event_id: record.source_event_id,
                dedupe_key,
                session_epoch: Some(1),
            };
            assert_eq!(
                input.validate().unwrap_err().code(),
                "INVALID_NOTIFICATION_CONTENT"
            );
        }
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

    fn assert_invalid_content(result: Result<NotificationContent, NotificationValidationError>) {
        assert_eq!(result.unwrap_err().code(), "INVALID_NOTIFICATION_CONTENT");
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
