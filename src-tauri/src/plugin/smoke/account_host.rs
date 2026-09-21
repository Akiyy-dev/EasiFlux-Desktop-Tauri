use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::error::AppError;
use crate::models::{
    account::Balance,
    trading::{
        CancelOrderRequest, Order, OrderStatus, PlaceOrderRequest, Position, SessionContext,
        SubmissionContext,
    },
};
use crate::plugin::workflow::{
    Account, AccountAuthority, HostFuture, Quote, WorkflowHost,
};
use crate::services::AccountLifecycleCoordinator;

const ACCOUNT_ID: &str = "plugin-smoke-account";
const SYMBOL: &str = "BTCUSDT";
const CAPTURED_ORDER_ID: &str = "plugin-smoke-open-order";

pub(crate) struct SmokeWorkflowHost {
    lifecycle: AccountLifecycleCoordinator,
    clock_ms: AtomicU64,
    place_mutations: AtomicUsize,
    cancel_mutations: AtomicUsize,
}

impl SmokeWorkflowHost {
    pub(crate) fn new() -> Self {
        let lifecycle = AccountLifecycleCoordinator::new();
        lifecycle.advance_session_epoch();
        Self {
            lifecycle,
            clock_ms: AtomicU64::new(1_789_948_800_000),
            place_mutations: AtomicUsize::new(0),
            cancel_mutations: AtomicUsize::new(0),
        }
    }

    pub(crate) fn mutation_counts(&self) -> (usize, usize) {
        (
            self.place_mutations.load(Ordering::Acquire),
            self.cancel_mutations.load(Ordering::Acquire),
        )
    }

    fn session_is_current(&self, account_id: &str, session_epoch: u64) -> bool {
        account_id == ACCOUNT_ID && session_epoch == self.lifecycle.current_session_epoch()
    }
}

fn smoke_error() -> AppError {
    AppError::Plugin {
        code: "plugin_workflow_data_unavailable",
        message: "隔离插件冒烟测试的合成账户状态不匹配",
        diagnostic: None,
    }
}

fn captured_order(status: OrderStatus) -> Order {
    Order {
        order_id: CAPTURED_ORDER_ID.into(),
        symbol: SYMBOL.into(),
        side: "Buy".into(),
        order_type: "Limit".into(),
        price: "50000".into(),
        qty: "0.001".into(),
        status,
        order_link_id: Some("plugin-smoke-existing-link".into()),
        filled_qty: "0".into(),
        avg_price: "0".into(),
    }
}

impl WorkflowHost for SmokeWorkflowHost {
    fn lifecycle(&self) -> &AccountLifecycleCoordinator {
        &self.lifecycle
    }

    fn now_ms(&self) -> u64 {
        self.clock_ms.fetch_add(1, Ordering::AcqRel)
    }

