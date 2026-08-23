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
        if matches!(self, Self::Number(number) if !number.is_finite()) {
            return Err(NotificationValidationError::new(
                "INVALID_NOTIFICATION_CONTENT",
                "通知内容包含非有限数值",
            ));
        }
        Ok(())
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
        if self.message_key.trim().is_empty()
            || self.fallback_title.trim().is_empty()
            || self.fallback_body.trim().is_empty()
            || self.params.keys().any(|key| key.trim().is_empty())
        {
            return Err(NotificationValidationError::new(
                "INVALID_NOTIFICATION_CONTENT",
                "通知内容包含空白字段",
            ));
        }
        self.params
            .values()
            .try_for_each(NotificationScalar::validate)
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
            return Err(NotificationValidationError::new(
                "INVALID_NOTIFICATION_EVENT",
                "只有新建通知可以携带 Toast 候选项",
            ));
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

fn invalid_record() -> NotificationValidationError {
    NotificationValidationError::new("INVALID_NOTIFICATION_RECORD", "通知记录无效")
}

fn invalid_content() -> NotificationValidationError {
    NotificationValidationError::new("INVALID_NOTIFICATION_CONTENT", "通知内容无效")
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
                [("limit", NotificationScalar::String("100".into()))],
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
