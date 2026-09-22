use super::super::store::*;
use super::super::*;
use crate::{
    error::{AppError, AppResult},
    models::{account::Balance, trading::*},
    plugin::{
        discovery::{LocalDiscoveryOutcome, LocalPluginDiscovery},
        manifest::PluginManifest,
        record::PluginRecord,
        workflow::*,
        PluginRegistry, PluginRuntime,
    },
    services::AccountLifecycleCoordinator,
    storage::{
        plugin_state::{PluginStateFileV2, PluginStateLoad, PluginStatePersistence},
        OrderSubmissionStore,
    },
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

const SCOPE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PLACE: &str = r#"{"state":{"phase":1},"action":{"kind":"placeOrder","order":{"symbol":"BTCUSDT","side":"Buy","orderType":"Limit","qty":"0.001","price":"50000","timeInForce":"GTC","positionIdx":1,"reduceOnly":false}},"message":"placed"}"#;
const CANCEL: &str = r#"{"state":{"phase":2},"action":{"kind":"cancelOrder","order":{"symbol":"BTCUSDT","orderId":"owned-1"}},"message":"cancel requested"}"#;
const STOP: &str = r#"{"state":{"phase":3},"action":{"kind":"stop"},"message":"done"}"#;
const CAPS: &[&str] = &[
    "account.read",
    "strategy.run",
    "orders.read",
    "market.read",
    "trade.place",
    "trade.cancel",
];
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
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
struct Temp(PathBuf);
struct FaultStore {
    file: FileStrategyStore,
    writes: AtomicUsize,
    fail_from: AtomicUsize,
    after_write: Mutex<Option<Arc<dyn Fn(usize) + Send + Sync>>>,
}
impl StrategyStore for FaultStore {
    fn load(&self) -> AppResult<StrategyDocument> {
        self.file.load()
    }
    fn save(&self, d: &StrategyDocument) -> AppResult<()> {
        let index = self.writes.fetch_add(1, Ordering::SeqCst);
        if index >= self.fail_from.load(Ordering::SeqCst) {
            return Err(super::super::error("plugin_strategy_storage_unavailable"));
        }
        self.file.save(d)?;
        let hook = self.after_write.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook(index);
        }
        Ok(())
    }
}
impl Temp {
    fn new() -> Self {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/strategy-tests")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn order(id: Option<String>) -> Order {
    Order {
        order_id: "owned-1".into(),
        symbol: "BTCUSDT".into(),
        side: "Buy".into(),
        order_type: "Limit".into(),
        price: "50000".into(),
        qty: "0.001".into(),
        status: OrderStatus::New,
        order_link_id: id,
        filled_qty: "0".into(),
        avg_price: "0".into(),
    }
}
struct Fake {
    lifecycle: AccountLifecycleCoordinator,
    now: AtomicU64,
    identity: Mutex<String>,
    store: Arc<dyn StrategyStore>,
    submissions: OrderSubmissionStore,
    placed: AtomicUsize,
    cancelled: AtomicUsize,
    acked: AtomicUsize,
    reads: AtomicUsize,
    visible: AtomicBool,
    unknown: AtomicBool,
    rejected: AtomicBool,
    hold_http: AtomicBool,
    read_delay_ms: AtomicU64,
    unknown_cancel: AtomicBool,
    terminal_cancel: AtomicBool,
    after_authority: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    hold_preparation: AtomicBool,
    preparing: tokio::sync::Notify,
    prepare_release: tokio::sync::Notify,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl WorkflowHost for Fake {
    fn lifecycle(&self) -> &AccountLifecycleCoordinator {
        &self.lifecycle
    }
    fn now_ms(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
    fn authority_locked(&self) -> HostFuture<'_, AccountAuthority> {
        Box::pin(async {
            let hook = self.after_authority.lock().unwrap().clone();
            if let Some(hook) = hook {
                hook();
            }
            Ok(AccountAuthority {
                account: Account {
                    account_id: "alpha".into(),
                    session_epoch: "0".into(),
                    environment: "testnet".into(),
                },
                private_session: self.identity.lock().unwrap().clone(),
            })
        })
    }
    fn balances_locked(&self) -> HostFuture<'_, Vec<Balance>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn positions_locked<'a>(&'a self, _: &'a str) -> HostFuture<'a, Vec<Position>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn orders_locked<'a>(&'a self, _: &'a str) -> HostFuture<'a, Vec<Order>> {
        Box::pin(async {
            Ok(
                if self.placed.load(Ordering::SeqCst) > 0 && self.visible.load(Ordering::SeqCst) {
                    vec![order(None)]
                } else {
                    vec![]
                },
            )
        })
    }
    fn market_locked<'a>(&'a self, _: &'a str) -> HostFuture<'a, Quote> {
        Box::pin(async {
            let n = self.reads.fetch_add(1, Ordering::SeqCst);
            self.now
                .fetch_add(self.read_delay_ms.load(Ordering::SeqCst), Ordering::SeqCst);
            Ok(Quote {
                symbol: "BTCUSDT".into(),
                last_price: (50000 + n).to_string(),
                bid_price: "49999".into(),
                ask_price: "50001".into(),
                mark_price: "50000".into(),
            })
        })
    }
    fn place_locked(&self, _: SubmissionContext, _: PlaceOrderRequest) -> HostFuture<'_, Order> {
        panic!("strategies must not use the unguarded v5 placement seam")
    }
    fn strategy_place_locked<'a>(
        &'a self,
        c: SubmissionContext,
        r: PlaceOrderRequest,
        admission: &'a crate::services::trading::StrategyAdmission<'a>,
    ) -> HostFuture<'a, Order> {
        Box::pin(async move {
            if self.hold_preparation.load(Ordering::SeqCst) {
                self.preparing.notify_one();
                self.prepare_release.notified().await;
            }
            let doc = self.store.load()?;
            let run = &doc.runs[0];
            assert!(run.pending.is_some());
            assert_eq!(run.view.actions_submitted, 1);
            assert_eq!(run.view.total_submitted_qty, "0.001");
            assert_eq!(c.submission_id, r.order_link_id.clone().unwrap());
            crate::services::order_submission::submit_once(
                &self.submissions,
                SCOPE,
                r,
                self.now_ms(),
                |r| async move {
                    admission()?;
                    self.placed.fetch_add(1, Ordering::SeqCst);
                    self.entered.notify_one();
                    if self.hold_http.load(Ordering::SeqCst) {
                        self.release.notified().await;
                    }
                    if self.unknown.load(Ordering::SeqCst) {
                        return Err(AppError::Connection("private-key-must-not-leak".into()));
                    }
                    if self.rejected.load(Ordering::SeqCst) {
                        return Err(AppError::Risk("private-raw-reason".into()));
                    }
                    Ok(order(r.order_link_id))
                },
            )
            .await
        })
    }
    fn cancel_locked(&self, _: SessionContext, _: CancelOrderRequest) -> HostFuture<'_, Order> {
        panic!("strategies must not use the unguarded v5 cancellation seam")
    }
    fn strategy_cancel_locked<'a>(
        &'a self,
        _: SessionContext,
        r: CancelOrderRequest,
        admission: &'a crate::services::trading::StrategyAdmission<'a>,
    ) -> HostFuture<'a, Order> {
        Box::pin(async move {
            if self.hold_preparation.load(Ordering::SeqCst) {
                self.preparing.notify_one();
                self.prepare_release.notified().await;
            }
            let doc = self.store.load()?;
            assert_eq!(doc.runs[0].view.actions_submitted, 2);
            assert!(doc.runs[0].pending.is_some());
            assert_eq!(r.order_id.as_deref(), Some("owned-1"));
            admission()?;
            self.cancelled.fetch_add(1, Ordering::SeqCst);
            if self.unknown_cancel.load(Ordering::SeqCst) {
                return Err(AppError::Connection(
                    "synthetic unknown cancellation".into(),
                ));
            }
            Ok(order(None))
        })
    }
    fn strategy_scope_locked(&self) -> HostFuture<'_, String> {
        Box::pin(async { Ok(SCOPE.into()) })
    }
    fn strategy_acknowledge_locked<'a>(
        &'a self,
        scope: &'a str,
        id: &'a str,
        expected: &'a Order,
        _: &'a PlaceProposal,
    ) -> HostFuture<'a, ()> {
        Box::pin(async move {
            let doc = self.store.load()?;
            let run = &doc.runs[0];
            assert!(run.owned.contains_key("owned-1"));
            assert_eq!(
                run.view.last_receipt.as_ref().unwrap().status,
                ReceiptStatus::Accepted
            );
            assert!(run.pending.is_some());
            assert_eq!(expected.order_link_id.as_deref(), Some(id));
            self.submissions.acknowledge(scope, id)?;
            self.acked.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
    fn strategy_reconcile_locked<'a>(
        &'a self,
        scope: &'a str,
        symbol: &'a str,
        submission: Option<&'a str>,
        exchange: Option<&'a str>,
    ) -> HostFuture<'a, Option<Order>> {
        Box::pin(async move {
            assert_eq!(symbol, "BTCUSDT");
            if let Some(id) = submission {
                crate::services::order_submission::reconcile(
                    &self.submissions,
                    scope,
                    id,
                    |_| async { Ok(vec![order(Some(id.into()))]) },
                )
                .await
            } else {
                assert_eq!(exchange, Some("owned-1"));
                let mut o = order(None);
                if self.terminal_cancel.load(Ordering::SeqCst) {
                    o.status = OrderStatus::Cancelled;
                }
                Ok(Some(o))
            }
        })
    }
}
fn guest(place: &str, cancel: &str) -> Vec<u8> {
    let escape = |s: &str| s.bytes().map(|b| format!("\\{b:02x}")).collect::<String>();
    let stop_offset = place.len() + cancel.len();
    wat::parse_str(format!(r#"(module(memory(export "memory")16 16)
      (data(i32.const 0)"{}{}{}")
      (func(export "alloc")(param i32)(result i32)local.get 0 i32.const 4096 i32.gt_u if(result i32)i32.const 32768 else local.get 0 i32.const 4096 i32.add end)
      (func(export "run")(param $c i32)(param $n i32)(param i32 i32)(result i64)(local $i i32)
        (block $done(loop $scan
          local.get $i local.get $n i32.const 9 i32.sub i32.ge_u br_if $done
          local.get $c local.get $i i32.add i64.load align=1 i64.const {} i64.eq
          if
            local.get $c local.get $i i32.add i32.const 8 i32.add i32.load8_u i32.const 50 i32.eq
            if i64.const {} return end
            i64.const {} return
          end
          local.get $i i32.const 1 i32.add local.set $i br $scan))
        i64.const {}))"#,escape(place),escape(cancel),escape(STOP),u64::from_le_bytes(*b"\"phase\":"),((stop_offset as u64)<<32)|STOP.len()as u64,((place.len()as u64)<<32)|cancel.len()as u64,place.len())).unwrap()
}
struct Fixture {
    _temp: Temp,
    supervisor: Arc<StrategySupervisor>,
    host: Arc<Fake>,
    runtime: Arc<PluginRuntime>,
    authority: AuthorityRequest,
    store: Arc<FaultStore>,
}
async fn fixture(place: &str, cancel: &str) -> Fixture {
    let mut m = super::manifest();
    m["requestedCapabilities"] = json!(CAPS);
    m["contributions"][0]["params"]["moduleBase64"] = json!(STANDARD.encode(guest(place, cancel)));
    fixture_manifest(m).await
}
async fn fixture_manifest(m: serde_json::Value) -> Fixture {
    let plugin_id = m["id"].as_str().unwrap().to_string();
    let contribution_id = m["contributions"][0]["contributionId"]
        .as_str()
        .unwrap()
        .to_string();
    let temp = Temp::new();
    let store = Arc::new(FaultStore {
        file: FileStrategyStore::new(temp.0.join("journal")),
        writes: AtomicUsize::new(0),
        fail_from: AtomicUsize::new(usize::MAX),
        after_write: Mutex::new(None),
    });
    let mut runtime = PluginRuntime::initialize(
        PluginRegistry::initialize(
            vec![],
            Box::new(Memory),
            crate::plugin::ownership::empty_test_persistence(),
        ),
        Arc::new(Discovery(serde_json::from_value(m).unwrap())),
    );
    crate::plugin::runtime::strategy_test_utils::isolate_compute(&mut runtime);
    let runtime = Arc::new(runtime);
    let catalog = runtime.get_catalog().await.unwrap();
    runtime
        .set_enabled(&plugin_id, true, &catalog.catalog_generation)
        .await
        .unwrap();
    let catalog = runtime.get_catalog().await.unwrap();
    let authority = AuthorityRequest {
        plugin_id,
        contribution_id,
        expected_catalog_generation: catalog.catalog_generation,
        expected_revision: catalog.revision,
    };
    let host = Arc::new(Fake {
        lifecycle: AccountLifecycleCoordinator::new(),
        now: AtomicU64::new(1000),
        identity: Mutex::new("installation-one".into()),
        store: store.clone(),
        submissions: OrderSubmissionStore::with_dir(temp.0.join("submissions")),
        placed: AtomicUsize::new(0),
        cancelled: AtomicUsize::new(0),
        acked: AtomicUsize::new(0),
        reads: AtomicUsize::new(0),
        visible: AtomicBool::new(true),
        unknown: AtomicBool::new(false),
        rejected: AtomicBool::new(false),
        hold_http: AtomicBool::new(false),
        read_delay_ms: AtomicU64::new(0),
        unknown_cancel: AtomicBool::new(false),
        terminal_cancel: AtomicBool::new(true),
        after_authority: Mutex::new(None),
        hold_preparation: AtomicBool::new(false),
        preparing: tokio::sync::Notify::new(),
        prepare_release: tokio::sync::Notify::new(),
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let supervisor = StrategySupervisor::new(
        runtime.clone(),
        host.clone(),
        store.clone(),
        Arc::new(tokio::sync::Notify::new()),
    );
    Fixture {
        _temp: temp,
        supervisor,
        host,
        runtime,
        authority,
        store,
    }
}
fn request(access: StrategyAccess) -> StrategyStartRequest {
    StrategyStartRequest {
        authorization_token: access.authorization_token,
        request_id: uuid::Uuid::new_v4().to_string(),
        resume_run_id: None,
        symbol: "BTCUSDT".into(),
        input_json: "{}".into(),
        capabilities: CAPS.iter().map(|v| v.to_string()).collect(),
        policy: StrategyPolicy {
            interval_ms: 5000,
            max_order_qty: "0.001".into(),
            max_total_qty: "0.01".into(),
            max_actions: 20,
            max_run_seconds: 3600,
            reduce_only: false,
        },
        acknowledge_automatic_trading: true,
    }
}
async fn start(f: &Fixture) -> StrategyRunView {
    let access = f.supervisor.access(f.authority.clone()).await.unwrap();
    f.supervisor.start(request(access)).await.unwrap()
}
async fn settled(f: &Fixture, sequence: &str) -> StrategyRunView {
    for _ in 0..100_000 {
        let view = f.supervisor.list().unwrap().runs[0].clone();
        if view.sequence == sequence && f.store.load().unwrap().runs[0].pending.is_none() {
            return view;
        }
        if matches!(
            view.status,
            StrategyStatus::Faulted | StrategyStatus::RecoveryRequired
        ) {
            panic!("unexpected status {:?}: {:?}", view.status, view.reason);
        }
        tokio::task::yield_now().await;
    }
    panic!("strategy pass did not settle");
}
async fn tick(f: &Fixture) {
    f.host.now.fetch_add(5000, Ordering::SeqCst);
    tokio::time::advance(std::time::Duration::from_secs(5)).await;
}

// Break: no native scheduling, forgotten journal debit, confirmation dependency, early ack or non-owned cancel.
#[tokio::test(start_paused = true)]
async fn native_worker_places_then_cancels_once_with_durable_intent_and_receipt_before_ack() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    assert!(f.supervisor.list().unwrap().runs.is_empty());
    start(&f).await;
    settled(&f, "1").await;
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.acked.load(Ordering::SeqCst), 1);
    assert!(f.host.submissions.list_pending(SCOPE).unwrap().is_empty());
    tick(&f).await;
    settled(&f, "2").await;
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 1);
    tick(&f).await;
    let view = settled(&f, "3").await;
    assert_eq!(view.status, StrategyStatus::Completed);
    assert_eq!(view.actions_submitted, 2);
    assert_eq!(view.total_submitted_qty, "0.001");
    assert!(f.host.reads.load(Ordering::SeqCst) >= 3);
    tick(&f).await;
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn pause_revokes_worker_and_tickets_before_waiting_for_plugin_gate() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    let access = f.supervisor.access(f.authority.clone()).await.unwrap();
    f.supervisor.stop_all().await.unwrap();
    assert!(f.supervisor.start(request(access)).await.is_err());
    let run = start(&f).await;
    settled(&f, "1").await;
    f.supervisor
        .control(StrategyControlRequest {
            run_id: run.run_id.clone(),
            action: ControlAction::Pause,
        })
        .await
        .unwrap();
    tick(&f).await;
    assert_eq!(
        f.supervisor.list().unwrap().runs[0].status,
        StrategyStatus::Paused
    );
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 0);
}
#[tokio::test(start_paused = true)]
async fn stopped_http_is_not_aborted_and_dropped_ipc_does_not_release_the_owned_worker() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    f.host.hold_http.store(true, Ordering::SeqCst);
    let access = f.supervisor.access(f.authority.clone()).await.unwrap();
    let supervisor = f.supervisor.clone();
    let task = tokio::spawn(async move { supervisor.start(request(access)).await });
    for _ in 0..100_000 {
        if f.host.placed.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 1);
    task.abort();
    let list = f.supervisor.stop_all().await.unwrap();
    assert_eq!(list.runs[0].status, StrategyStatus::Stopping);
    assert!(f.store.load().unwrap().runs[0].pending.is_some());
    f.host.release.notify_one();
    let view = settled(&f, "1").await;
    assert_eq!(view.status, StrategyStatus::Stopped);
    assert_eq!(f.host.acked.load(Ordering::SeqCst), 1);
    tick(&f).await;
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 0);
}
#[tokio::test(start_paused = true)]
async fn credential_reinstall_with_same_epoch_invalidates_ticket_and_live_admission() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    let access = f.supervisor.access(f.authority.clone()).await.unwrap();
    *f.host.identity.lock().unwrap() = "installation-two".into();
    assert!(f.supervisor.start(request(access)).await.is_err());
    start(&f).await;
    settled(&f, "1").await;
    *f.host.identity.lock().unwrap() = "installation-three".into();
    tick(&f).await;
    for _ in 0..10_000 {
        if f.supervisor.list().unwrap().runs[0].status != StrategyStatus::Running {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_ne!(
        f.supervisor.list().unwrap().runs[0].status,
        StrategyStatus::Running
    );
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 0);
}
#[tokio::test(start_paused = true)]
async fn oversized_order_faults_without_dispatch_even_when_global_risk_is_absent() {
    let _serial = SERIAL.lock().await;
    let f = fixture(&PLACE.replace("0.001", "0.002"), CANCEL).await;
    start(&f).await;
    for _ in 0..100_000 {
        if f.supervisor.list().unwrap().runs[0].status != StrategyStatus::Running {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_ne!(
        f.supervisor.list().unwrap().runs[0].status,
        StrategyStatus::Running
    );
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 0);
    assert_eq!(f.supervisor.list().unwrap().runs[0].actions_submitted, 0);
}
#[tokio::test(start_paused = true)]
async fn partial_snapshot_absence_never_authorizes_cancellation() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    start(&f).await;
    settled(&f, "1").await;
    f.host.visible.store(false, Ordering::SeqCst);
    tick(&f).await;
    for _ in 0..100_000 {
        if f.supervisor.list().unwrap().runs[0].status != StrategyStatus::Running {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 0);
    assert!(f.store.load().unwrap().runs[0]
        .owned
        .contains_key("owned-1"));
}
async fn status(f: &Fixture, wanted: StrategyStatus) -> StrategyRunView {
    for _ in 0..100_000 {
        let v = f.supervisor.list().unwrap().runs[0].clone();
        if v.status == wanted {
            return v;
        }
        tokio::task::yield_now().await;
    }
    panic!(
        "wanted {:?}, got {:?}",
        wanted,
        f.supervisor.list().unwrap().runs[0].status
    );
}
#[tokio::test(start_paused = true)]
async fn unknown_placement_survives_restart_without_replay_and_reconciles_without_resuming() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    f.host.unknown.store(true, Ordering::SeqCst);
    let run = start(&f).await;
    let view = status(&f, StrategyStatus::RecoveryRequired).await;
    assert_eq!(
        view.last_receipt.unwrap().error_code.as_deref(),
        Some("plugin_strategy_unknown")
    );
    f.supervisor.drain().await;
    let restarted = StrategySupervisor::new(
        f.runtime.clone(),
        f.host.clone(),
        f.store.clone(),
        Arc::new(tokio::sync::Notify::new()),
    );
    assert_eq!(
        restarted.list().unwrap().runs[0].status,
        StrategyStatus::RecoveryRequired
    );
    tick(&f).await;
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 1);
    let access = restarted.access(f.authority.clone()).await.unwrap();
    assert!(restarted.start(request(access)).await.is_err());
    let view = restarted.reconcile(run.run_id).await.unwrap();
    assert_eq!(view.status, StrategyStatus::Paused);
    assert_eq!(view.actions_submitted, 1);
    assert_eq!(view.total_submitted_qty, "0.001");
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.acked.load(Ordering::SeqCst), 1);
}
#[tokio::test(start_paused = true)]
async fn resume_requires_identical_policy_and_retains_state_counters_and_ownership() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    let run = start(&f).await;
    settled(&f, "1").await;
    f.supervisor
        .control(StrategyControlRequest {
            run_id: run.run_id.clone(),
            action: ControlAction::Pause,
        })
        .await
        .unwrap();
    status(&f, StrategyStatus::Paused).await;
    let access = f.supervisor.access(f.authority.clone()).await.unwrap();
    let mut changed = request(access);
    changed.resume_run_id = Some(run.run_id.clone());
    changed.policy.max_actions = 21;
    assert!(f.supervisor.start(changed).await.is_err());
    let access = f.supervisor.access(f.authority.clone()).await.unwrap();
    let mut resume = request(access);
    resume.resume_run_id = Some(run.run_id.clone());
    let new_request = resume.request_id.clone();
    let view = f.supervisor.start(resume).await.unwrap();
    assert_eq!(view.request_id, new_request);
    assert_eq!(view.actions_submitted, 1);
    assert_eq!(view.run_id, run.run_id);
    settled(&f, "2").await;
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 1);
    f.supervisor.stop_all().await.unwrap();
    f.supervisor.drain().await;
}
#[tokio::test(start_paused = true)]
async fn clock_rollback_and_plugin_reload_revoke_live_admission() {
    let _serial = SERIAL.lock().await;
    for reload in [false, true] {
        let f = fixture(PLACE, CANCEL).await;
        start(&f).await;
        settled(&f, "1").await;
        if reload {
            f.runtime.reload_catalog().await.unwrap();
        } else {
            f.host.now.store(0, Ordering::SeqCst);
        }
        tokio::time::advance(std::time::Duration::from_secs(5)).await;
        status(&f, StrategyStatus::Paused).await;
        assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 0);
    }
}
#[tokio::test(start_paused = true)]
async fn ticket_expiry_and_issuance_capacity_never_create_a_run() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    let ticket = f.supervisor.access(f.authority.clone()).await.unwrap();
    tokio::time::advance(std::time::Duration::from_secs(60)).await;
    assert!(f.supervisor.start(request(ticket)).await.is_err());
    for _ in 0..32 {
        f.supervisor.access(f.authority.clone()).await.unwrap();
    }
    assert!(f.supervisor.access(f.authority.clone()).await.is_err());
    assert!(f.supervisor.list().unwrap().runs.is_empty());
}
#[tokio::test(start_paused = true)]
async fn packaged_threshold_guest_trades_only_after_price_crossing_then_cancels_owned_order() {
    let _serial = SERIAL.lock().await;
    let manifest = serde_json::from_str(include_str!(
        "../../../../../examples/plugins/threshold-strategy/manifest.json"
    ))
    .unwrap();
    let f = fixture_manifest(manifest).await;
    let mut r = request(f.supervisor.access(f.authority.clone()).await.unwrap());
    r.input_json=r#"{"threshold":"50001","order":{"symbol":"BTCUSDT","side":"Buy","orderType":"Limit","qty":"0.001","price":"50000","timeInForce":"GTC","positionIdx":1,"reduceOnly":false}}"#.into();
    f.supervisor.start(r).await.unwrap();
    settled(&f, "1").await;
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 0);
    tick(&f).await;
    settled(&f, "2").await;
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 1);
    tick(&f).await;
    settled(&f, "3").await;
    // The example first persists ownership from the accepted native receipt,
    // then evaluates current order visibility on its next callback.
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 0);
    tick(&f).await;
    settled(&f, "4").await;
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 1);
    tick(&f).await;
    status(&f, StrategyStatus::Completed).await;
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.acked.load(Ordering::SeqCst), 1);
}
#[tokio::test(start_paused = true)]
async fn storage_corruption_revokes_admission_and_cannot_be_bypassed_by_a_new_start() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    start(&f).await;
    settled(&f, "1").await;
    std::fs::write(f._temp.0.join("journal/strategy.json.pending"), b"broken").unwrap();
    tick(&f).await;
    status(&f, StrategyStatus::RecoveryRequired).await;
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 0);
    assert!(f.supervisor.access(f.authority.clone()).await.is_err());
    let restarted = StrategySupervisor::new(
        f.runtime.clone(),
        f.host.clone(),
        f.store.clone(),
        Arc::new(tokio::sync::Notify::new()),
    );
    assert!(restarted.list().is_err());
}
#[tokio::test(start_paused = true)]
async fn failed_receipt_persistence_never_acknowledges_and_recovery_uses_original_submission() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    f.store.fail_from.store(2, Ordering::SeqCst);
    let run = start(&f).await;
    status(&f, StrategyStatus::RecoveryRequired).await;
    f.supervisor.drain().await;
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.acked.load(Ordering::SeqCst), 0);
    let pending = f.host.submissions.list_pending(SCOPE).unwrap();
    assert_eq!(pending.len(), 1);
    assert!(f.store.load().unwrap().runs[0].owned.is_empty());
    f.store.fail_from.store(usize::MAX, Ordering::SeqCst);
    let restarted = StrategySupervisor::new(
        f.runtime.clone(),
        f.host.clone(),
        f.store.clone(),
        Arc::new(tokio::sync::Notify::new()),
    );
    let recovered = restarted.reconcile(run.run_id).await.unwrap();
    assert_eq!(recovered.status, StrategyStatus::Paused);
    assert_eq!(
        recovered.last_receipt.unwrap().submission_id.as_deref(),
        Some(pending[0].order_link_id.as_str())
    );
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.acked.load(Ordering::SeqCst), 1);
}
#[tokio::test(start_paused = true)]
async fn forbidden_symbol_side_and_duplicate_cancel_never_dispatch() {
    let _serial = SERIAL.lock().await;
    for invalid in [
        PLACE.replace("BTCUSDT", "ETHUSDT"),
        PLACE.replace("\"Buy\"", "\"Sell\""),
        PLACE.replace("\"positionIdx\":1", "\"positionIdx\":0"),
    ] {
        let f = fixture(&invalid, CANCEL).await;
        start(&f).await;
        status(&f, StrategyStatus::Faulted).await;
        assert_eq!(f.host.placed.load(Ordering::SeqCst), 0);
    }
    let repeated = CANCEL.replace("\"phase\":2", "\"phase\":1");
    let f = fixture(PLACE, &repeated).await;
    start(&f).await;
    settled(&f, "1").await;
    tick(&f).await;
    settled(&f, "2").await;
    tick(&f).await;
    status(&f, StrategyStatus::Faulted).await;
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 1);
}
#[tokio::test(start_paused = true)]
async fn missing_ownership_cannot_be_inferred_from_a_visible_exchange_order() {
    let _serial = SERIAL.lock().await;
    let f = fixture(CANCEL, CANCEL).await;
    f.host.placed.store(1, Ordering::SeqCst);
    start(&f).await;
    status(&f, StrategyStatus::Faulted).await;
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 0);
    assert!(f.store.load().unwrap().runs[0].owned.is_empty());
}
#[tokio::test(start_paused = true)]
async fn queued_start_cannot_resurrect_after_stop_all_and_exit_revokes_fresh_access() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    let ticket = f.supervisor.access(f.authority.clone()).await.unwrap();
    let gate = f.runtime.strategy_gate().await;
    let s = f.supervisor.clone();
    let start = tokio::spawn(async move { s.start(request(ticket)).await });
    tokio::task::yield_now().await;
    f.supervisor.stop_all().await.unwrap();
    drop(gate);
    assert!(start.await.unwrap().is_err());
    assert!(f.supervisor.list().unwrap().runs.is_empty());
    f.supervisor.revoke_for_exit();
    assert!(f.supervisor.access(f.authority.clone()).await.is_err());
    f.supervisor.drain().await;
}
#[tokio::test(start_paused = true)]
async fn public_view_environment_and_receipt_codes_keep_the_closed_account_contract() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    start(&f).await;
    let mut v = settled(&f, "1").await;
    v.account.environment = format!("https://{}.example", "x".repeat(180));
    assert!(v.validate().is_ok());
    v.last_receipt.as_mut().unwrap().error_code = Some("plugin_strategy_rejected".into());
    assert!(v.validate().is_err());
    f.supervisor.stop_all().await.unwrap();
    f.supervisor.drain().await;
}

