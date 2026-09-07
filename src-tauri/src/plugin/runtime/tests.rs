use super::PluginRuntime;
use crate::error::{AppError, AppResult};
use crate::plugin::discovery::{LocalDiscoveryOutcome, LocalPluginDiscovery};
use crate::plugin::manifest::PluginManifestV1;
use crate::plugin::record::PluginRecord;
use crate::plugin::PluginRegistry;
use crate::storage::plugin_state::{PluginStateFileV2, PluginStateLoad, PluginStatePersistence};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

const WATCHDOG: Duration = Duration::from_secs(10);

#[derive(Clone, Default)]
struct MemoryPersistence(Arc<Mutex<MemoryState>>);

#[derive(Default)]
struct MemoryState {
    state: Option<PluginStateFileV2>,
    unavailable: bool,
    loads: usize,
    saves: usize,
}

impl PluginStatePersistence for MemoryPersistence {
    fn load(&self) -> AppResult<PluginStateLoad> {
        let mut memory = self.0.lock().unwrap();
        memory.loads += 1;
        if memory.unavailable {
            return Err(AppError::Internal("synthetic unavailable store".into()));
        }
        Ok(PluginStateLoad {
            state: memory
                .state
                .clone()
                .unwrap_or_else(PluginStateFileV2::empty),
            requires_rewrite: false,
        })
    }

    fn save(&self, next: &PluginStateFileV2) -> AppResult<()> {
        let mut memory = self.0.lock().unwrap();
        memory.saves += 1;
        memory.state = Some(next.clone());
        Ok(())
    }
}

// Only the blocking discovery boundary is controlled; publication and mutations
// use the real registry and its actual persisted-state transaction.
struct ControlledDiscovery {
    scans: Mutex<VecDeque<Scan>>,
    calls: AtomicUsize,
    active: AtomicUsize,
    max_active: AtomicUsize,
}

struct Scan {
    outcome: LocalDiscoveryOutcome,
    started: oneshot::Sender<()>,
    release: mpsc::Receiver<()>,
}

struct ScanControl {
    started: oneshot::Receiver<()>,
    release: mpsc::Sender<()>,
}

impl ScanControl {
    async fn wait_started(&mut self) {
        tokio::time::timeout(WATCHDOG, &mut self.started)
            .await
            .expect("scan must start")
            .unwrap();
    }

    fn release(&self) {
        self.release.send(()).unwrap();
    }
}

impl ControlledDiscovery {
    fn new(outcomes: Vec<LocalDiscoveryOutcome>) -> (Arc<Self>, Vec<ScanControl>) {
        let mut scans = VecDeque::new();
        let mut controls = Vec::new();
        for outcome in outcomes {
            let (started_tx, started_rx) = oneshot::channel();
            let (release_tx, release_rx) = mpsc::channel();
            scans.push_back(Scan {
                outcome,
                started: started_tx,
                release: release_rx,
            });
            controls.push(ScanControl {
                started: started_rx,
                release: release_tx,
            });
        }
        (
            Arc::new(Self {
                scans: Mutex::new(scans),
                calls: AtomicUsize::new(0),
                active: AtomicUsize::new(0),
                max_active: AtomicUsize::new(0),
            }),
            controls,
        )
    }
}

impl LocalPluginDiscovery for ControlledDiscovery {
    fn discover(&self) -> LocalDiscoveryOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let scan = self
            .scans
            .lock()
            .unwrap()
            .pop_front()
            .expect("planned scan");
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        scan.started.send(()).unwrap();
        scan.release.recv_timeout(WATCHDOG).expect("scan released");
        self.active.fetch_sub(1, Ordering::SeqCst);
        scan.outcome
    }
}

fn manifest(id: &str, version: &str) -> PluginManifestV1 {
    serde_json::from_value(json!({
        "schemaVersion": 1, "id": id, "publisherId": "com.example",
        "publisher": "Example", "name": "Alpha", "description": "Metadata only",
        "version": version, "contributions": [], "requestedCapabilities": []
    }))
    .unwrap()
}

fn local(version: &str) -> LocalDiscoveryOutcome {
    LocalDiscoveryOutcome::available(vec![PluginRecord::local_declarative(manifest(
        "com.example.alpha",
        version,
    ))
    .unwrap()])
}

fn runtime_with(
    discovery: Arc<dyn LocalPluginDiscovery>,
    persistence: &MemoryPersistence,
) -> Arc<PluginRuntime> {
    Arc::new(PluginRuntime::initialize(
        PluginRegistry::initialize(
            vec![manifest("com.example.builtin", "1.0.0")],
            Box::new(persistence.clone()),
        ),
        discovery,
    ))
}

