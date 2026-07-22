use serde::{Deserialize, Serialize};

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