#[tokio::test(start_paused = true)]
async fn persisted_observation_prevents_resume_from_renewing_time_after_clock_rollback() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    let run = start(&f).await;
    settled(&f, "1").await;
    tick(&f).await;
    settled(&f, "2").await;
    f.supervisor
        .control(StrategyControlRequest {
            run_id: run.run_id.clone(),
            action: ControlAction::Pause,
        })
        .await
        .unwrap();
    status(&f, StrategyStatus::Paused).await;
    f.host.now.store(2000, Ordering::SeqCst);
    let restarted = StrategySupervisor::new(
        f.runtime.clone(),
        f.host.clone(),
        f.store.clone(),
        Arc::new(tokio::sync::Notify::new()),
    );
    let mut r = request(restarted.access(f.authority.clone()).await.unwrap());
    r.resume_run_id = Some(run.run_id);
    assert!(restarted.start(r).await.is_err());
}

#[tokio::test]
async fn exit_intent_never_waits_for_the_durable_state_mutex() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    let held = f.supervisor.lock().unwrap();
    let owner = f.supervisor.clone();
    let (sent, received) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        owner.revoke_for_exit();
        sent.send(()).unwrap();
    });
    let returned = received
        .recv_timeout(std::time::Duration::from_secs(1))
        .is_ok();
    drop(held);
    thread.join().unwrap();
    assert!(
        returned,
        "exit intent waited for an unrelated durable write"
    );
}

