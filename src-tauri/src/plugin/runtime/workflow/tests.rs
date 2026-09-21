use super::*;
use crate::models::{account::Balance, trading::Position};
use crate::plugin::{
    discovery::{LocalDiscoveryOutcome, LocalPluginDiscovery},
    manifest::PluginManifest,
    record::PluginRecord,
};
use crate::services::AccountLifecycleCoordinator;
use crate::storage::plugin_state::{PluginStateFileV2, PluginStateLoad, PluginStatePersistence};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::json;

struct Memory;
impl PluginStatePersistence for Memory {
    fn load(&self) -> AppResult<PluginStateLoad> {
        Ok(PluginStateLoad {
            state: PluginStateFileV2::empty(),
            requires_rewrite: false,
        })
    }
    fn save(&self, _: &PluginStateFileV2) -> crate::storage::safe_plugin_document::PersistResult {
        Ok(crate::storage::safe_plugin_document::PersistOutcome::CommittedProcessCrashSafe)
    }
}
struct Discovery(PluginManifest);
impl LocalPluginDiscovery for Discovery {
    fn discover(&self) -> LocalDiscoveryOutcome {
        LocalDiscoveryOutcome::available(vec![
            PluginRecord::local_declarative(self.0.clone()).unwrap()
        ])
    }
}
fn order() -> Order {
    Order {
        order_id: "exchange-1".into(),
        symbol: "BTCUSDT".into(),
        side: "Buy".into(),
        order_type: "Limit".into(),
        price: "50000".into(),
        qty: "0.01".into(),
        status: OrderStatus::New,
        order_link_id: None,
        filled_qty: "0".into(),
        avg_price: "0".into(),
    }
}
struct Fake {
    lifecycle: AccountLifecycleCoordinator,
    identity: std::sync::Mutex<String>,
    account_id: std::sync::Mutex<String>,
    reads: std::sync::Mutex<Vec<&'static str>>,
    placed: std::sync::Mutex<Vec<(SubmissionContext, PlaceOrderRequest)>>,
    cancelled: std::sync::Mutex<Vec<(SessionContext, CancelOrderRequest)>>,
    cancel_payload: std::sync::Mutex<Option<serde_json::Value>>,
    unknown: AtomicBool,
    rejected: AtomicBool,
    hold: AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl Fake {
    fn new() -> Self {
        Self {
            lifecycle: AccountLifecycleCoordinator::new(),
            identity: std::sync::Mutex::new("private-session-1".into()),
            account_id: std::sync::Mutex::new("alpha".into()),
            reads: Default::default(),
            placed: Default::default(),
            cancelled: Default::default(),
            cancel_payload: Default::default(),
            unknown: AtomicBool::new(false),
            rejected: AtomicBool::new(false),
            hold: AtomicBool::new(false),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        }
    }
}
impl WorkflowHost for Fake {
    fn lifecycle(&self) -> &AccountLifecycleCoordinator {
        &self.lifecycle
    }
    fn now_ms(&self) -> u64 {
        1000
    }
    fn authority_locked(&self) -> HostFuture<'_, AccountAuthority> {
        Box::pin(async {
            Ok(AccountAuthority {
                account: Account {
                    account_id: self.account_id.lock().unwrap().clone(),
                    session_epoch: self.lifecycle.current_session_epoch().to_string(),
                    environment: "Test exchange".into(),
                },
                private_session: self.identity.lock().unwrap().clone(),
            })
        })
    }
    fn balances_locked(&self) -> HostFuture<'_, Vec<Balance>> {
        Box::pin(async {
            self.reads.lock().unwrap().push("balances");
            if self.hold.load(Ordering::Acquire) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            Ok(vec![Balance {
                asset: "USDT".into(),
                available: "9007199254740993.1".into(),
                frozen: "0".into(),
                total: "9007199254740993.1".into(),
            }])
        })
    }
    fn positions_locked<'a>(&'a self, _: &'a str) -> HostFuture<'a, Vec<Position>> {
        Box::pin(async {
            self.reads.lock().unwrap().push("positions");
            Ok(vec![])
        })
    }
    fn orders_locked<'a>(&'a self, _: &'a str) -> HostFuture<'a, Vec<Order>> {
        Box::pin(async {
            self.reads.lock().unwrap().push("orders");
            Ok(vec![order()])
        })
    }
    fn market_locked<'a>(&'a self, _: &'a str) -> HostFuture<'a, Quote> {
        Box::pin(async {
            self.reads.lock().unwrap().push("market");
            Ok(Quote {
                symbol: "BTCUSDT".into(),
                last_price: "50000".into(),
                bid_price: "49999".into(),
                ask_price: "50001".into(),
                mark_price: "50000".into(),
            })
        })
    }
    fn place_locked(&self, c: SubmissionContext, r: PlaceOrderRequest) -> HostFuture<'_, Order> {
        Box::pin(async move {
            self.placed.lock().unwrap().push((c, r));
            if self.hold.load(Ordering::Acquire) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            if self.rejected.load(Ordering::Acquire) {
                return Err(AppError::Notified {
                    code: "RISK_ORDER_BLOCKED",
                    message: "blocked",
                    notification_id: "host-only-id".into(),
                    cause: None,
                });
            }
            if self.unknown.load(Ordering::Acquire) {
                Err(AppError::Internal("secret exchange diagnostics".into()))
            } else {
                Ok(order())
            }
        })
    }
    fn cancel_locked(&self, c: SessionContext, r: CancelOrderRequest) -> HostFuture<'_, Order> {
        Box::pin(async move {
            self.cancelled.lock().unwrap().push((c, r.clone()));
            if let Some(payload) = self.cancel_payload.lock().unwrap().as_ref() {
                return crate::api::PrivateApi::parse_cancel_acknowledgement(payload, &r);
            }
            let mut o = order();
            o.status = OrderStatus::Cancelled;
            o.side.clear();
            o.qty = "0".into();
            Ok(o)
        })
    }
}
async fn fixture(output: &str) -> (Arc<PluginRuntime>, Arc<Fake>, AuthorityRequest) {
    fixture_capabilities(output, CAPABILITIES).await
}
async fn fixture_capabilities(
    output: &str,
    capabilities: &[&str],
) -> (Arc<PluginRuntime>, Arc<Fake>, AuthorityRequest) {
    let bytes = output
        .bytes()
        .map(|b| format!("\\{b:02x}"))
        .collect::<String>();
    // Alloc uses two fixed, disjoint regions selected by the documented input bound.
    let module=wat::parse_str(format!(r#"(module (memory (export "memory") 16 16)
 (func (export "alloc") (param i32) (result i32) local.get 0 i32.const 4096 i32.gt_u if (result i32) i32.const 32768 else local.get 0 i32.const 4096 i32.add end)
 (data (i32.const 0) "{bytes}") (func (export "run") (param i32 i32 i32 i32) (result i64) i64.const {}))"#,output.len())).unwrap();
    let manifest:PluginManifest=serde_json::from_value(json!({"schemaVersion":5,"id":"com.example.workflow","publisherId":"com.example","publisher":"Example","name":"Workflow","description":"Account workflow","version":"1.0.0","requestedCapabilities":capabilities,"contributions":[{"kind":"command","contributionId":"account.run","title":"Run","actionId":"sandbox.accountWorkflow","params":{"runtime":"wasm-v1","abi":"account-json-v1","moduleBase64":STANDARD.encode(module),"defaultInput":"{}"}}]})).unwrap();
    let mut runtime = PluginRuntime::initialize(
        PluginRegistry::initialize(
            vec![],
            Box::new(Memory),
            crate::plugin::ownership::empty_test_persistence(),
        ),
        Arc::new(Discovery(manifest)),
    );
    runtime.compute_slot = Arc::new(crate::plugin::compute::slot::ComputeSlot::default());
    let runtime = Arc::new(runtime);
    let catalog = runtime.get_catalog().await.unwrap();
    runtime
        .set_enabled("com.example.workflow", true, &catalog.catalog_generation)
        .await
        .unwrap();
    let catalog = runtime.get_catalog().await.unwrap();
    (
        runtime,
        Arc::new(Fake::new()),
        AuthorityRequest {
            plugin_id: "com.example.workflow".into(),
            contribution_id: "account.run".into(),
            expected_catalog_generation: catalog.catalog_generation,
            expected_revision: catalog.revision,
        },
    )
}
const PLACE: &str = r#"{"kind":"placeOrder","order":{"symbol":"BTCUSDT","side":"Buy","orderType":"Limit","qty":"0.0100","price":"50000.00","timeInForce":"GTC","positionIdx":1,"reduceOnly":false}}"#;
fn grants(a: &Access, caps: &[&str]) -> GrantRequest {
    GrantRequest {
        plugin_id: a.plugin_id.clone(),
        contribution_id: a.contribution_id.clone(),
        expected_catalog_generation: a.catalog_generation.clone(),
        expected_revision: a.revision.clone(),
        expected_account_id: a.account.account_id.clone(),
        expected_session_epoch: a.account.session_epoch.clone(),
        expected_grant_revision: a.grant_revision.clone(),
        capabilities: caps.iter().map(|v| v.to_string()).collect(),
    }
}
fn run(a: &Access) -> RunRequest {
    RunRequest {
        plugin_id: a.plugin_id.clone(),
        contribution_id: a.contribution_id.clone(),
        expected_catalog_generation: a.catalog_generation.clone(),
        expected_revision: a.revision.clone(),
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_account_id: a.account.account_id.clone(),
        expected_session_epoch: a.account.session_epoch.clone(),
        expected_grant_revision: a.grant_revision.clone(),
        symbol: "BTCUSDT".into(),
        input_json: "{}".into(),
    }
}
async fn authorize(
    r: &Arc<PluginRuntime>,
    h: &Arc<Fake>,
    a: AuthorityRequest,
    caps: &[&str],
) -> Access {
    let a = r.get_workflow_access(h.clone(), a).await.unwrap();
    r.set_workflow_grants(h.clone(), grants(&a, caps))
        .await
        .unwrap()
}

#[tokio::test]
async fn no_grants_reads_nothing_and_subset_fetches_only_requested_sections() {
    let (r, h, a) = fixture(r#"{"kind":"display","text":"ok"}"#).await;
    let access = r.get_workflow_access(h.clone(), a.clone()).await.unwrap();
    assert!(access.granted_capabilities.is_empty());
    assert!(r.run_workflow(h.clone(), run(&access)).await.is_err());
    assert!(h.reads.lock().unwrap().is_empty());
    let access = authorize(&r, &h, a, &["account.read", "balances.read"]).await;
    let result = r.run_workflow(h.clone(), run(&access)).await.unwrap();
    assert_eq!(*h.reads.lock().unwrap(), vec!["balances"]);
    assert!(result.snapshot.positions.is_none());
    assert!(result.confirmation.is_none());
    assert_eq!(
        result.snapshot.balances.unwrap().items[0].total,
        "9007199254740993.1"
    );
}
#[tokio::test]
async fn place_is_exact_canonical_and_only_one_confirmation_can_dispatch() {
    let (r, h, a) = fixture(PLACE).await;
    let access = authorize(&r, &h, a, &["account.read", "trade.place"]).await;
    let result = r.run_workflow(h.clone(), run(&access)).await.unwrap();
    assert!(h.placed.lock().unwrap().is_empty());
    let confirmation = result.confirmation.unwrap();
    let receipt = r
        .confirm_workflow(h.clone(), confirmation.token.clone())
        .await
        .unwrap();
    assert_eq!(receipt.status, TradeStatus::Accepted);
    assert!(r
        .confirm_workflow(h.clone(), confirmation.token)
        .await
        .is_err());
    let placed = h.placed.lock().unwrap();
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].0.account_id, "alpha");
    assert_eq!(placed[0].1.qty, "0.01");
    assert_eq!(placed[0].1.price.as_deref(), Some("50000"));
    assert_eq!(placed[0].1.order_link_id, confirmation.submission_id);
    assert_eq!(placed[0].1.time_in_force.as_deref(), Some("GTC"));
    assert_eq!(placed[0].1.reduce_only, Some(false));
    assert_eq!(placed[0].1.position_idx, 1);
}
#[tokio::test]
async fn revoke_secret_replacement_epoch_and_content_reload_invalidate_tokens_and_grants() {
    for cause in ["revoke", "secret", "account", "epoch", "reload", "expire"] {
        let (r, h, a) = fixture(PLACE).await;
        let access = authorize(&r, &h, a.clone(), &["account.read", "trade.place"]).await;
        let token = r
            .run_workflow(h.clone(), run(&access))
            .await
            .unwrap()
            .confirmation
            .unwrap()
            .token;
        match cause {
            "revoke" => {
                r.set_workflow_grants(h.clone(), grants(&access, &[]))
                    .await
                    .unwrap();
            }
            "secret" => {
                *h.identity.lock().unwrap() = "replacement-same-key-new-secret".into();
            }
            "account" => {
                *h.account_id.lock().unwrap() = "beta".into();
            }
            "epoch" => {
                h.lifecycle.advance_session_epoch();
            }
            "reload" => {
                r.reload_catalog().await.unwrap();
            }
            _ => {
                r.workflow
                    .lock()
                    .unwrap()
                    .pending
                    .get_mut(&token)
                    .unwrap()
                    .deadline = Instant::now() - Duration::from_secs(1);
            }
        }
        assert!(
            r.confirm_workflow(h.clone(), token).await.is_err(),
            "{cause}"
        );
        assert!(h.placed.lock().unwrap().is_empty());
        if cause == "secret" {
            let fresh = r.get_workflow_access(h.clone(), a).await.unwrap();
            assert_ne!(fresh.grant_revision, access.grant_revision);
            assert!(fresh.granted_capabilities.is_empty());
            assert!(r
                .set_workflow_grants(h.clone(), grants(&access, &["account.read", "trade.place"]))
                .await
                .is_err());
        }
    }
}
#[tokio::test]
async fn cancel_requires_captured_open_order_and_reports_acceptance_without_terminal_status() {
    for id in ["missing", "exchange-1"] {
        let (r, h, a) = fixture(&format!(
            r#"{{"kind":"cancelOrder","order":{{"symbol":"BTCUSDT","orderId":"{id}"}}}}"#
        ))
        .await;
        let access = authorize(&r, &h, a, &["account.read", "orders.read", "trade.cancel"]).await;
        let result = r.run_workflow(h.clone(), run(&access)).await;
        if id == "missing" {
            assert!(result.is_err());
            continue;
        }
        assert!(h.cancelled.lock().unwrap().is_empty());
        let token = result.unwrap().confirmation.unwrap().token;
        let receipt = r.confirm_workflow(h.clone(), token).await.unwrap();
        assert_eq!(receipt.status, TradeStatus::Accepted);
        let order = receipt.order.unwrap();
        assert_eq!(order.status, OrderStatus::Unknown);
        assert_eq!(order.side, "Buy");
        assert_eq!(order.qty, "0.01");
        let calls = h.cancelled.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1.order_id.as_deref(), Some("exchange-1"));
        assert!(calls[0].1.order_link_id.is_none());
    }
}
#[tokio::test]
async fn unknown_is_sanitized_and_never_retries_and_dropped_ipc_retains_admission() {
    let (r, h, a) = fixture(PLACE).await;
    let access = authorize(&r, &h, a, &["account.read", "trade.place"]).await;
    let token = r
        .run_workflow(h.clone(), run(&access))
        .await
        .unwrap()
        .confirmation
        .unwrap()
        .token;
    h.unknown.store(true, Ordering::Release);
    let receipt = r.confirm_workflow(h.clone(), token.clone()).await.unwrap();
    assert_eq!(receipt.status, TradeStatus::Unknown);
    assert!(!serde_json::to_string(&receipt)
        .unwrap()
        .contains("secret exchange"));
    assert!(r.confirm_workflow(h.clone(), token).await.is_err());
    assert_eq!(h.placed.lock().unwrap().len(), 1);
    let token = r
        .run_workflow(h.clone(), run(&access))
        .await
        .unwrap()
        .confirmation
        .unwrap()
        .token;
    h.hold.store(true, Ordering::Release);
    let rr = r.clone();
    let hh = h.clone();
    let caller = tokio::spawn(async move { rr.confirm_workflow(hh, token).await });
    h.entered.notified().await;
    caller.abort();
    assert!(r.operation_gate.try_lock().is_err());
    h.release.notify_one();
    let _guard = r.operation_gate.lock().await;
    assert_eq!(h.placed.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn invalid_capabilities_and_invalid_order_never_reach_host_mutation() {
    let (r, h, a) = fixture(&PLACE.replace("0.0100", "0")).await;
    let access = r.get_workflow_access(h.clone(), a).await.unwrap();
    for caps in [
        vec!["account.read", "account.read"],
        vec!["account.read", "network"],
        vec!["trade.place"],
    ] {
        assert!(r
            .set_workflow_grants(h.clone(), grants(&access, &caps))
            .await
            .is_err());
    }
    let access = r
        .set_workflow_grants(h.clone(), grants(&access, &["account.read", "trade.place"]))
        .await
        .unwrap();
    assert!(r.run_workflow(h.clone(), run(&access)).await.is_err());
    assert!(h.placed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn queued_revoke_invalidates_snapshot_before_its_result_is_published() {
    let (r, h, a) = fixture(PLACE).await;
    let access = authorize(&r, &h, a, &["account.read", "balances.read", "trade.place"]).await;
    h.hold.store(true, Ordering::Release);
    let mut running = Box::pin(r.run_workflow(h.clone(), run(&access)));
    assert!(futures_util::poll!(&mut running).is_pending());
    h.entered.notified().await;
    let mut revoke = Box::pin(r.set_workflow_grants(h.clone(), grants(&access, &[])));
    assert!(futures_util::poll!(&mut revoke).is_pending());
    h.release.notify_one();
    assert!(running.await.is_err());
    revoke.await.unwrap();
    assert!(r.workflow.lock().unwrap().pending.is_empty());
    assert!(h.placed.lock().unwrap().is_empty());
}
#[tokio::test]
async fn queued_confirm_rechecks_revoke_and_expiry_after_final_account_guard() {
    for cause in ["revoke", "expire", "secret"] {
        let (r, h, a) = fixture(PLACE).await;
        let access = authorize(&r, &h, a, &["account.read", "trade.place"]).await;
        let token = r
            .run_workflow(h.clone(), run(&access))
            .await
            .unwrap()
            .confirmation
            .unwrap()
            .token;
        let account = h.lifecycle.read_guard().await;
        let mut confirming = Box::pin(r.clone().confirm_owned(h.clone(), token.clone()));
        assert!(futures_util::poll!(&mut confirming).is_pending());
        assert!(r.operation_gate.try_lock().is_err());
        let mut revoke = Box::pin(r.set_workflow_grants(h.clone(), grants(&access, &[])));
        match cause {
            "revoke" => assert!(futures_util::poll!(&mut revoke).is_pending()),
            "expire" => {
                r.workflow
                    .lock()
                    .unwrap()
                    .pending
                    .get_mut(&token)
                    .unwrap()
                    .deadline = Instant::now() - Duration::from_secs(1)
            }
            _ => *h.identity.lock().unwrap() = "changed-while-queued".into(),
        }
        drop(account);
        assert!(confirming.await.is_err());
        if cause == "revoke" {
            revoke.await.unwrap();
        }
        assert!(h.placed.lock().unwrap().is_empty());
    }
}
#[tokio::test]
async fn prepared_capacity_is_hard_bounded_and_expired_slots_are_collected() {
    let (r, h, a) = fixture(PLACE).await;
    let access = authorize(&r, &h, a, &["account.read", "trade.place"]).await;
    for _ in 0..32 {
        r.run_workflow(h.clone(), run(&access)).await.unwrap();
    }
    assert_eq!(r.workflow.lock().unwrap().pending.len(), 32);
    assert!(r.run_workflow(h.clone(), run(&access)).await.is_err());
    for p in r.workflow.lock().unwrap().pending.values_mut() {
        p.deadline = Instant::now() - Duration::from_secs(1);
    }
    r.run_workflow(h.clone(), run(&access)).await.unwrap();
    assert_eq!(r.workflow.lock().unwrap().pending.len(), 1);
    assert!(h.placed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn notified_risk_rejection_is_rejected_not_unknown_and_does_not_leak_notification_id() {
    let (r, h, a) = fixture(PLACE).await;
    let access = authorize(&r, &h, a, &["account.read", "trade.place"]).await;
    let token = r
        .run_workflow(h.clone(), run(&access))
        .await
        .unwrap()
        .confirmation
        .unwrap()
        .token;
    h.rejected.store(true, Ordering::Release);
    let receipt = r.confirm_workflow(h, token).await.unwrap();
    assert_eq!(receipt.status, TradeStatus::Rejected);
    assert_eq!(
        receipt.error_code.as_deref(),
        Some("plugin_workflow_rejected")
    );
    assert!(!serde_json::to_string(&receipt)
        .unwrap()
        .contains("host-only-id"));
}

#[tokio::test]
async fn dropped_revoke_waiter_still_finishes_revocation_before_later_admission() {
    let (r, h, a) = fixture(PLACE).await;
    let access = authorize(&r, &h, a.clone(), &["account.read", "trade.place"]).await;
    let gate = r.operation_gate.lock().await;
    let mut revoke = Box::pin(r.set_workflow_grants(h.clone(), grants(&access, &[])));
    assert!(futures_util::poll!(&mut revoke).is_pending());
    drop(revoke);
    drop(gate);
    let fresh = r.get_workflow_access(h, a).await.unwrap();
    assert!(fresh.granted_capabilities.is_empty());
}

#[tokio::test]
async fn known_but_unrequested_capability_is_not_grantable() {
    let (r, h, a) = fixture_capabilities(PLACE, &["account.read"]).await;
    let access = r.get_workflow_access(h.clone(), a).await.unwrap();
    assert!(r
        .set_workflow_grants(h.clone(), grants(&access, &["account.read", "trade.place"]))
        .await
        .is_err());
    assert!(h.reads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancel_acknowledgement_requires_actual_matching_exchange_identity_and_never_retries() {
    for (payload, status) in [
        (json!({"code":0,"data":{}}), TradeStatus::Unknown),
        (
            json!({"code":0,"data":{"order_id":"other-exchange-order"}}),
            TradeStatus::Unknown,
        ),
        (
            json!({"code":0,"data":{"order_id":"exchange-1","order_link_id":"optional","futureHarmlessField":true}}),
            TradeStatus::Accepted,
        ),
    ] {
        let (r, h, a) = fixture(
            r#"{"kind":"cancelOrder","order":{"symbol":"BTCUSDT","orderId":"exchange-1"}}"#,
        )
        .await;
        let access = authorize(&r, &h, a, &["account.read", "orders.read", "trade.cancel"]).await;
        let token = r
            .run_workflow(h.clone(), run(&access))
            .await
            .unwrap()
            .confirmation
            .unwrap()
            .token;
        *h.cancel_payload.lock().unwrap() = Some(payload);
        let receipt = r.confirm_workflow(h.clone(), token.clone()).await.unwrap();
        assert_eq!(receipt.status, status);
        if status == TradeStatus::Accepted {
            assert_eq!(receipt.order.unwrap().status, OrderStatus::Unknown);
        } else {
            assert!(receipt.order.is_none());
            assert_eq!(
                receipt.error_code.as_deref(),
                Some("plugin_workflow_unknown")
            );
        }
        assert!(r.confirm_workflow(h.clone(), token).await.is_err());
        assert_eq!(h.cancelled.lock().unwrap().len(), 1);
    }
}