fn item(snapshot: &Value, id: &str) -> Value {
    snapshot["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["manifest"]["id"] == id)
        .unwrap()
        .clone()
}

// Catches eager discovery, a missing in-gate double-check, or implicit rescans.
#[tokio::test]
async fn concurrent_first_gets_scan_once_and_later_get_does_not_rescan() {
    let (discovery, mut scans) = ControlledDiscovery::new(vec![local("1.0.0")]);
    let runtime = runtime_with(discovery.clone(), &MemoryPersistence::default());
    assert_eq!(discovery.calls.load(Ordering::SeqCst), 0);
    let first = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.get_catalog().await }
    });
    scans[0].wait_started().await;
    let second = runtime.get_catalog();
    tokio::pin!(second);
    assert!(futures_util::poll!(&mut second).is_pending());
    scans[0].release();
    let left = first.await.unwrap().unwrap();
    let right = second.await.unwrap();
    assert_eq!(left.catalog_generation, "1");
    assert_eq!(right.catalog_generation, "1");
    assert_eq!(runtime.get_catalog().await.unwrap().catalog_generation, "1");
    assert_eq!(discovery.calls.load(Ordering::SeqCst), 1);
}

// Catches holding either registry guard on the blocking worker boundary.
#[tokio::test]
async fn scan_does_not_hold_registry_locks_and_prepublication_toggle_survives() {
    let persistence = MemoryPersistence::default();
    let (discovery, mut scans) = ControlledDiscovery::new(vec![local("1.0.0")]);
    let runtime = runtime_with(discovery, &persistence);
    let reload = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.reload_catalog().await }
    });
    scans[0].wait_started().await;
    assert_eq!(runtime.registry.try_read().unwrap().catalog_generation(), 0);
    assert!(runtime.registry.try_write().is_ok());
    assert!(runtime.reload_gate.try_lock().is_err());
    let toggled = tokio::time::timeout(
        WATCHDOG,
        runtime.set_enabled("com.example.builtin", true, "0"),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(toggled.revision, "1");
    assert_eq!(toggled.catalog_generation, "0");
    scans[0].release();
    let published = serde_json::to_value(reload.await.unwrap().unwrap()).unwrap();
    assert_eq!(published["catalogGeneration"], "1");
    assert_eq!(published["revision"], "1");
    assert_eq!(item(&published, "com.example.builtin")["status"], "enabled");
    assert_eq!(item(&published, "com.example.alpha")["status"], "disabled");
    let enabled = runtime
        .set_enabled("com.example.alpha", true, "1")
        .await
        .unwrap();
    assert_eq!(enabled.revision, "2");
    assert_eq!(enabled.catalog_generation, "1");
    let error = runtime
        .set_enabled("com.example.alpha", false, "0")
        .await
        .unwrap_err();
    assert_eq!(
        serde_json::to_value(error).unwrap()["code"],
        "plugin_catalog_stale"
    );
    assert_eq!(persistence.0.lock().unwrap().saves, 2);
}

// Catches overlapping scans, lost publication order, or unchanged generation churn.
#[tokio::test]
async fn reloads_are_serialized_and_publish_in_scan_order() {
    let (discovery, mut scans) =
        ControlledDiscovery::new(vec![local("1.0.0"), local("2.0.0"), local("2.0.0")]);
    let runtime = runtime_with(discovery.clone(), &MemoryPersistence::default());
    let first = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.reload_catalog().await }
    });
    scans[0].wait_started().await;
    let second = runtime.reload_catalog();
    tokio::pin!(second);
    assert!(futures_util::poll!(&mut second).is_pending());
    assert!(runtime.reload_gate.try_lock().is_err());
    scans[0].release();
    scans[1].wait_started().await;
    let first = serde_json::to_value(first.await.unwrap().unwrap()).unwrap();
    assert_eq!(first["catalogGeneration"], "1");
    assert_eq!(
        item(&first, "com.example.alpha")["manifest"]["version"],
        "1.0.0"
    );
    // Ordinary get after first publication returns the current catalog during reload.
    let during = tokio::time::timeout(WATCHDOG, runtime.get_catalog())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(during.catalog_generation, "1");
    scans[1].release();
    let second = serde_json::to_value(second.await.unwrap()).unwrap();
    assert_eq!(second["catalogGeneration"], "2");
    assert_eq!(
        item(&second, "com.example.alpha")["manifest"]["version"],
        "2.0.0"
    );
    scans[2].release();
    assert_eq!(
        runtime.reload_catalog().await.unwrap().catalog_generation,
        "2"
    );
    scans[2].wait_started().await;
    assert_eq!(discovery.calls.load(Ordering::SeqCst), 3);
    assert_eq!(discovery.max_active.load(Ordering::SeqCst), 1);
}