#[tokio::test(start_paused = true)]
async fn exit_during_pending_fsync_records_definitely_unsent_rejection_without_http() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    let owner = Arc::downgrade(&f.supervisor);
    *f.store.after_write.lock().unwrap() = Some(Arc::new(move |write| {
        if write == 1 {
            owner.upgrade().unwrap().revoke_for_exit();
        }
    }));
    start(&f).await;
    let view = status(&f, StrategyStatus::Stopped).await;
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 0);
    assert_eq!(f.host.acked.load(Ordering::SeqCst), 0);
    assert_eq!(view.actions_submitted, 1);
    assert_eq!(view.total_submitted_qty, "0.001");
    assert_eq!(view.last_receipt.unwrap().status, ReceiptStatus::Rejected);
    assert!(f.store.load().unwrap().runs[0].pending.is_none());
}

// Break: omitted pending is treated as an absent intent, or contradictory ownership
// is accepted as proof even though the persisted mutation count cannot explain it.
#[tokio::test(start_paused = true)]
async fn malformed_durable_ownership_or_omitted_intent_never_loads_as_safe() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    start(&f).await;
    settled(&f, "1").await;
    f.supervisor.stop_all().await.unwrap();
    f.supervisor.drain().await;
    let doc = f.store.load().unwrap();
    let mut missing = serde_json::to_value(&doc).unwrap();
    missing["runs"][0]
        .as_object_mut()
        .unwrap()
        .remove("pending");
    let missing_rejected = serde_json::from_value::<StrategyDocument>(missing).is_err();
    let mut impossible = doc.clone();
    impossible.runs[0].view.actions_submitted = 0;
    let impossible_rejected = impossible.validate().is_err();
    let mut duplicate = doc.clone();
    let owned = duplicate.runs[0].owned["owned-1"].clone();
    duplicate.runs[0]
        .owned
        .insert("different-order".into(), owned);
    duplicate.runs[0].view.actions_submitted = 2;
    duplicate.runs[0].view.sequence = "2".into();
    let duplicate_rejected = duplicate.validate().is_err();
    let raw = serde_json::to_string(&doc).unwrap();
    let member = format!(
        "\"owned-1\":{}",
        serde_json::to_string(&doc.runs[0].owned["owned-1"]).unwrap()
    );
    let duplicate_key = raw.replace(&member, &format!("{member},{member}"));
    let duplicate_key_rejected = serde_json::from_str::<StrategyDocument>(&duplicate_key).is_err();
    let missing_submission_rejected = serde_json::from_value::<PendingAction>(json!({
        "sequence":"1", "action":{"kind":"cancelOrder","order":{"symbol":"BTCUSDT","orderId":"owned-1"}}, "state":{}, "message":"x"
    })).is_err();
    let mut contradictory = doc.clone();
    contradictory.runs[0].view.last_message = "different valid revision contents".into();
    contradictory.validate().unwrap();
    std::fs::write(
        f._temp.0.join("journal/strategy.json.tmp"),
        serde_json::to_vec(&contradictory).unwrap(),
    )
    .unwrap();
    assert!(
        f.store.load().is_err(),
        "same revision chose one contradictory candidate"
    );
    let checks = [
        ("missing pending", missing_rejected),
        ("unfunded ownership", impossible_rejected),
        ("duplicate submission", duplicate_rejected),
        ("duplicate exchange ID key", duplicate_key_rejected),
        ("missing submission nullable", missing_submission_rejected),
    ];
    assert!(checks.iter().all(|(_, rejected)| *rejected), "{checks:?}");
}

