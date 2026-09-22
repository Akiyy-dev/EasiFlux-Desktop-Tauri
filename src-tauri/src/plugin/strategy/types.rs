use super::*;
use crate::{
    error::AppResult,
    plugin::{
        manifest::deserialize_object,
        workflow::{self, Account, CancelProposal, PlaceProposal},
    },
};
use serde::{Deserialize, Deserializer, Serialize};

macro_rules! object {
    ($name:ident { $($(#[$a:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Clone,Debug,PartialEq,Eq,Serialize)]
        #[serde(rename_all="camelCase")]
        pub(crate) struct $name { $(pub(crate) $field:$ty),* }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D:Deserializer<'de>>(d:D)->Result<Self,D::Error> {
                #[derive(Deserialize)] #[serde(rename_all="camelCase",deny_unknown_fields)]
                struct Wire { $($(#[$a])* $field:$ty),* }
                let v=deserialize_object::<D,Wire>(d)?; Ok(Self { $($field:v.$field),* })
            }
        }
    }
}
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    Option::deserialize(d)
}
object!(StrategyPolicy {
    interval_ms: u64,
    max_order_qty: String,
    max_total_qty: String,
    max_actions: u32,
    max_run_seconds: u64,
    reduce_only: bool
});
object!(StrategyAccess { schema_version:u8,plugin_id:String,contribution_id:String,catalog_generation:String,revision:String,account:Account,requested_capabilities:Vec<String>,authorization_token:String,expires_at_ms:String });
object!(StrategyStartRequest { authorization_token:String,request_id:String,#[serde(deserialize_with="nullable")] resume_run_id:Option<String>,symbol:String,input_json:String,capabilities:Vec<String>,policy:StrategyPolicy,acknowledge_automatic_trading:bool });
object!(StrategyControlRequest {
    run_id: String,
    action: ControlAction
});
object!(StrategyRunView { schema_version:u8,run_id:String,request_id:String,plugin_id:String,contribution_id:String,account:Account,symbol:String,status:StrategyStatus,#[serde(deserialize_with="nullable")] reason:Option<String>,policy:StrategyPolicy,capabilities:Vec<String>,input_json:String,started_at_ms:String,expires_at_ms:String,sequence:String,actions_submitted:u32,total_submitted_qty:String,last_message:String,#[serde(deserialize_with="nullable")] last_receipt:Option<StrategyReceipt> });
object!(StrategyList { schema_version:u8,runs:Vec<StrategyRunView> });
object!(StrategyReceipt { sequence:String,kind:ReceiptKind,status:ReceiptStatus,#[serde(deserialize_with="nullable")] submission_id:Option<String>,#[serde(deserialize_with="nullable")] order_id:Option<String>,#[serde(deserialize_with="nullable")] error_code:Option<String> });
object!(StrategyOutput {
    state: serde_json::Value,
    action: StrategyAction,
    message: String
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ControlAction {
    Pause,
    Stop,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum StrategyStatus {
    Running,
    Paused,
    Stopping,
    Stopped,
    RecoveryRequired,
    Faulted,
    Completed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ReceiptKind {
    PlaceOrder,
    CancelOrder,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ReceiptStatus {
    Accepted,
    Rejected,
    Unknown,
}
macro_rules! literal {
    ($t:ty,$($text:literal=>$variant:ident),+)=>{
        impl<'de> Deserialize<'de> for $t { fn deserialize<D:Deserializer<'de>>(d:D)->Result<Self,D::Error>{match String::deserialize(d)?.as_str(){$($text=>Ok(Self::$variant),)+ _=>Err(serde::de::Error::custom("invalid strategy literal"))}} }
    }
}
literal!(ControlAction,"pause"=>Pause,"stop"=>Stop);
literal!(StrategyStatus,"running"=>Running,"paused"=>Paused,"stopping"=>Stopping,"stopped"=>Stopped,"recoveryRequired"=>RecoveryRequired,"faulted"=>Faulted,"completed"=>Completed);
literal!(ReceiptKind,"placeOrder"=>PlaceOrder,"cancelOrder"=>CancelOrder);
literal!(ReceiptStatus,"accepted"=>Accepted,"rejected"=>Rejected,"unknown"=>Unknown);
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum StrategyAction {
    None,
    Stop,
    PlaceOrder { order: PlaceProposal },
    CancelOrder { order: CancelProposal },
}
impl<'de> Deserialize<'de> for StrategyAction {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
        enum Wire {
            None {},
            Stop {},
            PlaceOrder { order: PlaceProposal },
            CancelOrder { order: CancelProposal },
        }
        Ok(match deserialize_object::<D, Wire>(d)? {
            Wire::None {} => Self::None,
            Wire::Stop {} => Self::Stop,
            Wire::PlaceOrder { order } => Self::PlaceOrder { order },
            Wire::CancelOrder { order } => Self::CancelOrder { order },
        })
    }
}
pub(crate) const REASONS: &[&str] = &[
    "plugin_strategy_paused",
    "plugin_strategy_stopped",
    "plugin_strategy_restarted",
    "plugin_strategy_expired",
    "plugin_strategy_limit_reached",
    "plugin_strategy_stale",
    "plugin_strategy_data_unavailable",
    "plugin_strategy_compute_failed",
    "plugin_strategy_invalid_output",
    "plugin_strategy_recovery_required",
    "plugin_strategy_storage_unavailable",
    "plugin_strategy_ack_failed",
    "plugin_strategy_unavailable",
];
pub(crate) fn quantity(value: &str) -> AppResult<rust_decimal::Decimal> {
    workflow::decimal(value, true).map_err(|_| error("plugin_strategy_invalid_request"))?;
    rust_decimal::Decimal::from_str_exact(value)
        .map_err(|_| error("plugin_strategy_invalid_request"))
}
pub(crate) fn validate_state(state: &serde_json::Value) -> AppResult<()> {
    if !state.is_object()
        || serde_json::to_vec(state)
            .map_err(|_| error("plugin_strategy_invalid_output"))?
            .len()
            > 4096
    {
        return Err(error("plugin_strategy_invalid_output"));
    }
    Ok(())
}
impl StrategyPolicy {
    pub(crate) fn validate(&self) -> AppResult<()> {
        if !(5000..=60000).contains(&self.interval_ms)
            || !(60..=86400).contains(&self.max_run_seconds)
            || !(1..=1000).contains(&self.max_actions)
            || quantity(&self.max_order_qty)? > quantity(&self.max_total_qty)?
        {
            return Err(error("plugin_strategy_invalid_request"));
        }
        Ok(())
    }
}
impl StrategyOutput {
    pub(crate) fn validate(&self) -> AppResult<()> {
        validate_state(&self.state)?;
        if self.message.len() > 2000 {
            return Err(error("plugin_strategy_invalid_output"));
        }
        Ok(())
    }
}
impl StrategyStartRequest {
    pub(crate) fn validate(&self) -> AppResult<()> {
        for id in [&self.authorization_token, &self.request_id]
            .into_iter()
            .chain(self.resume_run_id.iter())
        {
            if uuid::Uuid::parse_str(id).is_err() {
                return Err(error("plugin_strategy_invalid_request"));
            }
        }
        if !self.acknowledge_automatic_trading {
            return Err(error("plugin_strategy_denied"));
        }
        workflow::symbol(&self.symbol).map_err(|_| error("plugin_strategy_invalid_request"))?;
        workflow::validate_input(&self.input_json)
            .map_err(|_| error("plugin_strategy_invalid_request"))?;
        validate_capabilities(&self.capabilities).map_err(|_| error("plugin_strategy_denied"))?;
        if self.capabilities.iter().any(|c| c == "trade.cancel")
            && !self.capabilities.iter().any(|c| c == "orders.read")
        {
            return Err(error("plugin_strategy_denied"));
        }
        self.policy.validate()
    }
}
impl StrategyRunView {
    pub(crate) fn validate(&self) -> AppResult<()> {
        self.policy.validate()?;
        validate_capabilities(&self.capabilities)
            .map_err(|_| error("plugin_strategy_storage_unavailable"))?;
        workflow::symbol(&self.symbol)?;
        workflow::validate_input(&self.input_json)
            .map_err(|_| error("plugin_strategy_storage_unavailable"))?;
        for id in [&self.plugin_id, &self.contribution_id] {
            crate::plugin::manifest::PluginId::parse(id)
                .map_err(|_| error("plugin_strategy_storage_unavailable"))?;
        }
        for id in [&self.run_id, &self.request_id] {
            if uuid::Uuid::parse_str(id).is_err() {
                return Err(error("plugin_strategy_storage_unavailable"));
            }
        }
        let start = workflow::counter(&self.started_at_ms)?;
        let expiry = workflow::counter(&self.expires_at_ms)?;
        workflow::counter(&self.sequence)?;
        workflow::counter(&self.account.session_epoch)?;
        let total = rust_decimal::Decimal::from_str_exact(&workflow::decimal(
            &self.total_submitted_qty,
            false,
        )?)
        .map_err(|_| error("plugin_strategy_storage_unavailable"))?;
        if self.schema_version != 1
            || !workflow::bounded_id(&self.account.account_id)
            || self.account.environment.is_empty()
            || self.account.environment.len() > 256
            || self.account.environment.chars().any(char::is_control)
            || expiry
                != start
                    .checked_add(self.policy.max_run_seconds * 1000)
                    .ok_or_else(|| error("plugin_strategy_storage_unavailable"))?
            || self.actions_submitted > self.policy.max_actions
            || u64::from(self.actions_submitted) > workflow::counter(&self.sequence)?
            || total < rust_decimal::Decimal::ZERO
            || total > quantity(&self.policy.max_total_qty)?
            || self.last_message.len() > 2000
            || self
                .reason
                .as_ref()
                .is_some_and(|c| !REASONS.contains(&c.as_str()))
        {
            return Err(error("plugin_strategy_storage_unavailable"));
        }
        if let Some(receipt) = &self.last_receipt {
            let expected_code = match receipt.status {
                ReceiptStatus::Accepted => None,
                ReceiptStatus::Rejected => Some("plugin_strategy_rejected"),
                ReceiptStatus::Unknown => Some("plugin_strategy_unknown"),
            };
            if workflow::counter(&receipt.sequence)? > workflow::counter(&self.sequence)?
                || receipt.error_code.as_deref() != expected_code
                || receipt
                    .order_id
                    .as_ref()
                    .is_some_and(|id| !workflow::bounded_id(id))
                || receipt
                    .submission_id
                    .as_ref()
                    .is_some_and(|id| uuid::Uuid::parse_str(id).is_err())
                || (receipt.kind == ReceiptKind::PlaceOrder) != receipt.submission_id.is_some()
                || (receipt.status == ReceiptStatus::Accepted && receipt.order_id.is_none())
            {
                return Err(error("plugin_strategy_storage_unavailable"));
            }
        }
        Ok(())
    }
}
