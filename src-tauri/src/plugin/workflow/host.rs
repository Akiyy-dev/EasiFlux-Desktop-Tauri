use super::*;
use crate::{
    error::AppResult,
    models::{
        account::Balance,
        trading::{
            CancelOrderRequest, Order, PlaceOrderRequest, Position, SessionContext,
            SubmissionContext,
        },
    },
    services::AccountLifecycleCoordinator,
};
use std::{future::Future, pin::Pin, sync::Arc};

pub(crate) type HostFuture<'a, T> = Pin<Box<dyn Future<Output = AppResult<T>> + Send + 'a>>;
/// Native only. Deliberately neither Debug nor Serialize: authority never enters IPC/guest JSON.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct AccountAuthority {
    pub(crate) account: Account,
    pub(crate) private_session: String,
}

pub(crate) trait WorkflowHost: Send + Sync {
    fn lifecycle(&self) -> &AccountLifecycleCoordinator;
    fn now_ms(&self) -> u64;
    // Caller holds the lifecycle read/write guard. Never acquire it recursively.
    fn authority_locked(&self) -> HostFuture<'_, AccountAuthority>;
    fn balances_locked(&self) -> HostFuture<'_, Vec<Balance>>;
    fn positions_locked<'a>(&'a self, symbol: &'a str) -> HostFuture<'a, Vec<Position>>;
    fn orders_locked<'a>(&'a self, symbol: &'a str) -> HostFuture<'a, Vec<Order>>;
    fn market_locked<'a>(&'a self, symbol: &'a str) -> HostFuture<'a, Quote>;
    fn place_locked(
        &self,
        context: SubmissionContext,
        request: PlaceOrderRequest,
    ) -> HostFuture<'_, Order>;
    fn cancel_locked(
        &self,
        context: SessionContext,
        request: CancelOrderRequest,
    ) -> HostFuture<'_, Order>;
}

pub(crate) struct WorkflowHostState(pub(crate) Arc<dyn WorkflowHost>);

pub(crate) async fn snapshot_locked(
    host: &dyn WorkflowHost,
    authority: &AccountAuthority,
    symbol_value: &str,
    capabilities: &[String],
) -> AppResult<Snapshot> {
    let has = |c: &str| capabilities.iter().any(|v| v == c);
    let safe = || error("plugin_workflow_data_unavailable");
    let mut snapshot = Snapshot {
        schema_version: 1,
        account: authority.account.clone(),
        captured_at_ms: String::new(),
        symbol: symbol_value.into(),
        granted_capabilities: capabilities.to_vec(),
        balances: None,
        positions: None,
        orders: None,
        market: None,
    };
    if has("balances.read") {
        let mut items = host.balances_locked().await.map_err(|_| safe())?;
        let fetched_at_ms = host.now_ms().to_string();
        items.truncate(100);
        for b in &items {
            symbol(&b.asset).map_err(|_| safe())?;
            for value in [&b.available, &b.frozen, &b.total] {
                decimal(value, false).map_err(|_| safe())?;
            }
        }
        snapshot.balances = Some(Section {
            items,
            fetched_at_ms,
            partial: true,
        });
    }
    if has("positions.read") {
        let mut items = host
            .positions_locked(symbol_value)
            .await
            .map_err(|_| safe())?;
        let fetched_at_ms = host.now_ms().to_string();
        items.truncate(100);
        for p in &items {
            if p.symbol != symbol_value
                || !["Buy", "Sell"].contains(&p.side.as_str())
                || !(0..=2).contains(&p.position_idx)
            {
                return Err(safe());
            }
            for v in [&p.size, &p.entry_price, &p.leverage, &p.unrealised_pnl] {
                decimal(v, false).map_err(|_| safe())?;
            }
        }
        snapshot.positions = Some(Section {
            items,
            fetched_at_ms,
            partial: true,
        });
    }
    if has("orders.read") {
        let mut items = host.orders_locked(symbol_value).await.map_err(|_| safe())?;
        let fetched_at_ms = host.now_ms().to_string();
        items.truncate(100);
        for o in &items {
            validate_order(o, symbol_value).map_err(|_| safe())?;
        }
        snapshot.orders = Some(Section {
            items,
            fetched_at_ms,
            partial: true,
        });
    }
    if has("market.read") {
        let ticker = host.market_locked(symbol_value).await.map_err(|_| safe())?;
        let fetched_at_ms = host.now_ms().to_string();
        if ticker.symbol != symbol_value {
            return Err(safe());
        }
        for v in [
            &ticker.last_price,
            &ticker.bid_price,
            &ticker.ask_price,
            &ticker.mark_price,
        ] {
            decimal(v, true).map_err(|_| safe())?;
        }
        snapshot.market = Some(MarketSection {
            ticker,
            fetched_at_ms,
        });
    }
    snapshot.captured_at_ms = host.now_ms().to_string();
    Ok(snapshot)
}
pub(crate) fn validate_order(o: &Order, expected_symbol: &str) -> AppResult<()> {
    if !bounded_id(&o.order_id)
        || o.symbol != expected_symbol
        || !["Buy", "Sell"].contains(&o.side.as_str())
        || !["Market", "Limit"].contains(&o.order_type.as_str())
        || o.order_link_id.as_ref().is_some_and(|id| !bounded_id(id))
    {
        return Err(error("plugin_workflow_data_unavailable"));
    }
    for v in [&o.price, &o.qty, &o.filled_qty, &o.avg_price] {
        decimal(v, false)?;
    }
    Ok(())
}
