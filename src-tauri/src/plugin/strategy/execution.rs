use super::{store::*, supervisor::Live, *};
use crate::{
    error::{AppError, AppResult},
    models::trading::{
        CancelOrderRequest, Order, PlaceOrderRequest, SessionContext, SubmissionContext,
    },
    plugin::workflow::{self, snapshot_locked, Snapshot, WorkflowOutput},
};
use futures_util::FutureExt;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::time::Instant;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Context<'a> {
    schema_version: u8,
    run_id: &'a str,
    sequence: &'a str,
    event: &'a str,
    snapshot: &'a Snapshot,
    state: &'a serde_json::Value,
    last_receipt: &'a Option<StrategyReceipt>,
}

impl StrategySupervisor {
    pub(super) async fn worker(self: Arc<Self>, id: String, live: Arc<Live>, resume: bool) {
        let result = std::panic::AssertUnwindSafe(self.worker_loop(&id, &live, resume))
            .catch_unwind()
            .await;
        if result.is_err() {
            self.mark(&id, StrategyStatus::Faulted, "plugin_strategy_unavailable");
        }
        self.finish(&id, &live);
    }
    async fn worker_loop(&self, id: &str, live: &Arc<Live>, resume: bool) {
        let mut event = if resume { "update" } else { "start" };
        loop {
            if !live.admitted() {
                return;
            }
            let record = match self.state(id) {
                Ok(r) => r,
                Err(_) => return,
            };
            if let Err(code) = self.clock_check(live, &record) {
                self.mark(
                    id,
                    if code == "plugin_strategy_expired" {
                        StrategyStatus::Completed
                    } else {
                        StrategyStatus::Paused
                    },
                    code,
                );
                return;
            }
            match self.pass(id, live, event).await {
                Ok(false) => return,
                Ok(true) => {}
                Err(AppError::Plugin {
                    code: "plugin_strategy_busy",
                    ..
                }) => {}
                Err(failure) => {
                    if !live.admitted() {
                        return;
                    }
                    let code = match failure {
                        AppError::Plugin { code, .. } if REASONS.contains(&code) => code,
                        _ => "plugin_strategy_unavailable",
                    };
                    let status = if matches!(
                        code,
                        "plugin_strategy_stale" | "plugin_strategy_data_unavailable"
                    ) {
                        StrategyStatus::Paused
                    } else if matches!(
                        code,
                        "plugin_strategy_expired" | "plugin_strategy_limit_reached"
                    ) {
                        StrategyStatus::Completed
                    } else {
                        StrategyStatus::Faulted
                    };
                    self.mark(id, status, code);
                    return;
                }
            }
            let completed = Instant::now();
            let wall = self.host.now_ms();
            let interval = Duration::from_millis(record.view.policy.interval_ms);
            let deadline = completed + interval;
            event = "timer";
            loop {
                if !live.admitted() {
                    return;
                }
                tokio::select! {biased;
                    _=live.signal.notified()=>{if !live.admitted(){return;}}
                    _=tokio::time::sleep_until(deadline)=>break,
                    _=self.wake.notified()=>{event="update";}
                }
            }
            let max_gap = 30000.max(record.view.policy.interval_ms * 3);
            if completed.elapsed() > Duration::from_millis(max_gap)
                || self
                    .host
                    .now_ms()
                    .checked_sub(wall)
                    .is_none_or(|gap| gap > max_gap)
            {
                self.mark(id, StrategyStatus::Paused, "plugin_strategy_stale");
                return;
            }
        }
    }
    fn clock_check(&self, live: &Live, record: &RunRecord) -> Result<(), &'static str> {
        let now = self.host.now_ms();
        let last = live.last_wall.fetch_max(now, Ordering::AcqRel);
        if now < last {
            return Err("plugin_strategy_stale");
        }
        if Instant::now() >= live.deadline
            || now
                >= workflow::counter(&record.view.expires_at_ms)
                    .map_err(|_| "plugin_strategy_stale")?
        {
            return Err("plugin_strategy_expired");
        }
        Ok(())
    }
    async fn check_binding(&self, live: &Live) -> AppResult<()> {
        if !live.admitted() || self.runtime.strategy_epoch()? != live.epoch {
            return Err(error("plugin_strategy_stale"));
        }
        let current = self.runtime.capture_strategy(&live.request).await?;
        let authority = self
            .host
            .authority_locked()
            .await
            .map_err(|_| error("plugin_strategy_stale"))?;
        if current.identity != live.captured.identity
            || authority != live.authority
            || !live.admitted()
            || self.runtime.strategy_epoch()? != live.epoch
        {
            return Err(error("plugin_strategy_stale"));
        }
        Ok(())
    }
    async fn fresh(&self, live: &Live, record: &RunRecord) -> AppResult<Snapshot> {
        let begin = Instant::now();
        let wall = self.host.now_ms();
        let result = tokio::time::timeout(
            Duration::from_secs(30),
            snapshot_locked(
                self.host.as_ref(),
                &live.authority,
                &record.view.symbol,
                &record.view.capabilities,
            ),
        )
        .await
        .map_err(|_| error("plugin_strategy_data_unavailable"))?
        .map_err(|_| error("plugin_strategy_data_unavailable"))?;
        if begin.elapsed() > Duration::from_secs(30)
            || self
                .host
                .now_ms()
                .checked_sub(wall)
                .is_none_or(|elapsed| elapsed > 30000)
        {
            return Err(error("plugin_strategy_data_unavailable"));
        }
        Ok(result)
    }
    async fn pass(&self, id: &str, live: &Arc<Live>, event: &str) -> AppResult<bool> {
        let record = self.state(id)?;
        if record.pending.is_some() {
            return Err(error("plugin_strategy_recovery_required"));
        }
        let op = self.runtime.strategy_try_gate()?;
        let account = self.host.lifecycle().read_guard().await;
        self.check_binding(live).await?;
        let lease = self.runtime.strategy_compute(id)?;
        *live
            .cancel
            .lock()
            .map_err(|_| error("plugin_strategy_unavailable"))? = Some(lease.cancel.clone());
        if !live.admitted() {
            lease.cancel.store(true, Ordering::Release);
            return Err(error("plugin_strategy_stale"));
        }
        // The oldest input used by this decision must remain fresh all the way
        // through compute, gate waits, fsync and downstream quote/risk preparation.
        let observed = Instant::now();
        let observed_wall = self.host.now_ms();
        let snapshot = self.fresh(live, &record).await?;
        let sequence = workflow::counter(&record.view.sequence)?
            .checked_add(1)
            .ok_or_else(|| error("plugin_strategy_limit_reached"))?
            .to_string();
        let context = serde_json::to_vec(&Context {
            schema_version: 1,
            run_id: id,
            sequence: &sequence,
            event,
            snapshot: &snapshot,
            state: &record.state,
            last_receipt: &record.view.last_receipt,
        })
        .map_err(|_| error("plugin_strategy_invalid_output"))?;
        if context.len() > 65536 {
            return Err(error("plugin_strategy_data_unavailable"));
        }
        let bytes = live
            .captured
            .params
            .module_bytes()
            .map_err(|_| error("plugin_strategy_compute_failed"))?;
        let input = record.view.input_json.clone();
        drop(account);
        drop(op);
        let output = tokio::task::spawn_blocking(move || {
            let lease = lease;
            let bytes = crate::plugin::compute::sandbox::execute_json(
                &bytes,
                &context,
                input.as_bytes(),
                &lease.cancel,
            )
            .map_err(|_| error("plugin_strategy_compute_failed"))?;
            serde_json::from_slice::<StrategyOutput>(&bytes)
                .map_err(|_| error("plugin_strategy_invalid_output"))
        })
        .await
        .map_err(|_| error("plugin_strategy_compute_failed"))??;
        *live
            .cancel
            .lock()
            .map_err(|_| error("plugin_strategy_unavailable"))? = None;
        output.validate()?;
        if !live.admitted() {
            return Err(error("plugin_strategy_stale"));
        }
        let _op = tokio::select! {biased;_=live.signal.notified()=>return Err(error("plugin_strategy_stale")),guard=self.runtime.strategy_gate()=>guard};
        let _account = tokio::select! {biased;_=live.signal.notified()=>return Err(error("plugin_strategy_stale")),guard=self.host.lifecycle().mutation_guard()=>guard};
        self.check_binding(live).await?;
        self.check_dispatch(live, &record, observed, observed_wall)?;
        let fresh = if matches!(output.action, StrategyAction::CancelOrder { .. }) {
            Some(self.fresh(live, &record).await?)
        } else {
            None
        };
        self.check_binding(live).await?;
        self.check_dispatch(live, &record, observed, observed_wall)?;
        let action = validate_action(output.action, &record, fresh.as_ref().unwrap_or(&snapshot))?;
        let pending = {
            let mut inner = self.lock()?;
            Self::ready(&inner)?;
            if !live.admitted()
                || self.runtime.strategy_epoch()? != live.epoch
                || !inner.live.get(id).is_some_and(|v| Arc::ptr_eq(v, live))
            {
                return Err(error("plugin_strategy_stale"));
            }
            let mut next = inner.document.clone();
            let r = next
                .runs
                .iter_mut()
                .find(|r| r.view.run_id == id)
                .ok_or_else(|| error("plugin_strategy_stale"))?;
            if r.view.sequence != record.view.sequence || r.pending.is_some() {
                return Err(error("plugin_strategy_stale"));
            }
            r.view.sequence = sequence.clone();
            r.last_observed_at_ms = self.host.now_ms().to_string();
            match action {
                StrategyAction::None | StrategyAction::Stop => {
                    r.state = output.state;
                    r.view.last_message = output.message;
                    let keep = matches!(action, StrategyAction::None);
                    if !keep {
                        r.view.status = StrategyStatus::Completed;
                        r.view.reason = None;
                    }
                    self.save(&mut inner, next)?;
                    return Ok(keep);
                }
                _ => {
                    r.view.actions_submitted = r
                        .view
                        .actions_submitted
                        .checked_add(1)
                        .ok_or_else(|| error("plugin_strategy_limit_reached"))?;
                    let submission_id = match &action {
                        StrategyAction::PlaceOrder { order } => {
                            let total =
                                rust_decimal::Decimal::from_str_exact(&r.view.total_submitted_qty)
                                    .map_err(|_| error("plugin_strategy_limit_reached"))?
                                    .checked_add(quantity(&order.qty)?)
                                    .ok_or_else(|| error("plugin_strategy_limit_reached"))?;
                            r.view.total_submitted_qty = total.normalize().to_string();
                            Some(uuid::Uuid::new_v4().to_string())
                        }
                        StrategyAction::CancelOrder { order } => {
                            r.owned
                                .get_mut(&order.order_id)
                                .ok_or_else(|| error("plugin_strategy_invalid_output"))?
                                .cancel_requested = true;
                            None
                        }
                        _ => unreachable!(),
                    };
                    let pending = PendingAction {
                        sequence,
                        action,
                        submission_id,
                        state: output.state,
                        message: output.message,
                    };
                    r.pending = Some(pending.clone());
                    // Prepare durable intent before the final admission check. Once
                    // dispatched, HTTP belongs to this worker and is never aborted.
                    self.save(&mut inner, next)?;
                    pending
                }
            }
        };
        // Fsync can outlast a concurrently accepted control/catalog intent. This is
        // still definitely unsent, so close it as a local rejection, retaining caps.
        if let Err(failure) = self
            .check_binding(live)
            .await
            .and_then(|_| self.check_dispatch(live, &record, observed, observed_wall))
        {
            self.account_response(
                id,
                &pending,
                Err(AppError::OrderSubmissionRejected(
                    "local admission revoked".into(),
                )),
            )
            .await?;
            return Err(failure);
        }
        let context = SessionContext {
            account_id: live.authority.account.account_id.clone(),
            session_epoch: workflow::counter(&live.authority.account.session_epoch)?,
        };
        let denied = std::sync::Mutex::new(None);
        let admission = || match self.check_dispatch(live, &record, observed, observed_wall) {
            Ok(()) => Ok(()),
            Err(failure) => {
                *denied
                    .lock()
                    .map_err(|_| error("plugin_strategy_unavailable"))? = Some(failure);
                // Definitely no mutation API hand-off: submit_once may close its
                // own intent as rejected, while the strategy's caps stay spent.
                Err(AppError::OrderSubmissionRejected(
                    "strategy admission revoked".into(),
                ))
            }
        };
        let response = match &pending.action {
            StrategyAction::PlaceOrder { order } => {
                let id = pending
                    .submission_id
                    .clone()
                    .ok_or_else(|| error("plugin_strategy_unavailable"))?;
                self.host
                    .strategy_place_locked(
                        SubmissionContext {
                            submission_id: id.clone(),
                            account_id: context.account_id,
                            session_epoch: context.session_epoch,
                        },
                        placement(order, id),
                        &admission,
                    )
                    .await
            }
            StrategyAction::CancelOrder { order } => {
                self.host
                    .strategy_cancel_locked(
                        context,
                        CancelOrderRequest {
                            symbol: order.symbol.clone(),
                            order_id: Some(order.order_id.clone()),
                            order_link_id: None,
                        },
                        &admission,
                    )
                    .await
            }
            _ => unreachable!(),
        };
        self.account_response(id, &pending, response).await?;
        if let Some(failure) = denied
            .into_inner()
            .map_err(|_| error("plugin_strategy_unavailable"))?
        {
            return Err(failure);
        }
        Ok(live.admitted() && self.state(id)?.pending.is_none())
    }
    fn check_dispatch(
        &self,
        live: &Live,
        record: &RunRecord,
        observed: Instant,
        observed_wall: u64,
    ) -> AppResult<()> {
        self.clock_check(live, record).map_err(error)?;
        if observed.elapsed() > Duration::from_secs(30)
            || self
                .host
                .now_ms()
                .checked_sub(observed_wall)
                .is_none_or(|age| age > 30000)
        {
            return Err(error("plugin_strategy_data_unavailable"));
        }
        // These are synchronous native checks. The final call occurs directly
        // before the mutation API hand-off; later stop drains that admitted call.
        if self.runtime.strategy_epoch()? != live.epoch || !live.admitted() {
            return Err(error("plugin_strategy_stale"));
        }
        Ok(())
    }
    pub(super) async fn account_response(
        &self,
        id: &str,
        pending: &PendingAction,
        response: AppResult<Order>,
    ) -> AppResult<()> {
        let (status, accepted) = match response {
            Ok(order) if matches_response(pending, &order) => {
                (ReceiptStatus::Accepted, Some(order))
            }
            Err(ref failure) if known_rejection(failure) => (ReceiptStatus::Rejected, None),
            _ => (ReceiptStatus::Unknown, None),
        };
        let kind = if matches!(pending.action, StrategyAction::PlaceOrder { .. }) {
            ReceiptKind::PlaceOrder
        } else {
            ReceiptKind::CancelOrder
        };
        {
            let mut inner = self.lock()?;
            let mut next = inner.document.clone();
            let r = next
                .runs
                .iter_mut()
                .find(|r| r.view.run_id == id)
                .ok_or_else(|| error("plugin_strategy_stale"))?;
            if r.pending.as_ref() != Some(pending) {
                return Err(error("plugin_strategy_stale"));
            }
            let order_id = accepted
                .as_ref()
                .map(|o| o.order_id.clone())
                .or_else(|| match &pending.action {
                    StrategyAction::CancelOrder { order } => Some(order.order_id.clone()),
                    _ => None,
                });
            r.view.last_receipt = Some(StrategyReceipt {
                sequence: pending.sequence.clone(),
                kind,
                status,
                submission_id: pending.submission_id.clone(),
                order_id,
                error_code: match status {
                    ReceiptStatus::Accepted => None,
                    ReceiptStatus::Rejected => Some("plugin_strategy_rejected".into()),
                    ReceiptStatus::Unknown => Some("plugin_strategy_unknown".into()),
                },
            });
            r.state = pending.state.clone();
            r.view.last_message = pending.message.clone();
            if let (Some(order), Some(submission_id)) = (&accepted, &pending.submission_id) {
                if r.owned
                    .get(&order.order_id)
                    .is_some_and(|o| o.submission_id != *submission_id)
                {
                    return Err(error("plugin_strategy_recovery_required"));
                }
                r.owned.insert(
                    order.order_id.clone(),
                    OwnedOrder {
                        submission_id: submission_id.clone(),
                        cancel_requested: false,
                    },
                );
            }
            if status == ReceiptStatus::Unknown {
                r.view.status = StrategyStatus::RecoveryRequired;
                r.view.reason = Some("plugin_strategy_recovery_required".into());
            } else if status == ReceiptStatus::Rejected || kind == ReceiptKind::CancelOrder {
                r.pending = None;
            }
            self.save(&mut inner, next)?;
        }
        if status == ReceiptStatus::Unknown {
            return Ok(());
        }
        if let (StrategyAction::PlaceOrder { order: request }, Some(order), Some(submission)) =
            (&pending.action, accepted, pending.submission_id.as_ref())
        {
            let record = self.state(id)?;
            if self
                .host
                .strategy_acknowledge_locked(&record.scope, submission, &order, request)
                .await
                .is_err()
            {
                self.mark(
                    id,
                    StrategyStatus::RecoveryRequired,
                    "plugin_strategy_ack_failed",
                );
                return Ok(());
            }
            let mut inner = self.lock()?;
            let mut next = inner.document.clone();
            let r = next
                .runs
                .iter_mut()
                .find(|r| r.view.run_id == id)
                .ok_or_else(|| error("plugin_strategy_stale"))?;
            if r.pending.as_ref() != Some(pending) {
                return Err(error("plugin_strategy_stale"));
            }
            r.pending = None;
            self.save(&mut inner, next)?;
        }
        Ok(())
    }
}
pub(super) fn placement(order: &workflow::PlaceProposal, id: String) -> PlaceOrderRequest {
    PlaceOrderRequest {
        symbol: order.symbol.clone(),
        side: order.side.clone(),
        order_type: order.order_type.clone(),
        qty: order.qty.clone(),
        position_idx: order.position_idx,
        price: order.price.clone(),
        time_in_force: Some(order.time_in_force.clone()),
        order_link_id: Some(id),
        reduce_only: Some(order.reduce_only),
    }
}
fn validate_action(
    action: StrategyAction,
    r: &RunRecord,
    snapshot: &Snapshot,
) -> AppResult<StrategyAction> {
    if matches!(action, StrategyAction::None | StrategyAction::Stop) {
        return Ok(action);
    }
    if r.view.actions_submitted >= r.view.policy.max_actions {
        return Err(error("plugin_strategy_limit_reached"));
    }
    match action {
        StrategyAction::PlaceOrder { order } => {
            let WorkflowOutput::PlaceOrder { order } = (WorkflowOutput::PlaceOrder { order })
                .validate(&r.view.symbol, &r.view.capabilities, None)
                .map_err(|_| error("plugin_strategy_invalid_output"))?
            else {
                unreachable!()
            };
            let qty = quantity(&order.qty)?;
            let total = rust_decimal::Decimal::from_str_exact(&r.view.total_submitted_qty)
                .map_err(|_| error("plugin_strategy_limit_reached"))?
                .checked_add(qty)
                .ok_or_else(|| error("plugin_strategy_limit_reached"))?;
            if qty > quantity(&r.view.policy.max_order_qty)?
                || total > quantity(&r.view.policy.max_total_qty)?
                || (r.view.policy.reduce_only && !order.reduce_only)
            {
                return Err(error("plugin_strategy_limit_reached"));
            }
            Ok(StrategyAction::PlaceOrder { order })
        }
        StrategyAction::CancelOrder { order } => {
            if !r
                .owned
                .get(&order.order_id)
                .is_some_and(|o| !o.cancel_requested)
            {
                return Err(error("plugin_strategy_invalid_output"));
            }
            let WorkflowOutput::CancelOrder { order } = (WorkflowOutput::CancelOrder { order })
                .validate(
                    &r.view.symbol,
                    &r.view.capabilities,
                    snapshot.orders.as_ref(),
                )
                .map_err(|_| error("plugin_strategy_invalid_output"))?
            else {
                unreachable!()
            };
            Ok(StrategyAction::CancelOrder { order })
        }
        _ => unreachable!(),
    }
}
pub(super) fn matches_response(p: &PendingAction, o: &Order) -> bool {
    match &p.action {
        StrategyAction::PlaceOrder { order } => {
            workflow::validate_order(o, &order.symbol).is_ok()
                && o.order_link_id == p.submission_id
                && o.side == order.side
                && o.order_type == order.order_type
                && quantity(&o.qty).ok() == quantity(&order.qty).ok()
                && (order.order_type != "Limit"
                    || o.price.parse::<rust_decimal::Decimal>().ok()
                        == order.price.as_ref().and_then(|p| p.parse().ok()))
        }
        StrategyAction::CancelOrder { order } => {
            o.symbol == order.symbol
                && o.order_id == order.order_id
                && workflow::bounded_id(&o.order_id)
        }
        _ => false,
    }
}
pub(super) fn known_rejection(f: &AppError) -> bool {
    crate::services::trading::is_certain_submission_failure(f)
        || matches!(
            f,
            AppError::OrderSubmissionRejected(_)
                | AppError::OrderSubmissionBlocked { .. }
                | AppError::Risk(_)
                | AppError::Notified {
                    code: "RISK_ORDER_BLOCKED" | "ORDER_REJECTED",
                    ..
                }
        )
}