// Break: failed cancellation is replayed or a nonterminal exact query is mistaken
// for terminal cancellation. Neither branch may erase ownership or spent caps.
#[tokio::test(start_paused = true)]
async fn unknown_cancel_is_never_retried_and_only_terminal_reconciliation_resolves_it() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    let run = start(&f).await;
    settled(&f, "1").await;
    f.host.unknown_cancel.store(true, Ordering::SeqCst);
    tick(&f).await;
    status(&f, StrategyStatus::RecoveryRequired).await;
    f.supervisor.drain().await;
    tick(&f).await;
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 1);
    f.host.terminal_cancel.store(false, Ordering::SeqCst);
    assert!(f.supervisor.reconcile(run.run_id.clone()).await.is_err());
    let unresolved = f.store.load().unwrap();
    assert!(unresolved.runs[0].pending.is_some());
    assert!(unresolved.runs[0].owned["owned-1"].cancel_requested);
    f.host.terminal_cancel.store(true, Ordering::SeqCst);
    let resolved = f.supervisor.reconcile(run.run_id).await.unwrap();
    assert_eq!(resolved.status, StrategyStatus::Paused);
    assert_eq!(resolved.actions_submitted, 2);
    assert_eq!(resolved.total_submitted_qty, "0.001");
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 1);
}

