use crate::{
    error::AppResult,
    models::trading::{OrderProtection, PlaceOrderRequest},
    plugin::{manifest::deserialize_object, workflow},
};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AmendProposal {
    pub(crate) symbol: String,
    pub(crate) order_id: String,
    pub(crate) price: String,
    pub(crate) qty: String,
}
impl<'de> Deserialize<'de> for AmendProposal {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            symbol: String,
            order_id: String,
            price: String,
            qty: String,
        }
        let w = deserialize_object::<D, Wire>(d)?;
        Ok(Self {
            symbol: w.symbol,
            order_id: w.order_id,
            price: w.price,
            qty: w.qty,
        })
    }
}
impl AmendProposal {
    pub(crate) fn validate(&mut self, symbol: &str) -> AppResult<()> {
        if self.symbol != symbol || !workflow::bounded_id(&self.order_id) {
            return Err(super::error("plugin_strategy_invalid_output"));
        }
        self.price = workflow::decimal(&self.price, true)?;
        self.qty = workflow::decimal(&self.qty, true)?;
        Ok(())
    }
}

impl OrderProtection {
    pub(crate) fn validate_structure(&self) -> AppResult<()> {
        self.validate_shape()
            .map_err(|_| super::error("plugin_strategy_invalid_output"))
    }
    pub(crate) fn validate(
        &mut self,
        side: &str,
        entry: Option<&str>,
        reference: &str,
    ) -> AppResult<()> {
        self.validate_structure()?;
        let reference = rust_decimal::Decimal::from_str_exact(&workflow::decimal(reference, true)?)
            .map_err(|_| super::error("plugin_strategy_invalid_output"))?;
        let entry = entry
            .map(|v| workflow::decimal(v, true))
            .transpose()?
            .map(|v| rust_decimal::Decimal::from_str_exact(&v))
            .transpose()
            .map_err(|_| super::error("plugin_strategy_invalid_output"))?;
        for (value, take) in [(&mut self.take_profit, true), (&mut self.stop_loss, false)] {
            if let Some(raw) = value {
                *raw = workflow::decimal(raw, true)?;
                let n = rust_decimal::Decimal::from_str_exact(raw)
                    .map_err(|_| super::error("plugin_strategy_invalid_output"))?;
                let above = (side == "Buy") == take;
                if !(if above {
                    n > reference && entry.is_none_or(|e| n > e)
                } else {
                    n < reference && entry.is_none_or(|e| n < e)
                }) {
                    return Err(super::error("plugin_strategy_invalid_output"));
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_canonical_placement(id: &str, request: &PlaceOrderRequest) -> AppResult<()> {
    if uuid::Uuid::parse_str(id).is_err() || request.order_link_id.as_deref() != Some(id) {
        return Err(super::error("plugin_strategy_storage_unavailable"));
    }
    let tif = match request.time_in_force.as_deref() {
        Some("GoodTillCancel") => "GTC",
        Some("ImmediateOrCancel") => "IOC",
        Some("FillOrKill") => "FOK",
        _ => return Err(super::error("plugin_strategy_storage_unavailable")),
    };
    let proposal = workflow::PlaceProposal {
        symbol: request.symbol.clone(),
        side: request.side.clone(),
        order_type: request.order_type.clone(),
        qty: request.qty.clone(),
        price: request.price.clone(),
        time_in_force: tif.into(),
        position_idx: request.position_idx,
        reduce_only: request
            .reduce_only
            .ok_or_else(|| super::error("plugin_strategy_storage_unavailable"))?,
    };
    let workflow::WorkflowOutput::PlaceOrder { order } =
        (workflow::WorkflowOutput::PlaceOrder { order: proposal }).validate(
            &request.symbol,
            &["trade.place".into()],
            None,
        )?
    else {
        unreachable!()
    };
    if order.qty != request.qty || order.price != request.price {
        return Err(super::error("plugin_strategy_storage_unavailable"));
    }
    if let Some(protection) = &request.protection {
        if request.reduce_only != Some(false) {
            return Err(super::error("plugin_strategy_storage_unavailable"));
        }
        let mut normalized = protection.clone();
        if request.order_type == "Limit" {
            let entry = request
                .price
                .as_deref()
                .ok_or_else(|| super::error("plugin_strategy_storage_unavailable"))?;
            normalized.validate(&request.side, Some(entry), entry)?;
        } else {
            normalized.validate_structure()?;
        }
        if &normalized != protection {
            return Err(super::error("plugin_strategy_storage_unavailable"));
        }
    }
    Ok(())
}
