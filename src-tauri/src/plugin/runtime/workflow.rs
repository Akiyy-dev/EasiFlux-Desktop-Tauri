use super::*;
use crate::models::trading::{
    CancelOrderRequest, Order, OrderStatus, PlaceOrderRequest, SessionContext, SubmissionContext,
};
use crate::plugin::{record::PluginIdentity, registry::workflow::CapturedWorkflow, workflow::*};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Clone, PartialEq, Eq)]
struct Binding {
    identity: PluginIdentity,
    epoch: u64,
    authority: AccountAuthority,
}
#[derive(Clone)]
struct Grant {
    binding: Binding,
    revision: u64,
    intent: u64,
    capabilities: Vec<String>,
}
struct Prepared {
    request: AuthorityRequest,
    binding: Binding,
    revision: u64,
    intent: u64,
    deadline: Instant,
    output: WorkflowOutput,
    target: Option<Order>,
    submission_id: Option<String>,
}
#[derive(Default)]
pub(super) struct WorkflowState {
    serial: u64,
    grants: BTreeMap<String, Grant>,
    pending: BTreeMap<String, Prepared>,
}
impl WorkflowState {
    fn next(&mut self) -> AppResult<u64> {
        self.serial = self
            .serial
            .checked_add(1)
            .ok_or_else(|| error("plugin_workflow_unavailable"))?;
        Ok(self.serial)
    }
    fn grant(&mut self, id: &str, binding: Binding) -> AppResult<Grant> {
        self.pending
            .retain(|_, p| p.deadline > Instant::now() && p.binding.epoch == binding.epoch);
        self.grants.retain(|_, g| {
            g.binding.epoch == binding.epoch && g.binding.authority == binding.authority
        });
        if let Some(g) = self.grants.get(id).filter(|g| g.binding == binding) {
            return Ok(g.clone());
        }
        self.pending.retain(|_, p| p.request.plugin_id != id);
        if self.grants.len() >= 256 {
            return Err(error("plugin_workflow_capacity"));
        }
        let revision = self.next()?;
        let grant = Grant {
            binding,
            revision,
            intent: revision,
            capabilities: vec![],
        };
        self.grants.insert(id.into(), grant.clone());
        Ok(grant)
    }
}
struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl PluginRuntime {
    fn workflow_state(&self) -> AppResult<std::sync::MutexGuard<'_, WorkflowState>> {
        self.workflow
            .lock()
            .map_err(|_| error("plugin_workflow_unavailable"))
    }
    fn workflow_epoch(&self) -> AppResult<u64> {
        let epoch = self.compute_epoch.load(Ordering::Acquire);
        if epoch == u64::MAX || !self.initial_discovery_attempted.load(Ordering::Acquire) {
            return Err(error("plugin_workflow_unavailable"));
        }
        Ok(epoch)
    }
    async fn binding_locked(
        &self,
        host: &dyn WorkflowHost,
        request: &AuthorityRequest,
    ) -> AppResult<(CapturedWorkflow, Binding)> {
        let captured = self.registry.read().await.capture_workflow(request)?;
        let epoch = self.workflow_epoch()?;
        let authority = host
            .authority_locked()
            .await
            .map_err(|_| error("plugin_workflow_account_unavailable"))?;
        counter(&authority.account.session_epoch)?;
        if !bounded_id(&authority.account.account_id)
            || authority.private_session.is_empty()
            || authority.account.environment.is_empty()
            || authority.account.environment.len() > 256
        {
            return Err(error("plugin_workflow_account_unavailable"));
        }
        Ok((
            captured.clone(),
            Binding {
                identity: captured.identity,
                epoch,
                authority,
            },
        ))
    }
    fn access(request: AuthorityRequest, captured: CapturedWorkflow, grant: Grant) -> Access {
        Access {
            schema_version: 1,
            plugin_id: request.plugin_id,
            contribution_id: request.contribution_id,
            catalog_generation: request.expected_catalog_generation,
            revision: request.expected_revision,
            account: grant.binding.authority.account,
            requested_capabilities: captured.requested,
            granted_capabilities: grant.capabilities,
            grant_revision: grant.revision.to_string(),
        }
    }
    pub(crate) async fn get_workflow_access(
        self: &Arc<Self>,
        host: Arc<dyn WorkflowHost>,
        request: AuthorityRequest,
    ) -> AppResult<Access> {
        request.validate()?;
        let _operation = self.operation_gate.clone().lock_owned().await;
        let _account = host.lifecycle().read_guard().await;
        let (captured, binding) = self.binding_locked(host.as_ref(), &request).await?;
        let grant = self.workflow_state()?.grant(&request.plugin_id, binding)?;
        Ok(Self::access(request, captured, grant))
    }
    pub(crate) async fn set_workflow_grants(
        self: &Arc<Self>,
        host: Arc<dyn WorkflowHost>,
        request: GrantRequest,
    ) -> AppResult<Access> {
        let authority = request.authority();
        authority.validate()?;
        validate_capabilities(&request.capabilities)
            .map_err(|_| error("plugin_workflow_invalid_request"))?;
        counter(&request.expected_session_epoch)?;
        counter(&request.expected_grant_revision)?;
        let reservation = self.reserve_operation_gate();
        // Invalidate queued computations/confirmations at intent acceptance, before waiting on admission.
        let intent = {
            let mut state = self.workflow_state()?;
            let next = state.next()?;
            if let Some(g) = state.grants.get_mut(&request.plugin_id) {
                g.intent = next;
            }
            state
                .pending
                .retain(|_, p| p.request.plugin_id != request.plugin_id);
            next
        };
        let runtime = self.clone();
        tokio::spawn(async move {
            let _operation = reservation.enter().await;
            runtime
                .set_workflow_grants_locked(host, request, authority, intent)
                .await
        })
        .await
        .map_err(|_| error("plugin_workflow_unavailable"))?
    }
    async fn set_workflow_grants_locked(
        &self,
        host: Arc<dyn WorkflowHost>,
        request: GrantRequest,
        authority: AuthorityRequest,
        intent: u64,
    ) -> AppResult<Access> {
        let _account = host.lifecycle().read_guard().await;
        let (captured, binding) = self.binding_locked(host.as_ref(), &authority).await?;
        let mut state = self.workflow_state()?;
        let mut grant = state.grant(&request.plugin_id, binding)?;
        check_expected(
            &grant,
            &request.expected_account_id,
            &request.expected_session_epoch,
            &request.expected_grant_revision,
        )?;
        if request
            .capabilities
            .iter()
            .any(|v| !captured.requested.contains(v))
        {
            return Err(error("plugin_workflow_denied"));
        }
        if grant.intent != intent {
            return Err(error("plugin_workflow_stale"));
        }
        grant.revision = state.next()?;
        grant.capabilities = request.capabilities;
        state.grants.insert(request.plugin_id, grant.clone());
        Ok(Self::access(authority, captured, grant))
    }
    pub(crate) async fn run_workflow(
        self: &Arc<Self>,
        host: Arc<dyn WorkflowHost>,
        request: RunRequest,
    ) -> AppResult<WorkflowResult> {
        let authority = request.authority();
        authority.validate()?;
        crate::plugin::compute::validate_request_id(&request.request_id)
            .map_err(|_| error("plugin_workflow_invalid_request"))?;
        validate_input(&request.input_json)
            .map_err(|_| error("plugin_workflow_invalid_request"))?;
        symbol(&request.symbol)?;
        let operation = self
            .operation_gate
            .clone()
            .try_lock_owned()
            .map_err(|_| error("plugin_workflow_unavailable"))?;
        let account = host.lifecycle().read_guard().await;
        let (captured, binding) = self.binding_locked(host.as_ref(), &authority).await?;
        let grant = self
            .workflow_state()?
            .grant(&request.plugin_id, binding.clone())?;
        check_expected(
            &grant,
            &request.expected_account_id,
            &request.expected_session_epoch,
            &request.expected_grant_revision,
        )?;
        if !grant.capabilities.iter().any(|v| v == "account.read") {
            return Err(error("plugin_workflow_denied"));
        }
        let lease = self
            .compute_slot
            .acquire(&request.request_id)
            .map_err(|_| error("plugin_workflow_busy"))?;
        let cancel = CancelOnDrop(lease.cancel.clone());
        let snapshot = snapshot_locked(
            host.as_ref(),
            &binding.authority,
            &request.symbol,
            &grant.capabilities,
        )
        .await?;
        let context =
            serde_json::to_vec(&snapshot).map_err(|_| error("plugin_workflow_invalid_output"))?;
        if context.len() > 65_536 {
            return Err(error("plugin_workflow_data_unavailable"));
        }
        drop(account);
        drop(operation);
        let input = request.input_json.clone();
        let bytes = captured
            .params
            .module_bytes()
            .map_err(|_| error("plugin_workflow_invalid_module"))?;
        let output = tokio::task::spawn_blocking(move || {
            let lease = lease;
            let bytes = crate::plugin::compute::sandbox::execute_json(
                &bytes,
                &context,
                input.as_bytes(),
                &lease.cancel,
            )
            .map_err(|_| error("plugin_workflow_compute_failed"))?;
            serde_json::from_slice::<WorkflowOutput>(&bytes)
                .map_err(|_| error("plugin_workflow_invalid_output"))
        })
        .await
        .map_err(|_| error("plugin_workflow_compute_failed"))??;
        let output = output.validate(
            &request.symbol,
            &grant.capabilities,
            snapshot.orders.as_ref(),
        )?;
        let _operation = self
            .operation_gate
            .clone()
            .try_lock_owned()
            .map_err(|_| error("plugin_workflow_stale"))?;
        let _account = host.lifecycle().read_guard().await;
        let (_, current) = self.binding_locked(host.as_ref(), &authority).await?;
        let mut state = self.workflow_state()?;
        let current_grant = state.grant(&request.plugin_id, current.clone())?;
        if cancel.0.load(Ordering::Acquire)
            || binding != current
            || grant.revision != current_grant.revision
            || grant.intent != current_grant.intent
            || self.workflow_epoch()? != binding.epoch
        {
            return Err(error("plugin_workflow_stale"));
        }
        let confirmation = if matches!(output, WorkflowOutput::Display { .. }) {
            None
        } else {
            if state.pending.len() >= 32 {
                return Err(error("plugin_workflow_capacity"));
            }
            let token = uuid::Uuid::new_v4().to_string();
            let submission_id = matches!(output, WorkflowOutput::PlaceOrder { .. })
                .then(|| uuid::Uuid::new_v4().to_string());
            let target = match &output {
                WorkflowOutput::CancelOrder { order } => snapshot
                    .orders
                    .as_ref()
                    .and_then(|s| s.items.iter().find(|o| o.order_id == order.order_id))
                    .cloned(),
                _ => None,
            };
            state.pending.insert(
                token.clone(),
                Prepared {
                    request: authority,
                    binding: binding.clone(),
                    revision: grant.revision,
                    intent: grant.intent,
                    deadline: Instant::now() + Duration::from_secs(60),
                    output: output.clone(),
                    target,
                    submission_id: submission_id.clone(),
                },
            );
            Some(Confirmation {
                token,
                expires_at_ms: host.now_ms().saturating_add(60_000).to_string(),
                submission_id,
            })
        };
        Ok(WorkflowResult {
            schema_version: 1,
            request_id: request.request_id,
            plugin_id: request.plugin_id,
            contribution_id: request.contribution_id,
            catalog_generation: request.expected_catalog_generation,
            revision: request.expected_revision,
            account: binding.authority.account,
            grant_revision: grant.revision.to_string(),
            snapshot,
            output,
            confirmation,
        })
    }
    pub(crate) async fn confirm_workflow(
        self: &Arc<Self>,
        host: Arc<dyn WorkflowHost>,
        token: String,
    ) -> AppResult<TradeReceipt> {
        if uuid::Uuid::parse_str(&token).is_err() {
            return Err(error("plugin_workflow_invalid_request"));
        }
        // Own the entire admission/side-effect future independently of the IPC caller.
        let runtime = self.clone();
        tokio::spawn(async move { runtime.confirm_owned(host, token).await })
            .await
            .map_err(|_| error("plugin_workflow_unknown"))?
    }
    async fn confirm_owned(
        self: Arc<Self>,
        host: Arc<dyn WorkflowHost>,
        token: String,
    ) -> AppResult<TradeReceipt> {
        let _operation = self.operation_gate.clone().lock_owned().await;
        let _account = host.lifecycle().mutation_guard().await;
        let prepared = self
            .workflow_state()?
            .pending
            .remove(&token)
            .ok_or_else(|| error("plugin_workflow_token_invalid"))?;
        let (_, current) = self
            .binding_locked(host.as_ref(), &prepared.request)
            .await?;
        let grant = self
            .workflow_state()?
            .grant(&prepared.request.plugin_id, current.clone())?;
        if prepared.deadline <= Instant::now()
            || current != prepared.binding
            || grant.revision != prepared.revision
            || grant.intent != prepared.intent
            || self.workflow_epoch()? != prepared.binding.epoch
        {
            return Err(error("plugin_workflow_stale"));
        }
        let account = current.authority.account.clone();
        let context = SessionContext {
            account_id: account.account_id.clone(),
            session_epoch: counter(&account.session_epoch)?,
        };
        let (action, response) = match &prepared.output {
            WorkflowOutput::PlaceOrder { order } => {
                // Recheck all order invariants independently of configurable risk policy.
                prepared
                    .output
                    .clone()
                    .validate(&order.symbol, &grant.capabilities, None)?;
                let submission_id = prepared
                    .submission_id
                    .clone()
                    .ok_or_else(|| error("plugin_workflow_stale"))?;
                let request = PlaceOrderRequest {
                    symbol: order.symbol.clone(),
                    side: order.side.clone(),
                    order_type: order.order_type.clone(),
                    qty: order.qty.clone(),
                    position_idx: order.position_idx,
                    price: order.price.clone(),
                    time_in_force: Some(order.time_in_force.clone()),
                    order_link_id: Some(submission_id.clone()),
                    reduce_only: Some(order.reduce_only),
                };
                (
                    "placeOrder",
                    host.place_locked(
                        SubmissionContext {
                            submission_id,
                            account_id: context.account_id,
                            session_epoch: context.session_epoch,
                        },
                        request,
                    )
                    .await,
                )
            }
            WorkflowOutput::CancelOrder { order } => {
                let target = prepared
                    .target
                    .as_ref()
                    .ok_or_else(|| error("plugin_workflow_stale"))?;
                let section = Section {
                    items: vec![target.clone()],
                    fetched_at_ms: "0".into(),
                    partial: true,
                };
                prepared.output.clone().validate(
                    &order.symbol,
                    &grant.capabilities,
                    Some(&section),
                )?;
                (
                    "cancelOrder",
                    host.cancel_locked(
                        context,
                        CancelOrderRequest {
                            symbol: order.symbol.clone(),
                            order_id: Some(order.order_id.clone()),
                            order_link_id: None,
                        },
                    )
                    .await,
                )
            }
            _ => return Err(error("plugin_workflow_token_invalid")),
        };
        let (status, order, code) = match response {
            Ok(mut order) if bounded_id(&order.order_id) => {
                let expected = match &prepared.output {
                    WorkflowOutput::PlaceOrder { order } => &order.symbol,
                    WorkflowOutput::CancelOrder { order } => &order.symbol,
                    _ => unreachable!(),
                };
                if let Some(target) = &prepared.target {
                    if order.order_id != target.order_id {
                        return Ok(receipt(
                            token,
                            account,
                            action,
                            TradeStatus::Unknown,
                            prepared.submission_id,
                            None,
                            Some("plugin_workflow_unknown"),
                        ));
                    }
                    // The upstream cancellation adapter can synthesize Cancelled. Preserve captured fields, but do not claim a terminal exchange status.
                    order = target.clone();
                    order.status = OrderStatus::Unknown;
                }
                if validate_order(&order, expected).is_err() {
                    (TradeStatus::Unknown, None, Some("plugin_workflow_unknown"))
                } else {
                    (TradeStatus::Accepted, Some(order), None)
                }
            }
            Err(ref failure) if known_rejection(failure) => (
                TradeStatus::Rejected,
                None,
                Some("plugin_workflow_rejected"),
            ),
            _ => (TradeStatus::Unknown, None, Some("plugin_workflow_unknown")),
        };
        Ok(receipt(
            token,
            account,
            action,
            status,
            prepared.submission_id,
            order,
            code,
        ))
    }
}
fn check_expected(g: &Grant, id: &str, epoch: &str, revision: &str) -> AppResult<()> {
    counter(epoch)?;
    counter(revision)?;
    if g.binding.authority.account.account_id != id
        || g.binding.authority.account.session_epoch != epoch
        || g.revision.to_string() != revision
    {
        return Err(error("plugin_workflow_stale"));
    }
    Ok(())
}
fn receipt(
    token: String,
    account: Account,
    action: &str,
    status: TradeStatus,
    submission_id: Option<String>,
    order: Option<Order>,
    code: Option<&str>,
) -> TradeReceipt {
    TradeReceipt {
        schema_version: 1,
        token,
        account,
        action: action.into(),
        status,
        submission_id,
        order,
        error_code: code.map(str::to_owned),
    }
}

fn known_rejection(failure: &AppError) -> bool {
    crate::services::trading::is_certain_submission_failure(failure)
        || matches!(
            failure,
            AppError::OrderSubmissionRejected(_)
                | AppError::OrderSubmissionBlocked { .. }
                | AppError::Risk(_)
                | AppError::Notified {
                    code: "RISK_ORDER_BLOCKED" | "ORDER_REJECTED",
                    ..
                }
        )
}

#[cfg(test)]
mod tests;