// Break: slow snapshots or sleep gaps feed a decision without revoking admission.
#[tokio::test(start_paused = true)]
async fn slow_read_and_missed_scheduling_gap_pause_without_using_stale_data() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    f.host.read_delay_ms.store(30001, Ordering::SeqCst);
    start(&f).await;
    let v = status(&f, StrategyStatus::Paused).await;
    assert_eq!(
        v.reason.as_deref(),
        Some("plugin_strategy_data_unavailable")
    );
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 0);
    f.supervisor.drain().await;
    let f = fixture(PLACE, CANCEL).await;
    start(&f).await;
    settled(&f, "1").await;
    f.host.now.fetch_add(30001, Ordering::SeqCst);
    tokio::time::advance(std::time::Duration::from_millis(30001)).await;
    let v = status(&f, StrategyStatus::Paused).await;
    assert_eq!(v.reason.as_deref(), Some("plugin_strategy_stale"));
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 0);
}

// Break: policy is only UI validation, cancellation refunds budget, or the native
// guard skips cumulative/action/reduce-only caps when no global risk service exists.
#[tokio::test(start_paused = true)]
async fn cumulative_quantity_action_and_reduce_only_limits_each_block_dispatch() {
    let _serial = SERIAL.lock().await;
    for case in ["total", "actions", "reduceOnly"] {
        let f = fixture(PLACE, if case == "total" { PLACE } else { CANCEL }).await;
        let mut r = request(f.supervisor.access(f.authority.clone()).await.unwrap());
        match case {
            "total" => r.policy.max_total_qty = "0.001".into(),
            "actions" => r.policy.max_actions = 1,
            _ => r.policy.reduce_only = true,
        }
        f.supervisor.start(r).await.unwrap();
        if case != "reduceOnly" {
            settled(&f, "1").await;
            tick(&f).await;
        }
        let v = status(&f, StrategyStatus::Completed).await;
        assert_eq!(v.reason.as_deref(), Some("plugin_strategy_limit_reached"));
        assert_eq!(
            f.host.placed.load(Ordering::SeqCst),
            usize::from(case != "reduceOnly")
        );
        assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 0);
        f.supervisor.drain().await;
    }
}

