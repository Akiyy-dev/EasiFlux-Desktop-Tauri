use super::error;
use crate::error::AppResult;
use crate::models::{
    account::Balance,
    trading::{Order, OrderStatus, Position},
};
use crate::plugin::manifest::deserialize_object;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};

pub(crate) const CAPABILITIES: &[&str] = &[
    "account.read",
    "balances.read",
    "positions.read",
    "orders.read",
    "market.read",
    "trade.place",
    "trade.cancel",
];

pub(crate) fn validate_capabilities(values: &[String]) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for value in values {
        if !CAPABILITIES.contains(&value.as_str()) || !seen.insert(value) {
            return Err("invalid or duplicate capability".into());
        }
    }
    if !values.is_empty() && !values.iter().any(|v| v == "account.read") {
        return Err("account.read required".into());
    }
    Ok(())
}

pub(crate) fn validate_input(value: &str) -> Result<(), String> {
    if value.len() > 4096
        || !serde_json::from_str::<serde_json::Value>(value).is_ok_and(|v| v.is_object())
    {
        return Err("input must be a bounded JSON object".into());
    }
    Ok(())
}

// Force map representation while retaining serde's duplicate/unknown field rejection.
macro_rules! object {
    ($name:ident { $($(#[$attr:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub(crate) struct $name { $(pub(crate) $field: $ty),* }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Wire { $($(#[$attr])* $field: $ty),* }
                let v = deserialize_object::<D, Wire>(d)?;
                Ok(Self { $($field: v.$field),* })
            }
        }
    }
}

object!(PluginWorkflowParams {
    runtime: String,
    abi: String,
    module_base64: String,
    default_input: String
});
impl PluginWorkflowParams {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.runtime != "wasm-v1" || self.abi != "account-json-v1" {
            return Err("unsupported workflow ABI".into());
        }
        validate_input(&self.default_input)?;
        self.module_bytes().map(|_| ())
    }
    pub(crate) fn module_bytes(&self) -> Result<Vec<u8>, String> {
        if self.module_base64.len() > 8192usize.div_ceil(3) * 4 {
            return Err("module too large".into());
        }
        let bytes = STANDARD
            .decode(&self.module_base64)
            .map_err(|_| "invalid module encoding")?;
        if bytes.len() > 8192
            || !bytes.starts_with(b"\0asm\x01\0\0\0")
            || STANDARD.encode(&bytes) != self.module_base64
        {
            return Err("invalid module encoding".into());
        }
        Ok(bytes)
    }
}

