use super::*;
use crate::{error::AppResult, models::trading::OrderStatus};
use std::sync::Arc;

impl StrategySupervisor {
    pub(crate) async fn reconcile(self: &Arc<Self>, id: String) -> AppResult<StrategyRunView> {
        if uuid::Uuid::parse_str(&id).is_err() {
            return Err(error("plugin_strategy_invalid_request"));
        }
        let owner = self.clone();
        tokio::spawn(async move { owner.reconcile_owned(id).await })
            .await
            .map_err(|_| error("plugin_strategy_unavailable"))?
    }
    async fn reconcile_owned(&self, id: String) -> AppResult<StrategyRunView> {
        let _op = self.runtime.strategy_gate().await;
        let _account = self.host.lifecycle().mutation_guard().await;
        {
            let inner = self.lock()?;
            Self::ready(&inner)?;
            if inner.live.contains_key(&id) {
                return Err(error("plugin_strategy_busy"));
            }
        }
        let record = self.state(&id)?;
        let pending = record
            .pending
            .clone()
            .ok_or_else(|| error("plugin_strategy_invalid_request"))?;
        let current = self
            .host
            .authority_locked()
            .await
            .map_err(|_| error("plugin_strategy_stale"))?;
        if current.account.account_id != record.view.account.account_id
            || current.account.environment != record.view.account.environment
            || self
                .host
                .strategy_scope_locked()
                .await
                .map_err(|_| error("plugin_strategy_stale"))?
                != record.scope
        {
            return Err(error("plugin_strategy_stale"));
        }
        let exchange = match &pending.action {
            StrategyAction::CancelOrder { order } => Some(order.order_id.as_str()),
            _ => None,
        };
        let result = self
            .host
            .strategy_reconcile_locked(
                &record.scope,
                &record.view.symbol,
                pending.submission_id.as_deref(),
                exchange,
            )
            .await;
        let response = match result {
            Ok(Some(order))
                if super::execution::matches_response(&pending, &order)
                    && (exchange.is_none()
                        || matches!(
                            order.status,
                            OrderStatus::Filled | OrderStatus::Cancelled | OrderStatus::Rejected
                        )) =>
            {
                Ok(order)
            }
            Err(failure) if exchange.is_none() && super::execution::known_rejection(&failure) => {
                Err(failure)
            }
            _ => return Err(error("plugin_strategy_recovery_required")),
        };
        self.account_response(&id, &pending, response).await?;
        let record = self.state(&id)?;
        if record.pending.is_none() {
            self.mark(&id, StrategyStatus::Paused, "plugin_strategy_paused");
        }
        Ok(self.state(&id)?.view)
    }
}