// Break: wall time alone extends a run when the clock stalls, or an expired run
// can be explicitly resumed to mint a fresh monotonic deadline.
#[tokio::test(start_paused = true)]
async fn monotonic_run_expiry_works_with_frozen_wall_time_and_cannot_be_resumed() {
    let _serial = SERIAL.lock().await;
    let none = r#"{"state":{},"action":{"kind":"none"},"message":"waiting"}"#;
    let f = fixture(none, none).await;
    let mut r = request(f.supervisor.access(f.authority.clone()).await.unwrap());
    r.policy.max_run_seconds = 60;
    let run = f.supervisor.start(r.clone()).await.unwrap();
    settled(&f, "1").await;
    for sequence in 2..=12 {
        tokio::time::advance(std::time::Duration::from_secs(5)).await;
        settled(&f, &sequence.to_string()).await;
    }
    tokio::time::advance(std::time::Duration::from_secs(5)).await;
    let expired = status(&f, StrategyStatus::Completed).await;
    assert_eq!(expired.reason.as_deref(), Some("plugin_strategy_expired"));
    assert_eq!(f.host.now_ms(), 1000);
    f.supervisor.drain().await;
    f.supervisor
        .control(StrategyControlRequest {
            run_id: run.run_id.clone(),
            action: ControlAction::Pause,
        })
        .await
        .unwrap();
    r.authorization_token = f
        .supervisor
        .access(f.authority.clone())
        .await
        .unwrap()
        .authorization_token;
    r.request_id = uuid::Uuid::new_v4().to_string();
    r.resume_run_id = Some(run.run_id);
    assert!(f.supervisor.start(r).await.is_err());
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn pause_resume_cannot_renew_monotonic_lifetime_while_wall_time_is_frozen() {
    let _serial = SERIAL.lock().await;
    let none = r#"{"state":{},"action":{"kind":"none"},"message":"waiting"}"#;
    let f = fixture(none, none).await;
    let mut r = request(f.supervisor.access(f.authority.clone()).await.unwrap());
    let run = f.supervisor.start(r.clone()).await.unwrap();
    settled(&f, "1").await;
    tokio::time::advance(std::time::Duration::from_secs(5)).await;
    settled(&f, "2").await;
    f.supervisor
        .control(StrategyControlRequest {
            run_id: run.run_id.clone(),
            action: ControlAction::Pause,
        })
        .await
        .unwrap();
    f.supervisor.drain().await;
    r.authorization_token = f
        .supervisor
        .access(f.authority.clone())
        .await
        .unwrap()
        .authorization_token;
    r.request_id = uuid::Uuid::new_v4().to_string();
    r.resume_run_id = Some(run.run_id);
    assert!(
        f.supervisor.start(r).await.is_err(),
        "pause granted a fresh lifetime using frozen wall time"
    );
}

// Break: storage capacity is implemented by pruning recovery/ownership history.
#[tokio::test(start_paused = true)]
async fn thirty_two_completed_records_are_retained_and_the_next_start_is_refused() {
    let _serial = SERIAL.lock().await;
    let f = fixture(STOP, STOP).await;
    let mut retained = Vec::new();
    for _ in 0..32 {
        retained.push(start(&f).await.run_id);
        f.supervisor.drain().await;
    }
    let ticket = f.supervisor.access(f.authority.clone()).await.unwrap();
    assert!(f.supervisor.start(request(ticket)).await.is_err());
    let ids: Vec<_> = f
        .supervisor
        .list()
        .unwrap()
        .runs
        .into_iter()
        .map(|r| r.run_id)
        .collect();
    assert_eq!(ids, retained);
    assert_eq!(f.store.load().unwrap().runs.len(), 32);
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 0);
}