// Catches retrying local failure on ordinary get instead of only explicit reload.
#[tokio::test]
async fn failed_first_discovery_counts_as_attempted_and_explicit_reload_retries() {
    let (discovery, mut scans) =
        ControlledDiscovery::new(vec![LocalDiscoveryOutcome::unavailable(), local("1.0.0")]);
    let runtime = runtime_with(discovery.clone(), &MemoryPersistence::default());
    scans[0].release();
    let failed = serde_json::to_value(runtime.get_catalog().await.unwrap()).unwrap();
    scans[0].wait_started().await;
    assert_eq!(failed["localDiscovery"]["status"], "unavailable");
    assert_eq!(failed["availability"], "available");
    assert_eq!(failed["catalogGeneration"], "1");
    assert_eq!(runtime.get_catalog().await.unwrap().catalog_generation, "1");
    assert_eq!(discovery.calls.load(Ordering::SeqCst), 1);
    scans[1].release();
    let recovered = serde_json::to_value(runtime.reload_catalog().await.unwrap()).unwrap();
    scans[1].wait_started().await;
    assert_eq!(recovered["catalogGeneration"], "2");
    assert_eq!(recovered["localDiscovery"]["status"], "available");
    assert_eq!(discovery.calls.load(Ordering::SeqCst), 2);
}

// Catches coupling state retry to a new local scan, or retrying storage on mutation.
#[tokio::test]
async fn later_get_recovers_state_without_rescanning_or_mutation_retry() {
    let persistence = MemoryPersistence::default();
    persistence.0.lock().unwrap().unavailable = true;
    let (discovery, mut scans) = ControlledDiscovery::new(vec![local("1.0.0")]);
    let runtime = runtime_with(discovery.clone(), &persistence);
    scans[0].release();
    let unavailable = serde_json::to_value(runtime.get_catalog().await.unwrap()).unwrap();
    scans[0].wait_started().await;
    assert_eq!(unavailable["availabilityReasonCode"], "stateUnavailable");
    assert_eq!(persistence.0.lock().unwrap().loads, 2);
    persistence.0.lock().unwrap().unavailable = false;
    let error = runtime
        .set_enabled("com.example.alpha", true, "1")
        .await
        .unwrap_err();
    assert_eq!(
        serde_json::to_value(error).unwrap()["code"],
        "plugin_state_unavailable"
    );
    assert_eq!(persistence.0.lock().unwrap().loads, 2);
    let recovered = serde_json::to_value(runtime.get_catalog().await.unwrap()).unwrap();
    assert_eq!(recovered["availability"], "available");
    assert_eq!(recovered["catalogGeneration"], "1");
    assert_eq!(recovered["revision"], "0");
    runtime.get_catalog().await.unwrap();
    assert_eq!(persistence.0.lock().unwrap().loads, 3);
    assert_eq!(discovery.calls.load(Ordering::SeqCst), 1);
}

// Catches an aborted command releasing the gate while its blocking scan survives.
#[tokio::test]
async fn cancelling_initial_get_or_reload_does_not_cancel_publication_or_release_gate() {
    for initial_get in [true, false] {
        let (discovery, mut scans) = ControlledDiscovery::new(vec![local("1.0.0"), local("2.0.0")]);
        let runtime = runtime_with(discovery.clone(), &MemoryPersistence::default());
        let cancelled = tokio::spawn({
            let runtime = runtime.clone();
            async move {
                if initial_get {
                    runtime.get_catalog().await
                } else {
                    runtime.reload_catalog().await
                }
            }
        });
        scans[0].wait_started().await;
        cancelled.abort();
        assert!(cancelled.await.unwrap_err().is_cancelled());
        assert!(
            runtime.reload_gate.try_lock().is_err(),
            "scan owns the gate after caller cancellation"
        );
        let later = runtime.reload_catalog();
        tokio::pin!(later);
        assert!(futures_util::poll!(&mut later).is_pending());
        scans[0].release();
        scans[1].wait_started().await;
        assert_eq!(runtime.registry.try_read().unwrap().catalog_generation(), 1);
        assert_eq!(discovery.max_active.load(Ordering::SeqCst), 1);
        scans[1].release();
        let later = serde_json::to_value(later.await.unwrap()).unwrap();
        assert_eq!(later["catalogGeneration"], "2");
        assert_eq!(
            item(&later, "com.example.alpha")["manifest"]["version"],
            "2.0.0"
        );
        assert_eq!(runtime.get_catalog().await.unwrap().catalog_generation, "2");
        assert_eq!(discovery.calls.load(Ordering::SeqCst), 2);
    }
}