object!(AuthorityRequest {
    plugin_id: String,
    contribution_id: String,
    expected_catalog_generation: String,
    expected_revision: String
});
object!(GrantRequest { plugin_id: String, contribution_id: String, expected_catalog_generation: String, expected_revision: String, expected_account_id: String, expected_session_epoch: String, expected_grant_revision: String, capabilities: Vec<String> });
object!(RunRequest {
    plugin_id: String,
    contribution_id: String,
    expected_catalog_generation: String,
    expected_revision: String,
    request_id: String,
    expected_account_id: String,
    expected_session_epoch: String,
    expected_grant_revision: String,
    symbol: String,
    input_json: String
});
object!(Account {
    account_id: String,
    session_epoch: String,
    environment: String
});
object!(PlaceProposal { symbol: String, side: String, order_type: String, qty: String, #[serde(deserialize_with = "required_nullable")] price: Option<String>, time_in_force: String, position_idx: i32, reduce_only: bool });
fn required_nullable<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}
object!(CancelProposal {
    symbol: String,
    order_id: String
});

impl AuthorityRequest {
    pub(crate) fn validate(&self) -> AppResult<()> {
        for id in [&self.plugin_id, &self.contribution_id] {
            crate::plugin::manifest::PluginId::parse(id)
                .map_err(|_| error("plugin_workflow_invalid_request"))?;
        }
        counter(&self.expected_catalog_generation)?;
        counter(&self.expected_revision)?;
        Ok(())
    }
}
macro_rules! authority {
    ($name:ident) => {
        impl $name {
            pub(crate) fn authority(&self) -> AuthorityRequest {
                AuthorityRequest {
                    plugin_id: self.plugin_id.clone(),
                    contribution_id: self.contribution_id.clone(),
                    expected_catalog_generation: self.expected_catalog_generation.clone(),
                    expected_revision: self.expected_revision.clone(),
                }
            }
        }
    };
}
authority!(GrantRequest);
authority!(RunRequest);
pub(crate) fn counter(value: &str) -> AppResult<u64> {
    value
        .parse::<u64>()
        .ok()
        .filter(|v| v.to_string() == value)
        .ok_or_else(|| error("plugin_workflow_invalid_request"))
}
pub(crate) fn symbol(value: &str) -> AppResult<()> {
    if !(1..=32).contains(&value.len())
        || !value
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    {
        return Err(error("plugin_workflow_invalid_request"));
    }
    Ok(())
}
pub(crate) fn decimal(value: &str, positive: bool) -> AppResult<String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.' || (!positive && b == b'-'))
    {
        return Err(error("plugin_workflow_invalid_output"));
    }
    let parsed =
        Decimal::from_str_exact(value).map_err(|_| error("plugin_workflow_invalid_output"))?;
    let unsigned = value.strip_prefix('-').unwrap_or(value);
    if unsigned.starts_with('.') || unsigned.ends_with('.') {
        return Err(error("plugin_workflow_invalid_output"));
    }
    if positive && parsed <= Decimal::ZERO {
        return Err(error("plugin_workflow_invalid_output"));
    }
    Ok(parsed.normalize().to_string())
}
pub(crate) fn bounded_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum WorkflowOutput {
    Display { text: String },
    PlaceOrder { order: PlaceProposal },
    CancelOrder { order: CancelProposal },
}
impl<'de> Deserialize<'de> for WorkflowOutput {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
        enum Wire {
            Display { text: String },
            PlaceOrder { order: PlaceProposal },
            CancelOrder { order: CancelProposal },
        }
        Ok(match deserialize_object::<D, Wire>(d)? {
            Wire::Display { text } => Self::Display { text },
            Wire::PlaceOrder { order } => Self::PlaceOrder { order },
            Wire::CancelOrder { order } => Self::CancelOrder { order },
        })
    }
}
impl WorkflowOutput {
    pub(crate) fn validate(
        mut self,
        expected_symbol: &str,
        capabilities: &[String],
        orders: Option<&Section<Order>>,
    ) -> AppResult<Self> {
        let has = |c: &str| capabilities.iter().any(|v| v == c);
        match &mut self {
            Self::Display { text } => {
                if text.len() > 2000 {
                    return Err(error("plugin_workflow_invalid_output"));
                }
            }
            Self::PlaceOrder { order } => {
                if !has("trade.place") {
                    return Err(error("plugin_workflow_denied"));
                }
                if order.symbol != expected_symbol
                    || !["Buy", "Sell"].contains(&order.side.as_str())
                    || order.position_idx
                        != if (order.side == "Buy") != order.reduce_only {
                            1
                        } else {
                            2
                        }
                {
                    return Err(error("plugin_workflow_invalid_output"));
                }
                symbol(&order.symbol)?;
                order.qty = decimal(&order.qty, true)?;
                match order.order_type.as_str() {
                    "Limit" if ["GTC", "IOC", "FOK"].contains(&order.time_in_force.as_str()) => {
                        order.price = Some(decimal(
                            order
                                .price
                                .as_deref()
                                .ok_or_else(|| error("plugin_workflow_invalid_output"))?,
                            true,
                        )?);
                    }
                    "Market" if order.price.is_none() && order.time_in_force == "IOC" => {}
                    _ => return Err(error("plugin_workflow_invalid_output")),
                }
            }
            Self::CancelOrder { order } => {
                if !has("trade.cancel") || !has("orders.read") {
                    return Err(error("plugin_workflow_denied"));
                }
                if order.symbol != expected_symbol
                    || !bounded_id(&order.order_id)
                    || !orders.is_some_and(|section| {
                        section.items.iter().any(|v| {
                            v.symbol == expected_symbol
                                && v.order_id == order.order_id
                                && matches!(
                                    v.status,
                                    OrderStatus::New | OrderStatus::PartiallyFilled
                                )
                        })
                    })
                {
                    return Err(error("plugin_workflow_invalid_output"));
                }
            }
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Section<T> {
    pub(crate) items: Vec<T>,
    pub(crate) fetched_at_ms: String,
    pub(crate) partial: bool,
}
object!(Quote {
    symbol: String,
    last_price: String,
    bid_price: String,
    ask_price: String,
    mark_price: String
});
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarketSection {
    pub(crate) ticker: Quote,
    pub(crate) fetched_at_ms: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    pub(crate) schema_version: u8,
    pub(crate) account: Account,
    pub(crate) captured_at_ms: String,
    pub(crate) symbol: String,
    pub(crate) granted_capabilities: Vec<String>,
    pub(crate) balances: Option<Section<Balance>>,
    pub(crate) positions: Option<Section<Position>>,
    pub(crate) orders: Option<Section<Order>>,
    pub(crate) market: Option<MarketSection>,
}
object!(Access { schema_version: u8, plugin_id: String, contribution_id: String, catalog_generation: String, revision: String, account: Account, requested_capabilities: Vec<String>, granted_capabilities: Vec<String>, grant_revision: String });
object!(Confirmation { token: String, expires_at_ms: String, submission_id: Option<String> });
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkflowResult {
    pub(crate) schema_version: u8,
    pub(crate) request_id: String,
    pub(crate) plugin_id: String,
    pub(crate) contribution_id: String,
    pub(crate) catalog_generation: String,
    pub(crate) revision: String,
    pub(crate) account: Account,
    pub(crate) grant_revision: String,
    pub(crate) snapshot: Snapshot,
    pub(crate) output: WorkflowOutput,
    pub(crate) confirmation: Option<Confirmation>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TradeStatus {
    Accepted,
    Rejected,
    Unknown,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TradeReceipt {
    pub(crate) schema_version: u8,
    pub(crate) token: String,
    pub(crate) account: Account,
    pub(crate) action: String,
    pub(crate) status: TradeStatus,
    pub(crate) submission_id: Option<String>,
    pub(crate) order: Option<Order>,
    pub(crate) error_code: Option<String>,
}
