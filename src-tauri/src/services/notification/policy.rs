use crate::models::notification::{
    is_safe_notification_source_event_id, AccountNotificationSection, ClientNotificationFailedStep,
    ClientNotificationKind, CreateClientNotificationRequest, NotificationAction,
    NotificationCategory, NotificationChannel, NotificationContent, NotificationEntity,
    NotificationEntityType, NotificationEnvironment, NotificationInput, NotificationKind,
    NotificationScalar, NotificationScope, NotificationSeverity, RiskViolationCode,
    MAX_JAVASCRIPT_SAFE_INTEGER,
};

use super::NotificationError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyContext {
    account_id: String,
    session_epoch: u64,
    source_event_id: String,
}

impl PolicyContext {
    pub fn new(
        account_id: impl Into<String>,
        session_epoch: u64,
        source_event_id: impl Into<String>,
    ) -> Result<Self, NotificationError> {
        let context = Self {
            account_id: account_id.into(),
            session_epoch,
            source_event_id: source_event_id.into(),
        };
        NotificationScope::Account {
            account_id: context.account_id.clone(),
        }
        .validate()
        .map_err(|error| NotificationError::new(error.code(), "通知账户范围无效"))?;
        if context.session_epoch > MAX_JAVASCRIPT_SAFE_INTEGER
            || !is_safe_notification_source_event_id(&context.source_event_id)
        {
            return Err(NotificationError::new(
                "INVALID_NOTIFICATION_CONTENT",
                "通知来源标识无效",
            ));
        }
        Ok(context)
    }