struct PanicOnceDiscovery(AtomicUsize);

impl LocalPluginDiscovery for PanicOnceDiscovery {
    fn discover(&self) -> LocalDiscoveryOutcome {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("synthetic private discovery failure");
        }
        local("1.0.0")
    }
}

// Catches leaking a blocking-worker panic, or silently retrying it on a later get.
#[tokio::test]
async fn blocking_worker_failure_publishes_unavailable_and_allows_explicit_retry() {
    let discovery = Arc::new(PanicOnceDiscovery(AtomicUsize::new(0)));
    let runtime = runtime_with(discovery.clone(), &MemoryPersistence::default());
    let failed = serde_json::to_value(runtime.get_catalog().await.unwrap()).unwrap();
    assert_eq!(failed["localDiscovery"]["status"], "unavailable");
    assert_eq!(failed["catalogGeneration"], "1");
    assert!(!failed.to_string().contains("private"));
    runtime.get_catalog().await.unwrap();
    assert_eq!(discovery.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime.reload_catalog().await.unwrap().catalog_generation,
        "2"
    );
}

// Catches accepting A before B but registering B first under Tokio's worker LIFO
// scheduling. The harness itself runs on the sole worker, so neither owned
// request can run between the consecutive first polls below.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn reload_acceptance_order_survives_lifo_and_caller_cancellation() {
    let harness = tokio::spawn(async {
        let (discovery, mut scans) = ControlledDiscovery::new(vec![
            local("1.0.0"),
            local("2.0.0"),
            local("3.0.0"),
            local("4.0.0"),
            local("5.0.0"),
        ]);
        let runtime = runtime_with(discovery.clone(), &MemoryPersistence::default());
        let mut first = Box::pin(runtime.reload_catalog());
        let mut second = Box::pin(runtime.reload_catalog());
        assert!(futures_util::poll!(&mut first).is_pending());
        assert!(futures_util::poll!(&mut second).is_pending());
        scans[0].wait_started().await;
        scans[0].release();
        scans[1].wait_started().await;
        scans[1].release();
        let first = serde_json::to_value(first.await.unwrap()).unwrap();
        let second = serde_json::to_value(second.await.unwrap()).unwrap();
        assert_eq!(
            first["catalogGeneration"], "1",
            "first accepted reload must publish first"
        );
        assert_eq!(
            item(&first, "com.example.alpha")["manifest"]["version"],
            "1.0.0"
        );
        assert_eq!(second["catalogGeneration"], "2");
        assert_eq!(
            item(&second, "com.example.alpha")["manifest"]["version"],
            "2.0.0"
        );

        let mut active = Box::pin(runtime.reload_catalog());
        let mut queued = Box::pin(runtime.reload_catalog());
        let mut last = Box::pin(runtime.reload_catalog());
        assert!(futures_util::poll!(&mut active).is_pending());
        assert!(futures_util::poll!(&mut queued).is_pending());
        assert!(futures_util::poll!(&mut last).is_pending());
        scans[2].wait_started().await;
        // Both the active caller and an already accepted queued caller disappear.
        drop(active);
        drop(queued);
        assert!(runtime.reload_gate.try_lock().is_err());
        scans[2].release();
        scans[3].wait_started().await;
        assert_eq!(runtime.registry.try_read().unwrap().catalog_generation(), 3);
        assert!(futures_util::poll!(&mut last).is_pending());
        scans[3].release();
        scans[4].wait_started().await;
        assert_eq!(runtime.registry.try_read().unwrap().catalog_generation(), 4);
        scans[4].release();
        let last = serde_json::to_value(last.await.unwrap()).unwrap();
        assert_eq!(last["catalogGeneration"], "5");
        assert_eq!(
            item(&last, "com.example.alpha")["manifest"]["version"],
            "5.0.0"
        );
        assert_eq!(runtime.get_catalog().await.unwrap().catalog_generation, "5");
        assert_eq!(discovery.calls.load(Ordering::SeqCst), 5);
        assert_eq!(discovery.max_active.load(Ordering::SeqCst), 1);
    });
    tokio::time::timeout(WATCHDOG, harness)
        .await
        .unwrap()
        .unwrap();
}

