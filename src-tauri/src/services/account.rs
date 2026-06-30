use std::sync::Arc;

use crate::api::{ApiClient, PrivateApi};
use crate::error::AppResult;
use crate::events::EventEmitter;
use crate::models::account::{AccountSummary, Balance};
use crate::models::trading::Position;

pub struct AccountService {
    api: Arc<ApiClient>,
    emitter: EventEmitter,
}

impl AccountService {
    pub fn new(api: Arc<ApiClient>, emitter: EventEmitter) -> Self {
        Self { api, emitter }
    }

    pub async fn refresh_balances(&self) -> AppResult<Vec<Balance>> {
        let balances = PrivateApi::balances(&self.api).await?;
        for balance in &balances {
            self.emitter.emit_balance(balance.clone());
        }
        Ok(balances)
    }

    pub async fn refresh_positions(&self, symbol: Option<&str>) -> AppResult<Vec<Position>> {
        let positions = PrivateApi::positions(&self.api, symbol).await?;
        for position in &positions {
            self.emitter.emit_position(position.clone());
        }
        Ok(positions)
    }

    pub async fn refresh_account(
        &self,
        account_id: &str,
        symbol: Option<&str>,
    ) -> AppResult<AccountSummary> {
        let balances = self.refresh_balances().await?;
        let _positions = self.refresh_positions(symbol).await?;
        let total_equity = balances
            .iter()
            .map(|b| b.total.parse::<f64>().unwrap_or(0.0))
            .sum::<f64>()
            .to_string();
        Ok(AccountSummary {
            account_id: account_id.to_string(),
            balances,
            total_equity,
        })
    }
}