    fn scope(&self) -> NotificationScope {
        NotificationScope::Account {
            account_id: self.account_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiskViolationInput {
    pub code: RiskViolationCode,
    pub limit: Option<f64>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NotificationPolicy;

impl NotificationPolicy {
    pub fn order_filled(
        &self,
        context: PolicyContext,
        order_id: &str,
    ) -> Result<NotificationInput, NotificationError> {
        self.order_terminal(
            context,
            order_id,
            NotificationKind::OrderFilled,
            NotificationSeverity::Success,
            "order.filled",
            "订单已成交",
            "订单已完全成交，请前往交易页查看。",
            "filled",
        )
    }

    pub fn order_canceled(
        &self,
        context: PolicyContext,
        order_id: &str,
    ) -> Result<NotificationInput, NotificationError> {
        self.order_terminal(
            context,
            order_id,
            NotificationKind::OrderCanceled,
            NotificationSeverity::Info,
            "order.canceled",
            "订单已取消",
            "订单已取消，请前往交易页查看。",
            "canceled",
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn order_terminal(
        &self,
        context: PolicyContext,
        order_id: &str,
        kind: NotificationKind,
        severity: NotificationSeverity,
        message_key: &str,
        title: &str,
        body: &str,
        suffix: &str,
    ) -> Result<NotificationInput, NotificationError> {
        controlled_input(
            &context,
            NotificationCategory::Trading,
            kind,
            severity,
            NotificationContent::new(
                message_key,
                [("orderId", NotificationScalar::String(order_id.into()))],
                title,
                body,
            ),
            Some(NotificationEntity {
                entity_type: NotificationEntityType::Order,
                id: order_id.into(),
            }),
            Some(NotificationAction::OpenTrading {
                order_id: Some(order_id.into()),
            }),
            format!("{}:{order_id}:{suffix}", context.account_id),
        )
    }

    pub fn order_rejected(
        &self,
        context: PolicyContext,
        order_id: Option<&str>,
        submission_id: Option<&str>,
    ) -> Result<NotificationInput, NotificationError> {
        let (params, identity) = if let Some(order_id) = order_id {
            (
                vec![("orderId", NotificationScalar::String(order_id.into()))],
                order_id,
            )
        } else {
            let submission_id = submission_id.ok_or_else(|| {
                NotificationError::new("INVALID_NOTIFICATION_CONTENT", "订单提交标识无效")
            })?;
            validate_policy_identifier(submission_id, "订单提交标识无效")?;
            (
                vec![(
                    "submissionId",
                    NotificationScalar::String(submission_id.into()),
                )],
                submission_id,
            )
        };
        controlled_input(
            &context,
            NotificationCategory::Trading,
            NotificationKind::OrderRejected,
            NotificationSeverity::Error,
            NotificationContent::new(
                "order.rejected",
                params,
                "订单被拒绝",
                "订单请求被交易端拒绝，请检查订单参数。",
            ),
            order_id.map(|id| NotificationEntity {
                entity_type: NotificationEntityType::Order,
                id: id.into(),
            }),
            Some(NotificationAction::OpenTrading {
                order_id: order_id.map(str::to_owned),
            }),
            format!("{}:{identity}:rejected", context.account_id),
        )
    }

    pub fn risk_order_blocked(
        &self,
        context: PolicyContext,
        violation: RiskViolationInput,
    ) -> Result<NotificationInput, NotificationError> {
        let mut params = vec![(
            "violationCode",
            NotificationScalar::String(risk_code(violation.code).into()),
        )];
        if let Some(limit) = violation.limit {
            params.push(("limit", NotificationScalar::Number(limit)));
        }
        controlled_input(
            &context,
            NotificationCategory::RiskAccount,
            NotificationKind::RiskOrderBlocked,
            NotificationSeverity::Warning,
            NotificationContent::new(
                "risk.orderBlocked",
                params,
                "订单被风控拦截",
                "请检查风控设置",
            ),
            None,
            Some(NotificationAction::OpenAccountSettings {
                account_section: AccountNotificationSection::Risk,
            }),
            format!(
                "{}:{}:{}",
                context.account_id,
                context.source_event_id,
                risk_code(violation.code)
            ),
        )
    }

    pub fn session_expired(
        &self,
        context: PolicyContext,
    ) -> Result<NotificationInput, NotificationError> {
        controlled_input(
            &context,
            NotificationCategory::RiskAccount,
            NotificationKind::AccountSessionExpired,
            NotificationSeverity::Warning,
            NotificationContent::new(
                "account.sessionExpired",
                [(
                    "accountId",
                    NotificationScalar::String(context.account_id.clone()),
                )],
                "账户会话已失效",
                "请检查账户 API 设置。",
            ),
            Some(NotificationEntity {
                entity_type: NotificationEntityType::Account,
                id: context.account_id.clone(),
            }),
            Some(NotificationAction::OpenAccountSettings {
                account_section: AccountNotificationSection::Api,
            }),
            format!(
                "{}:{}:session-expired",
                context.account_id, context.session_epoch
            ),
        )
    }

    pub fn client_account_failure(
        &self,
        request: CreateClientNotificationRequest,
    ) -> Result<NotificationInput, NotificationError> {
        request
            .validate()
            .map_err(|error| NotificationError::new(error.code(), "客户端通知请求无效"))?;
        let context = PolicyContext::new(
            request.account_id.clone(),
            request.session_epoch,
            request.attempt_id.clone(),
        )?;
        let failed_steps = normalize_steps(&request.failed_steps);
        let (kind, severity, key, title, body, suffix) = match request.kind {
            ClientNotificationKind::AccountRecoveryFailed => (
                NotificationKind::AccountRecoveryFailed,
                NotificationSeverity::Error,
                "account.recoveryFailed",
                "账户恢复失败",
                "请检查账户设置后重试。",
                "recovery",
            ),
            ClientNotificationKind::AccountReconciliationFailed => (
                NotificationKind::AccountReconciliationFailed,
                NotificationSeverity::Critical,
                "account.reconciliationFailed",
                "账户对账失败",
                "请检查账户数据后重试。",
                "reconciliation",
            ),
        };
        controlled_input(
            &context,
            NotificationCategory::RiskAccount,
            kind,
            severity,
            NotificationContent::new(
                key,
                [("failedSteps", NotificationScalar::String(failed_steps))],
                title,
                body,
            ),
            Some(NotificationEntity {
                entity_type: NotificationEntityType::Account,
                id: context.account_id.clone(),
            }),
            Some(NotificationAction::OpenAccountSettings {
                account_section: AccountNotificationSection::Api,
            }),
            format!("{}:{}:{suffix}", context.account_id, request.attempt_id),
        )
    }

    pub fn connection_unavailable(
        &self,
        context: PolicyContext,
        channel: NotificationChannel,
        incident_id: &str,
    ) -> Result<NotificationInput, NotificationError> {
        self.connection_edge(context, channel, incident_id, false)
    }

    pub fn connection_recovered(
        &self,
        context: PolicyContext,
        channel: NotificationChannel,
        incident_id: &str,
    ) -> Result<NotificationInput, NotificationError> {
        self.connection_edge(context, channel, incident_id, true)
    }

    fn connection_edge(
        &self,
        context: PolicyContext,
        channel: NotificationChannel,
        incident_id: &str,
        recovered: bool,
    ) -> Result<NotificationInput, NotificationError> {
        validate_policy_identifier(incident_id, "连接故障标识无效")?;
        let (kind, severity, key, title, body, suffix) = if recovered {
            (
                NotificationKind::ConnectionRecovered,
                NotificationSeverity::Success,
                "connection.recovered",
                "连接已恢复",
                "交易连接已恢复。",
                "recovered",
            )
        } else {
            (
                NotificationKind::ConnectionUnavailable,
                NotificationSeverity::Error,
                "connection.unavailable",
                "连接不可用",
                "交易连接暂时不可用，请检查网络或稍后重试。",
                "unavailable",
            )
        };
        controlled_input(
            &context,
            NotificationCategory::ConnectionSystem,
            kind,
            severity,
            NotificationContent::new(
                key,
                [(
                    "channel",
                    NotificationScalar::String(channel_name(channel).into()),
                )],
                title,
                body,
            ),
            Some(NotificationEntity {
                entity_type: NotificationEntityType::Connection,
                id: channel_name(channel).into(),
            }),
            None,
            format!(
                "{}:{}:{incident_id}:{suffix}",
                context.account_id,
                channel_name(channel)
            ),
        )
    }

    pub fn environment_unavailable(
        &self,
        context: PolicyContext,
        environment: NotificationEnvironment,
        incident_id: &str,
    ) -> Result<NotificationInput, NotificationError> {
        self.environment_edge(context, environment, incident_id, false)
    }

    pub fn environment_recovered(
        &self,
        context: PolicyContext,
        environment: NotificationEnvironment,
        incident_id: &str,
    ) -> Result<NotificationInput, NotificationError> {
        self.environment_edge(context, environment, incident_id, true)
    }

    fn environment_edge(
        &self,
        context: PolicyContext,
        environment: NotificationEnvironment,
        incident_id: &str,
        recovered: bool,
    ) -> Result<NotificationInput, NotificationError> {
        validate_policy_identifier(incident_id, "环境故障标识无效")?;
        let (kind, severity, key, title, body, suffix) = if recovered {
            (
                NotificationKind::EnvironmentRecovered,
                NotificationSeverity::Success,
                "environment.recovered",
                "环境已恢复",
                "当前交易环境已恢复。",
                "recovered",
            )
        } else {
            (
                NotificationKind::EnvironmentUnavailable,
                NotificationSeverity::Error,
                "environment.unavailable",
                "环境不可达",
                "当前交易环境暂时不可达，请稍后重试。",
                "unavailable",
            )
        };
        controlled_input(
            &context,
            NotificationCategory::ConnectionSystem,
            kind,
            severity,
            NotificationContent::new(
                key,
                [(
                    "environment",
                    NotificationScalar::String(environment_name(environment).into()),
                )],
                title,
                body,
            ),
            Some(NotificationEntity {
                entity_type: NotificationEntityType::Environment,
                id: environment_name(environment).into(),
            }),
            None,
            format!(
                "{}:{}:{incident_id}:{suffix}",
                context.account_id,
                environment_name(environment)
            ),
        )
    }
}

fn controlled_input(
    context: &PolicyContext,
    category: NotificationCategory,
    kind: NotificationKind,
    severity: NotificationSeverity,
    content: Result<NotificationContent, crate::models::notification::NotificationValidationError>,
    entity: Option<NotificationEntity>,
    action: Option<NotificationAction>,
    dedupe_key: String,
) -> Result<NotificationInput, NotificationError> {
    let input = NotificationInput {
        scope: context.scope(),
        category,
        kind,
        severity,
        content: content.map_err(|error| NotificationError::new(error.code(), "通知内容无效"))?,
        entity,
        action,
        source_event_id: Some(context.source_event_id.clone()),
        dedupe_key,
        session_epoch: Some(context.session_epoch),
    };
    input
        .validate()
        .map_err(|error| NotificationError::new(error.code(), "通知输入无效"))?;
    Ok(input)
}

fn normalize_steps(steps: &[ClientNotificationFailedStep]) -> String {
    let mut steps = steps.to_vec();
    steps.sort_by_key(|step| match step {
        ClientNotificationFailedStep::Config => 0,
        ClientNotificationFailedStep::Profiles => 1,
        ClientNotificationFailedStep::Connection => 2,
        ClientNotificationFailedStep::Bootstrap => 3,
    });
    steps
        .into_iter()
        .map(|step| match step {
            ClientNotificationFailedStep::Config => "config",
            ClientNotificationFailedStep::Profiles => "profiles",
            ClientNotificationFailedStep::Connection => "connection",
            ClientNotificationFailedStep::Bootstrap => "bootstrap",
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn channel_name(channel: NotificationChannel) -> &'static str {
    match channel {
        NotificationChannel::Api => "api",
        NotificationChannel::Websocket => "websocket",
    }
}

fn environment_name(environment: NotificationEnvironment) -> &'static str {
    match environment {
        NotificationEnvironment::Production => "production",
        NotificationEnvironment::Development => "development",
        NotificationEnvironment::Unknown => "unknown",
    }
}

fn risk_code(code: RiskViolationCode) -> &'static str {
    match code {
        RiskViolationCode::InvalidQuantity => "invalidQuantity",
        RiskViolationCode::NonPositiveQuantity => "nonPositiveQuantity",
        RiskViolationCode::MaxOrderQty => "maxOrderQty",
        RiskViolationCode::MissingLimitPrice => "missingLimitPrice",
        RiskViolationCode::InvalidLimitPrice => "invalidLimitPrice",
        RiskViolationCode::NonPositiveLimitPrice => "nonPositiveLimitPrice",
        RiskViolationCode::MaxPriceDeviation => "maxPriceDeviation",
        RiskViolationCode::DailyOrderLimit => "dailyOrderLimit",
        RiskViolationCode::LedgerUnavailable => "ledgerUnavailable",
    }
}

fn validate_policy_identifier(value: &str, message: &'static str) -> Result<(), NotificationError> {
    if value.contains(':') || !is_safe_notification_source_event_id(value) {
        return Err(NotificationError::new(
            "INVALID_NOTIFICATION_CONTENT",
            message,
        ));
    }
    Ok(())
}
