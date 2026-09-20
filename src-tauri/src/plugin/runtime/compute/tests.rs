use super::*;
use crate::plugin::compute::tests::{params, SMA};
use crate::plugin::discovery::{LocalDiscoveryOutcome, LocalPluginDiscovery};
use crate::plugin::manifest::PluginManifest;
use crate::plugin::record::PluginRecord;
use crate::storage::plugin_state::{PluginStateFileV2, PluginStateLoad, PluginStatePersistence};
use serde_json::{json, Value};

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
fn manifest() -> PluginManifest {
    serde_json::from_value(json!({"schemaVersion":4,"id":"com.example.compute","publisherId":"com.example",
        "publisher":"Example","name":"Compute","description":"Numerical guest","version":"1.0.0","requestedCapabilities":[],
        "contributions":[{"kind":"command","contributionId":"series.sma","title":"SMA","actionId":"sandbox.computeSeries","params":params(SMA)},
            {"kind":"command","contributionId":"series.info","title":"Info","actionId":"host.showInfo","params":{"title":"Info","text":"Information"}}]})).unwrap()
}
async fn fixture() -> (Arc<PluginRuntime>, ComputeRequest) {
    let mut runtime = PluginRuntime::initialize(
        PluginRegistry::initialize(
            vec![],
            Box::new(Memory),
            crate::plugin::ownership::empty_test_persistence(),
        ),
        Arc::new(Discovery(manifest())),
    );
    runtime.compute_slot = Arc::new(crate::plugin::compute::slot::ComputeSlot::default());
    let runtime = Arc::new(runtime);
    let catalog = runtime.get_catalog().await.unwrap();
    let request = ComputeRequest {
        request_id: "run-1".into(),
        plugin_id: "com.example.compute".into(),
        contribution_id: "series.sma".into(),
        expected_catalog_generation: catalog.catalog_generation,
        expected_revision: catalog.revision,
        values: vec![1., 2., 3., 4., 5.],
        parameter: 3.,
    };
    (runtime, request)
}
fn code<T: std::fmt::Debug>(result: AppResult<T>) -> Value {
    serde_json::to_value(result.unwrap_err()).unwrap()["code"].clone()
}
async fn enable(runtime: &Arc<PluginRuntime>, request: &mut ComputeRequest) {
    runtime
        .set_enabled(
            &request.plugin_id,
            true,
            &request.expected_catalog_generation,
        )
        .await
        .unwrap();
    request.expected_revision = runtime.get_catalog().await.unwrap().revision;
}

#[tokio::test]
async fn authority_disabled_then_explicit_enabled_run_returns_exact_provenance() {
    let (runtime, mut request) = fixture().await;
    assert_eq!(
        code(runtime.execute_compute(request.clone()).await),
        "plugin_compute_disabled"
    );
    enable(&runtime, &mut request).await;
    let value = serde_json::to_value(runtime.execute_compute(request).await.unwrap()).unwrap();
    assert_eq!(
        value,
        json!({"schemaVersion":1,"requestId":"run-1","pluginId":"com.example.compute","contributionId":"series.sma",
        "catalogGeneration":"1","revision":"1","value":4.,"inputCount":5,"parameter":3.})
    );
}

#[tokio::test]
async fn authority_rejects_bad_context_ids_and_non_compute_targets() {
    let (runtime, mut request) = fixture().await;
    enable(&runtime, &mut request).await;
    for (field, value, expected) in [
        ("generation", "0", "stale"),
        ("revision", "0", "stale"),
        ("generation", "01", "invalid_request"),
        ("request", "bad/id", "invalid_request"),
        ("plugin", "com.missing.plugin", "not_found"),
        ("contribution", "series.missing", "not_found"),
        ("contribution", "series.info", "not_supported"),
    ] {
        let mut bad = request.clone();
        match field {
            "generation" => bad.expected_catalog_generation = value.into(),
            "revision" => bad.expected_revision = value.into(),
            "request" => bad.request_id = value.into(),
            "plugin" => bad.plugin_id = value.into(),
            _ => bad.contribution_id = value.into(),
        }
        assert_eq!(
            code(runtime.execute_compute(bad).await),
            format!("plugin_compute_{expected}")
        );
    }
    assert_eq!(
        code(runtime.cancel_compute("../bad")),
        "plugin_compute_invalid_request"
    );
    assert!(!runtime.cancel_compute("old-request").unwrap().cancelled);
}

#[test]
fn compute_request_rejects_unknown_or_duplicate_module_fields() {
    let request = r#"{"requestId":"r","pluginId":"com.example.compute","contributionId":"series.sma","expectedCatalogGeneration":"1","expectedRevision":"1","values":[1],"parameter":3}"#;
    assert!(serde_json::from_str::<ComputeRequest>(request).is_ok());
    for injected in ["\"moduleBase64\":\"evil\",", "\"requestId\":\"other\","] {
        assert!(serde_json::from_str::<ComputeRequest>(&request.replacen(
            '{',
            &format!("{{{injected}"),
            1
        ))
        .is_err());
    }
}

