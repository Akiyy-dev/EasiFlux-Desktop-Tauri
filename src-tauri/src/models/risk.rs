use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskViolationCode {
    InvalidQuantity,
    NonPositiveQuantity,
    MaxOrderQty,
    MissingLimitPrice,
    InvalidLimitPrice,
    NonPositiveLimitPrice,
    ReferencePriceUnavailable,
    MaxPriceDeviation,
    DailyOrderLimit,
    LedgerUnavailable,
}

impl RiskViolationCode {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "invalidQuantity" => Self::InvalidQuantity,
            "nonPositiveQuantity" => Self::NonPositiveQuantity,
            "maxOrderQty" => Self::MaxOrderQty,
            "missingLimitPrice" => Self::MissingLimitPrice,
            "invalidLimitPrice" => Self::InvalidLimitPrice,
            "nonPositiveLimitPrice" => Self::NonPositiveLimitPrice,
            "referencePriceUnavailable" => Self::ReferencePriceUnavailable,
            "maxPriceDeviation" => Self::MaxPriceDeviation,
            "dailyOrderLimit" => Self::DailyOrderLimit,
            "ledgerUnavailable" => Self::LedgerUnavailable,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskViolationSafeParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
}

impl RiskViolationSafeParams {
    pub fn values_are_finite(self) -> bool {
        self.limit.is_none_or(f64::is_finite)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskViolation {
    pub code: RiskViolationCode,
    pub safe_params: RiskViolationSafeParams,
}

impl RiskViolation {
    pub fn new(code: RiskViolationCode) -> Self {
        Self {
            code,
            safe_params: RiskViolationSafeParams::default(),
        }
    }

    pub fn with_limit(code: RiskViolationCode, limit: f64) -> Self {
        Self {
            code,
            safe_params: RiskViolationSafeParams {
                limit: (limit.is_finite() && limit >= 0.0).then_some(limit),
            },
        }
    }

    pub fn user_message(&self) -> String {
        match (self.code, self.safe_params.limit) {
            (RiskViolationCode::InvalidQuantity, _) => "订单数量格式无效".into(),
            (RiskViolationCode::NonPositiveQuantity, _) => "订单数量必须大于 0".into(),
            (RiskViolationCode::MaxOrderQty, Some(limit)) => {
                format!("订单数量超过最大限制 {limit}")
            }
            (RiskViolationCode::MaxOrderQty, None) => "订单数量超过最大限制".into(),
            (RiskViolationCode::MissingLimitPrice, _) => "限价单必须提供价格".into(),
            (RiskViolationCode::InvalidLimitPrice, _) => "限价格式无效".into(),
            (RiskViolationCode::NonPositiveLimitPrice, _) => "限价必须大于 0".into(),
            (RiskViolationCode::ReferencePriceUnavailable, _) => {
                "无法获取有效的最新市价，请刷新行情后重试".into()
            }
            (RiskViolationCode::MaxPriceDeviation, Some(limit)) => {
                format!("限价偏离市价超过限制 {limit}%")
            }
            (RiskViolationCode::MaxPriceDeviation, None) => "限价偏离市价超过限制".into(),
            (RiskViolationCode::DailyOrderLimit, Some(limit)) => {
                format!("今日下单次数已达上限 {limit}")
            }
            (RiskViolationCode::DailyOrderLimit, None) => "今日下单次数已达上限".into(),
            (RiskViolationCode::LedgerUnavailable, _) => {
                "风控用量账本不可用，请检查本地存储权限或文件格式。".into()
            }
        }
    }
}

impl From<RiskViolation> for AppError {
    fn from(violation: RiskViolation) -> Self {
        Self::Risk(violation.user_message())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLedgerState {
    Disabled,
    Ready,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskStatus {
    pub enabled: bool,
    pub max_order_qty: String,
    pub max_price_deviation_pct: String,
    pub max_daily_orders: u32,
    pub trading_day_timezone: String,
    pub ledger_state: RiskLedgerState,
    pub trading_day: String,
    pub occupied_orders: Option<u32>,
    pub remaining_orders: Option<u32>,
    pub updated_at_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRiskConfigRequest {
    pub enabled: bool,
    pub max_order_qty: String,
    pub max_price_deviation_pct: String,
    pub max_daily_orders: u32,
    pub trading_day_timezone: String,
}