// Catches a first lock poll yielding for cooperative budget instead of reserving
// FIFO position, which would reintroduce scheduling-dependent acceptance order.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn acceptance_reservation_ignores_exhausted_cooperative_budget() {
    let harness = tokio::spawn(async {
        let (discovery, mut scans) = ControlledDiscovery::new(vec![local("1.0.0"), local("2.0.0")]);
        let runtime = runtime_with(discovery, &MemoryPersistence::default());
        let mut first = Box::pin(runtime.reload_catalog());
        let mut second = Box::pin(runtime.reload_catalog());
        let mut exhausted = false;
        for _ in 0..1024 {
            let budget = tokio::task::consume_budget();
            tokio::pin!(budget);
            if futures_util::poll!(&mut budget).is_pending() {
                exhausted = true;
                break;
            }
        }
        assert!(exhausted, "the harness must deplete its cooperative budget");
        assert!(futures_util::poll!(&mut first).is_pending());
        assert!(futures_util::poll!(&mut second).is_pending());
        scans[0].wait_started().await;
        scans[0].release();
        scans[1].wait_started().await;
        scans[1].release();
        assert_eq!(first.await.unwrap().catalog_generation, "1");
        assert_eq!(second.await.unwrap().catalog_generation, "2");
    });
    tokio::time::timeout(WATCHDOG, harness)
        .await
        .unwrap()
        .unwrap();
}

struct RecoveryPanicPersistence(AtomicUsize);

impl PluginStatePersistence for RecoveryPanicPersistence {
    fn load(&self) -> AppResult<PluginStateLoad> {
        match self.0.fetch_add(1, Ordering::SeqCst) {
            0 => Err(AppError::Internal("synthetic unavailable store".into())),
            1 => panic!("synthetic private state-recovery detail"),
            _ => Ok(PluginStateLoad {
                state: PluginStateFileV2::empty(),
                requires_rewrite: false,
            }),
        }
    }

    fn save(&self, _next: &PluginStateFileV2) -> AppResult<()> {
        Ok(())
    }
}

// Catches an owned request panic after successful discovery escaping its safe
// boundary, retaining the gate, or losing the completed-attempt marker.
#[tokio::test]
async fn post_discovery_recovery_panic_is_safe_releases_gate_and_remains_retryable() {
    let (discovery, mut scans) = ControlledDiscovery::new(vec![local("1.0.0"), local("2.0.0")]);
    let runtime = Arc::new(PluginRuntime::initialize(
        PluginRegistry::initialize(
            vec![],
            Box::new(RecoveryPanicPersistence(AtomicUsize::new(0))),
        ),
        discovery.clone(),
    ));
    scans[0].release();
    let error = runtime.get_catalog().await.unwrap_err();
    scans[0].wait_started().await;
    assert!(matches!(error, AppError::Internal(_)));
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        json!("内部错误: 插件目录请求暂不可用")
    );
    assert!(runtime.reload_gate.try_lock().is_ok());
    let recovered = serde_json::to_value(runtime.get_catalog().await.unwrap()).unwrap();
    assert_eq!(recovered["catalogGeneration"], "1");
    assert_eq!(recovered["availability"], "available");
    assert_eq!(discovery.calls.load(Ordering::SeqCst), 1);
    scans[1].release();
    assert_eq!(
        runtime.reload_catalog().await.unwrap().catalog_generation,
        "2"
    );
    scans[1].wait_started().await;
    assert!(runtime.reload_gate.try_lock().is_ok());
}

// Catches a detached request retaining its gate or losing the attempted marker
// after a post-discovery panic. No global logger or panic hook is changed.
#[tokio::test]
async fn cancelled_waiter_recovery_panic_releases_gate_and_remains_usable() {
    let (discovery, mut scans) = ControlledDiscovery::new(vec![local("1.0.0"), local("2.0.0")]);
    let runtime = Arc::new(PluginRuntime::initialize(
        PluginRegistry::initialize(
            vec![],
            Box::new(RecoveryPanicPersistence(AtomicUsize::new(0))),
        ),
        discovery.clone(),
    ));
    let mut waiter = Box::pin(runtime.get_catalog());
    assert!(futures_util::poll!(&mut waiter).is_pending());
    scans[0].wait_started().await;
    drop(waiter);
    assert!(runtime.reload_gate.try_lock().is_err());
    scans[0].release();
    let finished = tokio::time::timeout(WATCHDOG, runtime.reload_gate.lock())
        .await
        .unwrap();
    drop(finished);
    assert_eq!(runtime.get_catalog().await.unwrap().catalog_generation, "1");
    assert_eq!(discovery.calls.load(Ordering::SeqCst), 1);
    scans[1].release();
    assert_eq!(
        runtime.reload_catalog().await.unwrap().catalog_generation,
        "2"
    );
    scans[1].wait_started().await;
}
