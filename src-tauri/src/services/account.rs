use std::sync::Arc;

use crate::api::diagnostic::{warn_if_parse_empty, warn_if_raw_parsed_mismatch};
use crate::api::endpoints;
use crate::api::mapper::{build_order_query_params, parse_balances, parse_positions};
use crate::api::ApiClient;
use crate::error::AppResult;
use crate::events::EventEmitter;
use crate::models::account::{AccountSummary, Balance};
use crate::models::trading::{Position, SessionContext};
use crate::services::AnalyticsService;

pub struct AccountService {
    api: Arc<ApiClient>,
    emitter: EventEmitter,
    analytics: Arc<AnalyticsService>,
}

impl AccountService {
    pub fn new(
        api: Arc<ApiClient>,
        emitter: EventEmitter,
        analytics: Arc<AnalyticsService>,
    ) -> Self {
        Self {
            api,
            emitter,
            analytics,
        }
    }

    pub async fn refresh_balances(&self, context: &SessionContext) -> AppResult<Vec<Balance>> {
        let params =
            build_order_query_params(None, None, None, None, None, None, None, None, None, None);
        let payload = self.api.private_get(endpoints::BALANCES, params).await?;
        let balances = parse_balances(&payload);
        warn_if_parse_empty(&self.emitter, "account/balance", &payload, balances.len());
        for balance in &balances {
            self.emitter.emit_balance(context, balance.clone());
        }
        Ok(balances)
    }

    pub async fn refresh_positions(
        &self,
        context: &SessionContext,
        symbol: Option<&str>,
    ) -> AppResult<Vec<Position>> {
        let params =
            build_order_query_params(symbol, None, None, None, None, None, None, None, None, None);
        let payload = self.api.private_get(endpoints::POSITIONS, params).await?;
        let meta = crate::api::mapper::list_envelope_meta(&payload);
        let positions = parse_positions(&payload);
        warn_if_parse_empty(&self.emitter, "position/list", &payload, positions.len());
        warn_if_raw_parsed_mismatch(&self.emitter, "position/list", &meta, positions.len());
        for position in &positions {
            self.emitter.emit_position(context, position.clone());
            self.analytics.record_position(position.clone()).await;
        }
        Ok(positions)
    }

    pub async fn refresh_account(
        &self,
        context: &SessionContext,
        _symbol: Option<&str>,
    ) -> AppResult<AccountSummary> {
        let balances = self.refresh_balances(context).await?;
        let _positions = self.refresh_positions(context, None).await?;
        let total_equity = balances
            .iter()
            .map(|b| b.total.parse::<f64>().unwrap_or(0.0))
            .sum::<f64>()
            .to_string();
        Ok(AccountSummary {
            account_id: context.account_id.clone(),
            balances,
            total_equity,
        })
    }
}
