use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionContext {
    pub submission_id: String,
    pub account_id: String,
    pub session_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderStreamContext {
    pub account_id: String,
    pub session_epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradingFailureKind {
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradingFailure {
    pub kind: TradingFailureKind,
}

impl TradingFailure {
    pub const fn rejected() -> Self {
        Self {
            kind: TradingFailureKind::Rejected,
        }
    }

    pub const fn user_message(&self) -> &'static str {
        match self.kind {
            TradingFailureKind::Rejected => "订单请求被交易端拒绝",
        }
    }
}

impl std::fmt::Display for TradingFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.user_message())
    }
}

impl std::error::Error for TradingFailure {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    #[serde(other)]
    Unknown,
}

impl OrderStatus {
    pub fn from_raw(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "new" | "created" | "active" | "untriggered" => Self::New,
            "partiallyfilled" | "partially_filled" => Self::PartiallyFilled,
            "filled" => Self::Filled,
            "cancelled" | "canceled" => Self::Cancelled,
            "rejected" => Self::Rejected,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub order_id: String,
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub price: String,
    pub qty: String,
    pub status: OrderStatus,
    pub order_link_id: Option<String>,
    pub filled_qty: String,
    pub avg_price: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub symbol: String,
    pub side: String,
    pub size: String,
    pub entry_price: String,
    pub leverage: String,
    pub unrealised_pnl: String,
    #[serde(default)]
    pub position_idx: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceOrderRequest {
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub qty: String,
    #[serde(default)]
    pub position_idx: i32,
    pub price: Option<String>,
    pub time_in_force: Option<String>,
    pub order_link_id: Option<String>,
    pub reduce_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelOrderRequest {
    pub symbol: String,
    pub order_id: Option<String>,
    pub order_link_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeStats {
    pub total_orders: u32,
    pub filled_orders: u32,
    pub cancelled_orders: u32,
    pub total_volume: String,
    pub realized_pnl: String,
    pub unrealised_pnl: String,
    pub win_rate_pct: String,
    pub win_count: u32,
    pub loss_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivatePanelsSnapshot {
    pub open_orders: Vec<Order>,
    pub order_history: Vec<Order>,
    pub positions: Vec<Position>,
}