// Break: final authority read yields and stop revokes admission after the guard's
// first check, then that old check is incorrectly reused for HTTP dispatch.
#[tokio::test(start_paused = true)]
async fn stop_during_final_authority_read_closes_the_unsent_intent_without_http() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    let owner = Arc::downgrade(&f.supervisor);
    let store = f.store.clone();
    *f.host.after_authority.lock().unwrap() = Some(Arc::new(move || {
        if store
            .load()
            .unwrap()
            .runs
            .iter()
            .any(|r| r.pending.is_some())
        {
            owner.upgrade().unwrap().revoke_for_exit();
        }
    }));
    start(&f).await;
    let v = status(&f, StrategyStatus::Stopped).await;
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 0);
    assert_eq!(v.actions_submitted, 1);
    assert_eq!(v.last_receipt.unwrap().status, ReceiptStatus::Rejected);
    assert!(f.store.load().unwrap().runs[0].pending.is_none());
}

// Break: shutdown wraps an async drain in timeout, but drain enters a blocking
// fsync mutex and prevents that timer from ever being polled.
#[tokio::test]
async fn drain_timeout_remains_pollable_while_persistence_mutex_is_held() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    f.host.hold_http.store(true, Ordering::SeqCst);
    start(&f).await;
    f.host.entered.notified().await;
    f.supervisor.revoke_for_exit();
    let held = f.supervisor.lock().unwrap();
    let owner = f.supervisor.clone();
    let (sent, received) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let timed_out = runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(20), owner.drain())
                .await
                .is_err()
        });
        sent.send(timed_out).unwrap();
    });
    let polled_timeout = received
        .recv_timeout(std::time::Duration::from_secs(1))
        .ok();
    drop(held);
    thread.join().unwrap();
    f.host.release.notify_one();
    f.supervisor.drain().await;
    assert_eq!(
        polled_timeout,
        Some(true),
        "drain blocked the executor on persistence"
    );
    assert_eq!(f.host.acked.load(Ordering::SeqCst), 1);
}