#[tokio::test]
async fn pending_lifecycle_prevents_execution_and_disabled_context_cannot_return_results() {
    let (runtime, mut request) = fixture().await;
    enable(&runtime, &mut request).await;
    let operation = runtime.operation_gate.lock().await;
    assert_eq!(
        code(runtime.execute_compute(request.clone()).await),
        "plugin_compute_unavailable"
    );
    drop(operation);
    runtime
        .set_enabled(
            &request.plugin_id,
            false,
            &request.expected_catalog_generation,
        )
        .await
        .unwrap();
    assert_eq!(
        code(runtime.execute_compute(request.clone()).await),
        "plugin_compute_stale"
    );
    request.expected_revision = runtime.get_catalog().await.unwrap().revision;
    assert_eq!(
        code(runtime.execute_compute(request).await),
        "plugin_compute_disabled"
    );
}

#[tokio::test]
async fn captured_identity_cannot_return_after_disable_or_content_publication() {
    let (runtime, mut request) = fixture().await;
    enable(&runtime, &mut request).await;
    let captured = runtime
        .registry
        .read()
        .await
        .capture_compute(&request)
        .unwrap();
    runtime
        .set_enabled(
            &request.plugin_id,
            false,
            &request.expected_catalog_generation,
        )
        .await
        .unwrap();
    assert_eq!(
        code(
            runtime
                .registry
                .read()
                .await
                .revalidate_compute(&request, &captured)
        ),
        "plugin_compute_stale"
    );
    enable(&runtime, &mut request).await;
    let captured = runtime
        .registry
        .read()
        .await
        .capture_compute(&request)
        .unwrap();
    let mut changed = manifest();
    changed.name = "Changed approved content".into();
    runtime
        .registry
        .write()
        .await
        .apply_local_discovery(LocalDiscoveryOutcome::available(vec![
            PluginRecord::local_declarative(changed).unwrap(),
        ]))
        .unwrap();
    assert_eq!(
        code(
            runtime
                .registry
                .read()
                .await
                .revalidate_compute(&request, &captured)
        ),
        "plugin_compute_stale"
    );
    let catalog = runtime.registry.read().await.catalog_snapshot();
    request.expected_catalog_generation = catalog.catalog_generation;
    assert_eq!(
        code(runtime.execute_compute(request).await),
        "plugin_compute_disabled"
    );
}

#[tokio::test]
async fn reload_invalidates_even_unchanged_catalog_and_busy_slot_stays_owned() {
    let (runtime, mut request) = fixture().await;
    enable(&runtime, &mut request).await;
    let lease = runtime.compute_slot.acquire("held").unwrap();
    assert_eq!(
        code(runtime.execute_compute(request.clone()).await),
        "plugin_compute_busy"
    );
    runtime.reload_catalog().await.unwrap();
    assert!(lease.cancel.load(Ordering::Acquire));
    assert_eq!(
        code(runtime.execute_compute(request.clone()).await),
        "plugin_compute_busy"
    );
    drop(lease);
    assert_eq!(runtime.execute_compute(request).await.unwrap().value, 4.);
}

#[tokio::test]
async fn completed_worker_cannot_publish_after_reload_before_ipc_resumes() {
    let (runtime, mut request) = fixture().await;
    enable(&runtime, &mut request).await;
    let mut pending = Box::pin(runtime.execute_compute(request));
    assert!(futures_util::poll!(pending.as_mut()).is_pending());
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Ok(lease) = runtime.compute_slot.acquire("probe") {
                drop(lease);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    runtime.reload_catalog().await.unwrap();
    assert_eq!(code(pending.await), "plugin_compute_cancelled");
}

#[tokio::test]
async fn unavailable_discovery_cannot_execute_cached_enabled_authority() {
    let (runtime, mut request) = fixture().await;
    enable(&runtime, &mut request).await;
    runtime
        .registry
        .write()
        .await
        .apply_local_discovery(LocalDiscoveryOutcome::unavailable())
        .unwrap();
    assert_eq!(
        code(runtime.execute_compute(request).await),
        "plugin_compute_unavailable"
    );
}

#[tokio::test]
async fn lifecycle_reserves_admission_gate_before_invalidating_compute() {
    let (runtime, mut request) = fixture().await;
    enable(&runtime, &mut request).await;
    let invalidation =
        crate::plugin::compute::slot::tests::hold_invalidation(&runtime.compute_slot);
    let previous_epoch = runtime.compute_epoch.load(Ordering::Acquire);
    let worker_runtime = runtime.clone();
    let worker = std::thread::spawn(move || worker_runtime.reserve_operation());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while runtime.compute_epoch.load(Ordering::Acquire) == previous_epoch
        && std::time::Instant::now() < deadline
    {
        std::thread::yield_now();
    }
    let invalidation_started = runtime.compute_epoch.load(Ordering::Acquire) != previous_epoch;
    // The operation has advanced its epoch and is now blocked in the real
    // invalidation mutex. Compute admission must already be impossible here.
    let admission_blocked = runtime.operation_gate.try_lock().is_err();
    drop(invalidation);
    let reservation = worker.join().unwrap();
    assert!(
        invalidation_started,
        "lifecycle worker did not reach invalidation"
    );
    assert!(
        admission_blocked,
        "compute can enter after epoch change but before lifecycle gate reservation"
    );
    drop(reservation);
}