    fn authority_locked(&self) -> HostFuture<'_, AccountAuthority> {
        Box::pin(async move {
            Ok(AccountAuthority {
                account: Account {
                    account_id: ACCOUNT_ID.into(),
                    session_epoch: self.lifecycle.current_session_epoch().to_string(),
                    environment: "Injected synthetic smoke host".into(),
                },
                // Opaque test binding only. It is neither an API key nor guest-visible data.
                private_session: "plugin-smoke-private-authority-v1".into(),
            })
        })
    }

    fn balances_locked(&self) -> HostFuture<'_, Vec<Balance>> {
        Box::pin(async {
            Ok(vec![Balance {
                asset: "USDT".into(),
                available: "12.5".into(),
                frozen: "0".into(),
                total: "12.5".into(),
            }])
        })
    }

    fn positions_locked<'a>(&'a self, symbol: &'a str) -> HostFuture<'a, Vec<Position>> {
        Box::pin(async move {
            if symbol != SYMBOL {
                return Err(smoke_error());
            }
            Ok(Vec::new())
        })
    }

    fn orders_locked<'a>(&'a self, symbol: &'a str) -> HostFuture<'a, Vec<Order>> {
        Box::pin(async move {
            if symbol != SYMBOL {
                return Err(smoke_error());
            }
            Ok(vec![captured_order(OrderStatus::New)])
        })
    }

    fn market_locked<'a>(&'a self, symbol: &'a str) -> HostFuture<'a, Quote> {
        Box::pin(async move {
            if symbol != SYMBOL {
                return Err(smoke_error());
            }
            Ok(Quote {
                symbol: SYMBOL.into(),
                last_price: "50000".into(),
                bid_price: "49999".into(),
                ask_price: "50001".into(),
                mark_price: "50000".into(),
            })
        })
    }

    fn place_locked(
        &self,
        context: SubmissionContext,
        request: PlaceOrderRequest,
    ) -> HostFuture<'_, Order> {
        Box::pin(async move {
            if !self.session_is_current(&context.account_id, context.session_epoch)
                || request.symbol != SYMBOL
                || request.order_link_id.as_deref() != Some(context.submission_id.as_str())
            {
                return Err(smoke_error());
            }
            let sequence = self.place_mutations.fetch_add(1, Ordering::AcqRel) + 1;
            Ok(Order {
                order_id: format!("plugin-smoke-placed-{sequence}"),
                symbol: request.symbol,
                side: request.side,
                order_type: request.order_type,
                price: request.price.unwrap_or_else(|| "0".into()),
                qty: request.qty,
                status: OrderStatus::New,
                order_link_id: request.order_link_id,
                filled_qty: "0".into(),
                avg_price: "0".into(),
            })
        })
    }

    fn cancel_locked(
        &self,
        context: SessionContext,
        request: CancelOrderRequest,
    ) -> HostFuture<'_, Order> {
        Box::pin(async move {
            if !self.session_is_current(&context.account_id, context.session_epoch)
                || request.symbol != SYMBOL
                || request.order_id.as_deref() != Some(CAPTURED_ORDER_ID)
                || request.order_link_id.is_some()
            {
                return Err(smoke_error());
            }
            self.cancel_mutations.fetch_add(1, Ordering::AcqRel);
            Ok(captured_order(OrderStatus::Cancelled))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn synthetic_host_exposes_fixed_data_and_counts_only_explicit_mutations() {
        let host = SmokeWorkflowHost::new();
        let authority = host.authority_locked().await.unwrap();
        assert_eq!(authority.account.account_id, ACCOUNT_ID);
        assert_eq!(authority.account.session_epoch, "1");
        assert_eq!(host.mutation_counts(), (0, 0));
        assert_eq!(host.balances_locked().await.unwrap()[0].available, "12.5");
        assert_eq!(
            host.orders_locked(SYMBOL).await.unwrap()[0].order_id,
            CAPTURED_ORDER_ID
        );

        let submission_id = "plugin-smoke-submission".to_owned();
        let placed = host
            .place_locked(
                SubmissionContext {
                    submission_id: submission_id.clone(),
                    account_id: ACCOUNT_ID.into(),
                    session_epoch: 1,
                },
                PlaceOrderRequest {
                    symbol: SYMBOL.into(),
                    side: "Buy".into(),
                    order_type: "Limit".into(),
                    qty: "0.002".into(),
                    position_idx: 1,
                    price: Some("49000".into()),
                    time_in_force: Some("GTC".into()),
                    order_link_id: Some(submission_id),
                    reduce_only: Some(false),
                },
            )
            .await
            .unwrap();
        assert_eq!(placed.order_id, "plugin-smoke-placed-1");
        assert_eq!(host.mutation_counts(), (1, 0));

        host.cancel_locked(
            SessionContext {
                account_id: ACCOUNT_ID.into(),
                session_epoch: 1,
            },
            CancelOrderRequest {
                symbol: SYMBOL.into(),
                order_id: Some(CAPTURED_ORDER_ID.into()),
                order_link_id: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(host.mutation_counts(), (1, 1));
    }
}
