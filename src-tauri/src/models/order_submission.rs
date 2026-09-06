use serde::{Deserialize, Serialize};

use crate::models::trading::{Order, PlaceOrderRequest};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "order", rename_all = "camelCase")]
pub enum SubmissionOutcome {
    Pending,
    Accepted(Box<Order>),
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSubmission {
    pub schema_version: u32,
    pub scope: String,
    pub order_link_id: String,
    pub request: PlaceOrderRequest,
    pub created_at_ms: u64,
    pub outcome: SubmissionOutcome,
    #[serde(default)]
    pub acknowledged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingOrderSubmission {
    pub order_link_id: String,
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub qty: String,
    pub price: Option<String>,
    pub reduce_only: Option<bool>,
    pub created_at_ms: u64,
}

impl From<&StoredSubmission> for PendingOrderSubmission {
    fn from(record: &StoredSubmission) -> Self {
        Self {
            order_link_id: record.order_link_id.clone(),
            symbol: record.request.symbol.clone(),
            side: record.request.side.clone(),
            order_type: record.request.order_type.clone(),
            qty: record.request.qty.clone(),
            price: record.request.price.clone(),
            reduce_only: record.request.reduce_only,
            created_at_ms: record.created_at_ms,
        }
    }
}