// Review1: preparation is not mutation admission. Stop, expiry and snapshot age
// must be checked after preparation, with no replay of the definitely-unsent intent.
#[tokio::test(start_paused = true)]
async fn blocked_placement_preparation_rechecks_stop_expiry_and_snapshot_age() {
    let _serial = SERIAL.lock().await;
    for cause in ["stop", "expiry", "stale"] {
        let f = fixture(PLACE, CANCEL).await;
        f.host.hold_preparation.store(true, Ordering::SeqCst);
        let mut request = request(f.supervisor.access(f.authority.clone()).await.unwrap());
        request.policy.max_run_seconds = 60;
        f.supervisor.start(request).await.unwrap();
        f.host.preparing.notified().await;
        let expected = match cause {
            "stop" => {
                f.supervisor.stop_all().await.unwrap();
                StrategyStatus::Stopped
            }
            "expiry" => {
                f.host.now.fetch_add(60001, Ordering::SeqCst);
                tokio::time::advance(std::time::Duration::from_millis(60001)).await;
                StrategyStatus::Completed
            }
            _ => {
                f.host.now.fetch_add(30001, Ordering::SeqCst);
                tokio::time::advance(std::time::Duration::from_millis(30001)).await;
                StrategyStatus::Paused
            }
        };
        f.host.prepare_release.notify_one();
        // Let any wrongly admitted HTTP finish, so the assertion detects dispatch
        // rather than merely testing a transient status before the host resumes.
        for _ in 0..100_000 {
            if f.store.load().unwrap().runs[0].pending.is_none() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            f.host.placed.load(Ordering::SeqCst),
            0,
            "{cause} dispatched after preparation"
        );
        let view = status(&f, expected).await;
        assert_eq!(view.actions_submitted, 1);
        assert_eq!(view.total_submitted_qty, "0.001");
        assert_eq!(view.last_receipt.unwrap().status, ReceiptStatus::Rejected);
        f.supervisor.drain().await;
    }
}

#[tokio::test(start_paused = true)]
async fn blocked_cancellation_preparation_rechecks_stop_before_mutation_handoff() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    start(&f).await;
    settled(&f, "1").await;
    f.host.hold_preparation.store(true, Ordering::SeqCst);
    tick(&f).await;
    f.host.preparing.notified().await;
    f.supervisor.stop_all().await.unwrap();
    f.host.prepare_release.notify_one();
    f.supervisor.drain().await;
    assert_eq!(f.host.cancelled.load(Ordering::SeqCst), 0);
    let view = f.supervisor.list().unwrap().runs[0].clone();
    assert_eq!(view.actions_submitted, 2);
    assert_eq!(view.last_receipt.unwrap().status, ReceiptStatus::Rejected);
}

#[tokio::test(start_paused = true)]
async fn completed_guest_waiting_on_account_gate_cannot_dispatch_old_snapshot_decision() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    let held = f.host.lifecycle.read_guard().await;
    let run = start(&f).await;
    let mut computed = false;
    for _ in 0..100_000 {
        computed = f.host.reads.load(Ordering::SeqCst) > 0
            && f.supervisor.lock().unwrap().live[&run.run_id]
                .cancel
                .lock()
                .unwrap()
                .is_none();
        if computed {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(computed, "guest did not finish behind the account gate");
    f.host.now.fetch_add(30001, Ordering::SeqCst);
    tokio::time::advance(std::time::Duration::from_millis(30001)).await;
    drop(held);
    for _ in 0..100_000 {
        let v = f.supervisor.list().unwrap().runs[0].clone();
        if v.status != StrategyStatus::Running || v.sequence == "1" {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 0);
    let view = status(&f, StrategyStatus::Paused).await;
    assert_eq!(
        view.reason.as_deref(),
        Some("plugin_strategy_data_unavailable")
    );
    assert_eq!(view.actions_submitted, 0);
}

#[tokio::test(start_paused = true)]
async fn real_pending_document_with_missing_quantity_debit_or_impossible_sequence_disables_authority(
) {
    let _serial = SERIAL.lock().await;
    for corruption in ["quantity", "sequence"] {
        let f = fixture(PLACE, CANCEL).await;
        f.host.unknown.store(true, Ordering::SeqCst);
        start(&f).await;
        status(&f, StrategyStatus::RecoveryRequired).await;
        f.supervisor.drain().await;
        let mut doc = f.store.load().unwrap();
        assert!(doc.runs[0].pending.is_some());
        if corruption == "quantity" {
            doc.runs[0].view.total_submitted_qty = "0".into();
        } else {
            doc.runs[0].view.sequence = "0".into();
            doc.runs[0].view.last_receipt.as_mut().unwrap().sequence = "0".into();
            doc.runs[0].pending.as_mut().unwrap().sequence = "0".into();
        }
        std::fs::write(
            f._temp.0.join("journal/strategy.json"),
            serde_json::to_vec(&doc).unwrap(),
        )
        .unwrap();
        let restarted = StrategySupervisor::new(
            f.runtime.clone(),
            f.host.clone(),
            f.store.clone(),
            Arc::new(tokio::sync::Notify::new()),
        );
        assert!(
            restarted.list().is_err(),
            "{corruption} corruption loaded as authority"
        );
        assert!(restarted.access(f.authority.clone()).await.is_err());
        assert_eq!(f.host.placed.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test(start_paused = true)]
async fn rollback_after_start_commit_before_first_pass_pauses_without_completing() {
    let _serial = SERIAL.lock().await;
    let f = fixture(PLACE, CANCEL).await;
    let host = Arc::downgrade(&f.host);
    *f.store.after_write.lock().unwrap() = Some(Arc::new(move |index| {
        if index == 0 {
            host.upgrade().unwrap().now.store(0, Ordering::SeqCst);
        }
    }));
    start(&f).await;
    f.supervisor.drain().await;
    let view = f.supervisor.list().unwrap().runs[0].clone();
    assert_eq!(view.status, StrategyStatus::Paused);
    assert_eq!(view.reason.as_deref(), Some("plugin_strategy_stale"));
    assert_eq!(f.host.placed.load(Ordering::SeqCst), 0);
}
