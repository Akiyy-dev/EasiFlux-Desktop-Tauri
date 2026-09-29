use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionContext {
    pub submission_id: String,
    pub account_id: String,
    pub session_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionContext {
    pub account_id: String,
    pub session_epoch: u64,
}

pub use SessionContext as OrderStreamContext;

impl From<&SubmissionContext> for SessionContext {
    fn from(context: &SubmissionContext) -> Self {
        Self {
            account_id: context.account_id.clone(),
            session_epoch: context.session_epoch,
        }
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection: Option<OrderProtection>,
}

/// Exchange receipt identity only; it does not describe current order state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderAcknowledgement {
    pub order_id: String,
    pub order_link_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderProtection {
    pub take_profit: Option<String>,
    pub stop_loss: Option<String>,
    pub trigger_by: String,
}
impl OrderProtection {
    pub fn validate_shape(&self) -> Result<(), String> {
        if !["LastPrice", "MarkPrice"].contains(&self.trigger_by.as_str())
            || (self.take_profit.is_none() && self.stop_loss.is_none()) {
            return Err("invalid protection selector or empty legs".into());
        }
        for value in self.take_profit.iter().chain(self.stop_loss.iter()) {
            if value.is_empty() || value.len() > 64
                || !value.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                || value.starts_with('.') || value.ends_with('.')
                || !rust_decimal::Decimal::from_str_exact(value).is_ok_and(|n| n > rust_decimal::Decimal::ZERO) {
                return Err("invalid protection price".into());
            }
        }
        Ok(())
    }
}
impl<'de> Deserialize<'de> for OrderProtection {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(deserialize_with = "required_nullable_price")]
            take_profit: Option<String>,
            #[serde(deserialize_with = "required_nullable_price")]
            stop_loss: Option<String>,
            trigger_by: String,
        }
        struct ObjectVisitor;
        impl<'de> serde::de::Visitor<'de> for ObjectVisitor {
            type Value = OrderProtection;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an exact protection object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                let wire = Wire::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                let value = OrderProtection { take_profit: wire.take_profit, stop_loss: wire.stop_loss, trigger_by: wire.trigger_by };
                value.validate_shape().map_err(serde::de::Error::custom)?;
                Ok(value)
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}
fn required_nullable_price<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::deserialize(d)
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
