use std::fs;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::oneshot;

use super::super::PluginRuntime;
use crate::error::{AppError, AppResult};
use crate::plugin::discovery::{discover_from_root, LocalDiscoveryOutcome, LocalPluginDiscovery};
use crate::plugin::import::source::{LocalManifestReader, SystemLocalManifestReader};
use crate::plugin::import::{
    test_support::VALID, CommitImportResult, ImportCommitFailure, ImportPreview,
    LocalManifestSelector, PrepareImportResult, PreparedManifest, SelectedManifestSource,
};
use crate::plugin::manifest::{PluginManifestV1, PluginSource};
use crate::plugin::record::PluginRecord;
use crate::plugin::PluginRegistry;
use crate::storage::local_plugin_import::{
    LocalManifestImportStorage, OwnedImportStage, Promotion, SystemLocalManifestImportStorage,
};
use crate::storage::plugin_state::{
    PluginStateEntryV2, PluginStateFileV2, PluginStateLoad, PluginStatePersistence,
    PluginStateStore,
};

const WATCHDOG: Duration = Duration::from_secs(10);
const NEVER: usize = usize::MAX;
const CRASH_EXIT_CODE: i32 = 73;
const CRASH_ROOT_ENV: &str = "EASIFLUX_IMPORT_TEST_ROOT";
const CRASH_CHECKPOINT_ENV: &str = "EASIFLUX_IMPORT_TEST_CHECKPOINT";
const CRASH_CHILD_TEST: &str = "plugin::runtime::import::tests::import_crash_child";
const VALID_FINGERPRINT: &str =
    "v1:sha256:71bb47f19cd31329579e843b621a87085d3e1346a9e354d7f60b1393ee10deee";

type Events = Arc<Mutex<Vec<&'static str>>>;

struct FixedSelector(Option<PathBuf>);

impl LocalManifestSelector for FixedSelector {
    fn select(
        &self,
    ) -> Pin<Box<dyn Future<Output = AppResult<SelectedManifestSource>> + Send + '_>> {
        let selected = self.0.clone();
        Box::pin(async move {
            Ok(match selected {
                Some(path) => SelectedManifestSource::Selected(path),
                None => SelectedManifestSource::Cancelled,
            })
        })
    }
}

#[derive(Clone)]
struct RecordingPersistence {
    inner: Arc<PluginStateStore>,
    events: Events,
    fail_save: Arc<AtomicBool>,
    saves: Arc<AtomicUsize>,
}

impl RecordingPersistence {
    fn new(path: PathBuf, events: Events) -> Self {
        Self {
            inner: Arc::new(PluginStateStore::with_path(path)),
            events,
            fail_save: Arc::new(AtomicBool::new(false)),
            saves: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn fail_next_save(&self) {
        self.fail_save.store(true, Ordering::SeqCst);
    }
}

impl PluginStatePersistence for RecordingPersistence {
    fn load(&self) -> AppResult<PluginStateLoad> {
        self.inner.load()
    }

    fn save(&self, state: &PluginStateFileV2) -> AppResult<()> {
        self.events.lock().unwrap().push("disable");
        self.saves.fetch_add(1, Ordering::SeqCst);
        if self.fail_save.swap(false, Ordering::SeqCst) {
            return Err(AppError::Plugin {
                code: "plugin_state_persist_failed",
                message: "插件状态保存失败",
                diagnostic: None,
            });
        }
        self.inner.save(state)
    }
}

struct CrashPersistence {
    inner: PluginStateStore,
    checkpoint: String,
}

impl PluginStatePersistence for CrashPersistence {
    fn load(&self) -> AppResult<PluginStateLoad> {
        self.inner.load()
    }

    fn save(&self, state: &PluginStateFileV2) -> AppResult<()> {
        self.inner.save(state)?;
        exit_at_checkpoint(&self.checkpoint, "disabled-saved");
        Ok(())
    }
}

struct RecordingDiscovery {
    local_root: PathBuf,
    events: Events,
    calls: AtomicUsize,
    panic_on_call: AtomicUsize,
    degrade_on_call: AtomicUsize,
}

impl RecordingDiscovery {
    fn new(local_root: PathBuf, events: Events) -> Self {
        Self {
            local_root,
            events,
            calls: AtomicUsize::new(0),
            panic_on_call: AtomicUsize::new(NEVER),
            degrade_on_call: AtomicUsize::new(NEVER),
        }
    }

    fn panic_on(&self, call: usize) {
        self.panic_on_call.store(call, Ordering::SeqCst);
    }

    fn degrade_on(&self, call: usize) {
        self.degrade_on_call.store(call, Ordering::SeqCst);
    }
}

impl LocalPluginDiscovery for RecordingDiscovery {
    fn discover(&self) -> LocalDiscoveryOutcome {
        self.events.lock().unwrap().push("scan");
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.panic_on_call.load(Ordering::SeqCst) == call {
            panic!("synthetic private postscan failure");
        }
        let mut outcome = discover_from_root(&self.local_root);
        if self.degrade_on_call.load(Ordering::SeqCst) == call {
            outcome.summary = crate::plugin::manifest::LocalDiscoverySummary::degraded(1).unwrap();
        }
        outcome
    }
}

struct BlockingPoint {
    started: Mutex<Option<oneshot::Sender<()>>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl BlockingPoint {
    fn reach(&self) {
        if let Some(started) = self.started.lock().unwrap().take() {
            let _ = started.send(());
        }
        self.release
            .lock()
            .unwrap()
            .recv_timeout(WATCHDOG)
            .expect("blocked promotion must be released");
    }
}

struct BlockingControl {
    started: oneshot::Receiver<()>,
    release: mpsc::Sender<()>,
}

impl BlockingControl {
    async fn wait_started(&mut self) {
        tokio::time::timeout(WATCHDOG, &mut self.started)
            .await
            .expect("promotion must start")
            .unwrap();
    }

    fn release(&self) {
        self.release.send(()).unwrap();
    }
}

#[derive(Default)]
struct StorageControls {
    fail_prepare: AtomicBool,
    fail_promote: AtomicBool,
    unverified_promotion: AtomicBool,
    panic_after_promotion: AtomicBool,
    promote_calls: AtomicUsize,
    promotion_block: Mutex<Option<Arc<BlockingPoint>>>,
}

impl StorageControls {
    fn fail_next_prepare(&self) {
        self.fail_prepare.store(true, Ordering::SeqCst);
    }

    fn fail_next_promotion(&self) {
        self.fail_promote.store(true, Ordering::SeqCst);
    }

    fn make_next_promotion_unverified(&self) {
        self.unverified_promotion.store(true, Ordering::SeqCst);
    }

    fn panic_after_next_promotion(&self) {
        self.panic_after_promotion.store(true, Ordering::SeqCst);
    }

    fn block_next_promotion(&self) -> BlockingControl {
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let point = Arc::new(BlockingPoint {
            started: Mutex::new(Some(started_tx)),
            release: Mutex::new(release_rx),
        });
        assert!(self
            .promotion_block
            .lock()
            .unwrap()
            .replace(point)
            .is_none());
        BlockingControl {
            started: started_rx,
            release: release_tx,
        }
    }
}

struct RecordingStorage {
    inner: SystemLocalManifestImportStorage,
    events: Events,
    controls: Arc<StorageControls>,
}

impl LocalManifestImportStorage for RecordingStorage {
    fn prepare_stage(
        &self,
        bytes: &[u8],
    ) -> Result<Box<dyn OwnedImportStage>, ImportCommitFailure> {
        self.events.lock().unwrap().push("stage");
        if self.controls.fail_prepare.swap(false, Ordering::SeqCst) {
            return Err(ImportCommitFailure::WriteFailed);
        }
        let inner = self.inner.prepare_stage(bytes)?;
        Ok(Box::new(RecordingStage {
            inner,
            events: Arc::clone(&self.events),
            controls: Arc::clone(&self.controls),
        }))
    }
}

struct RecordingStage {
    inner: Box<dyn OwnedImportStage>,
    events: Events,
    controls: Arc<StorageControls>,
}

impl OwnedImportStage for RecordingStage {
    fn promote(&mut self) -> Result<Promotion, ImportCommitFailure> {
        self.events.lock().unwrap().push("promote");
        self.controls.promote_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(block) = self.controls.promotion_block.lock().unwrap().take() {
            block.reach();
        }
        if self.controls.fail_promote.swap(false, Ordering::SeqCst) {
            return Err(ImportCommitFailure::WriteFailed);
        }
        let mut promotion = self.inner.promote()?;
        if self
            .controls
            .panic_after_promotion
            .swap(false, Ordering::SeqCst)
        {
            panic!("synthetic private post-promotion failure");
        }
        if self
            .controls
            .unverified_promotion
            .swap(false, Ordering::SeqCst)
        {
            promotion.object_identity_verified = false;
        }
        Ok(promotion)
    }

    fn cleanup(&mut self) -> Result<(), ImportCommitFailure> {
        self.events.lock().unwrap().push("cleanup");
        self.inner.cleanup()
    }
}

struct CrashStorage {
    inner: SystemLocalManifestImportStorage,
    checkpoint: String,
}

impl LocalManifestImportStorage for CrashStorage {
    fn prepare_stage(
        &self,
        bytes: &[u8],
    ) -> Result<Box<dyn OwnedImportStage>, ImportCommitFailure> {
        let inner = self.inner.prepare_stage(bytes)?;
        exit_at_checkpoint(&self.checkpoint, "stage-ready");
        Ok(Box::new(CrashStage {
            inner,
            checkpoint: self.checkpoint.clone(),
        }))
    }
}

struct CrashStage {
    inner: Box<dyn OwnedImportStage>,
    checkpoint: String,
}

impl OwnedImportStage for CrashStage {
    fn promote(&mut self) -> Result<Promotion, ImportCommitFailure> {
        let promotion = self.inner.promote()?;
        exit_at_checkpoint(&self.checkpoint, "promoted");
        Ok(promotion)
    }

    fn cleanup(&mut self) -> Result<(), ImportCommitFailure> {
        self.inner.cleanup()
    }
}

fn exit_at_checkpoint(configured: &str, actual: &str) {
    if configured == actual {
        std::process::exit(CRASH_EXIT_CODE);
    }
}

struct ImportFixture {
    runtime: Arc<PluginRuntime>,
    events: Events,
    _temp: TempDir,
    source: PathBuf,
    plugins_root: PathBuf,
    local_root: PathBuf,
    discovery: Arc<RecordingDiscovery>,
    persistence: RecordingPersistence,
    storage_controls: Arc<StorageControls>,
}

impl ImportFixture {
    async fn new() -> Self {
        Self::with_existing_manifest(None).await
    }

    async fn with_real_disk() -> Self {
        Self::with_existing_manifest(None).await
    }

    async fn with_existing_manifest(manifest: Option<&[u8]>) -> Self {
        let fixture_base = std::env::current_dir().unwrap().join("target");
        fs::create_dir_all(&fixture_base).unwrap();
        let temp = tempfile::Builder::new()
            .prefix("plugin-runtime-import-")
            .tempdir_in(fixture_base.canonicalize().unwrap())
            .unwrap();
        let root = temp.path().canonicalize().unwrap();
        let source = root.join("selected.json");
        fs::write(&source, VALID).unwrap();
        let plugins_root = root.join("plugins");
        let local_root = plugins_root.join("local");
        fs::create_dir_all(&local_root).unwrap();
        if let Some(bytes) = manifest {
            write_package(&local_root, "pkg-00000000000000000000000000000001", bytes);
        }
        let events = Arc::new(Mutex::new(Vec::new()));
        let persistence =
            RecordingPersistence::new(plugins_root.join("state.json"), Arc::clone(&events));
        let discovery = Arc::new(RecordingDiscovery::new(
            local_root.clone(),
            Arc::clone(&events),
        ));
        let storage_controls = Arc::new(StorageControls::default());
        let storage = Arc::new(RecordingStorage {
            inner: SystemLocalManifestImportStorage::with_plugins_root(plugins_root.clone()),
            events: Arc::clone(&events),
            controls: Arc::clone(&storage_controls),
        });
        let registry = PluginRegistry::initialize(
            vec![manifest_from_bytes(builtin_manifest())],
            Box::new(persistence.clone()),
        );
        let runtime = Arc::new(PluginRuntime::with_import_services(
            registry,
            discovery.clone(),
            Arc::new(SystemLocalManifestReader),
            storage,
        ));
        runtime.get_catalog().await.unwrap();
        Self {
            runtime,
            events,
            _temp: temp,
            source,
            plugins_root,
            local_root,
            discovery,
            persistence,
            storage_controls,
        }
    }

    async fn prepare_valid(&self) -> ImportPreview {
        match self
            .runtime
            .prepare_import(Arc::new(FixedSelector(Some(self.source.clone()))))
            .await
            .unwrap()
        {
            PrepareImportResult::Ready(preview) => preview,
            PrepareImportResult::Cancelled(_) => panic!("ready preview required"),
        }
    }

    fn state_path(&self) -> PathBuf {
        self.plugins_root.join("state.json")
    }

    fn seed_enabled_orphan(&self) -> io::Result<()> {
        let builtin = PluginRecord::built_in(manifest_from_bytes(builtin_manifest()))
            .unwrap()
            .identity();
        let local = PreparedManifest::parse(VALID).unwrap().record().identity();
        assert_eq!(local.approval_fingerprint, VALID_FINGERPRINT);
        let state = PluginStateFileV2 {
            schema_version: 2,
            revision: 7,
            entries: vec![
                PluginStateEntryV2 {
                    id: builtin.id,
                    source: builtin.source,
                    publisher_id: builtin.publisher_id,
                    approval_fingerprint: builtin.approval_fingerprint,
                    enabled: true,
                },
                PluginStateEntryV2 {
                    id: local.id,
                    source: local.source,
                    publisher_id: local.publisher_id,
                    approval_fingerprint: VALID_FINGERPRINT.to_owned(),
                    enabled: true,
                },
            ],
        };
        fs::create_dir_all(&self.plugins_root)?;
        fs::write(self.state_path(), serde_json::to_vec(&state).unwrap())
    }

    fn corrupt_primary(&self) -> io::Result<()> {
        fs::write(self.state_path(), b"broken")
    }

    fn restart_runtime(&self) -> Arc<PluginRuntime> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let registry = PluginRegistry::initialize(
            vec![manifest_from_bytes(builtin_manifest())],
            Box::new(PluginStateStore::with_path(self.state_path())),
        );
        Arc::new(PluginRuntime::with_import_services(
            registry,
            Arc::new(RecordingDiscovery::new(self.local_root.clone(), events)),
            Arc::new(SystemLocalManifestReader),
            Arc::new(SystemLocalManifestImportStorage::with_plugins_root(
                self.plugins_root.clone(),
            )),
        ))
    }

    fn root(&self) -> &Path {
        self.source.parent().unwrap()
    }

    fn clear_events(&self) {
        self.events.lock().unwrap().clear();
    }

    fn target_manifests(&self) -> Vec<Vec<u8>> {
        let Ok(entries) = fs::read_dir(&self.local_root) else {
            return Vec::new();
        };
        let mut manifests = entries
            .filter_map(Result::ok)
            .filter_map(|entry| fs::read(entry.path().join("manifest.json")).ok())
            .collect::<Vec<_>>();
        manifests.sort();
        manifests
    }
}

fn crash_runtime(root: &Path, checkpoint: &str) -> Arc<PluginRuntime> {
    let plugins_root = root.join("plugins");
    let local_root = plugins_root.join("local");
    let events = Arc::new(Mutex::new(Vec::new()));
    let registry = PluginRegistry::initialize(
        vec![manifest_from_bytes(builtin_manifest())],
        Box::new(CrashPersistence {
            inner: PluginStateStore::with_path(plugins_root.join("state.json")),
            checkpoint: checkpoint.to_owned(),
        }),
    );
    Arc::new(PluginRuntime::with_import_services(
        registry,
        Arc::new(RecordingDiscovery::new(local_root, events)),
        Arc::new(SystemLocalManifestReader),
        Arc::new(CrashStorage {
            inner: SystemLocalManifestImportStorage::with_plugins_root(plugins_root),
            checkpoint: checkpoint.to_owned(),
        }),
    ))
}

fn run_crash_child(root: &Path, checkpoint: &str) -> ExitStatus {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", CRASH_CHILD_TEST, "--nocapture"])
        .env_clear()
        .env(CRASH_ROOT_ENV, root)
        .env(CRASH_CHECKPOINT_ENV, checkpoint)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + WATCHDOG;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("import crash child timed out at {checkpoint}");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn persisted_enabled(plugins_root: &Path, id: &str, source: PluginSource) -> bool {
    let state = PluginStateStore::with_path(plugins_root.join("state.json"))
        .load()
        .unwrap()
        .state;
    state_enabled(&state, id, source)
}

fn state_enabled(state: &PluginStateFileV2, id: &str, source: PluginSource) -> bool {
    state
        .entries
        .iter()
        .find(|entry| entry.id.as_str() == id && entry.source == source)
        .expect("persisted plugin decision")
        .enabled
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    value.into()
}

fn direct_entry_count(path: &Path) -> usize {
    fs::read_dir(path).unwrap().count()
}

fn assert_abandoned_stage_fills_capacity(plugins_root: &Path) {
    let staging = plugins_root.join("import-staging");
    assert_eq!(direct_entry_count(&staging), 1);
    let abandoned_stage = fs::read_dir(&staging)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    for index in 0..15 {
        fs::write(staging.join(format!("unknown-{index}")), b"preserve").unwrap();
    }
    let mut before = fs::read_dir(&staging)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    before.sort();
    let storage = SystemLocalManifestImportStorage::with_plugins_root(plugins_root.to_owned());
    assert_eq!(
        storage.prepare_stage(VALID).err(),
        Some(ImportCommitFailure::StagingCapacityExceeded)
    );
    let mut after = fs::read_dir(&staging)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    after.sort();
    assert_eq!(after, before);
    assert!(abandoned_stage.is_dir());
    for index in 0..15 {
        assert_eq!(
            fs::read(staging.join(format!("unknown-{index}"))).unwrap(),
            b"preserve"
        );
    }
}

fn builtin_manifest() -> &'static [u8] {
    br#"{"schemaVersion":1,"id":"com.example.builtin","publisherId":"com.example","publisher":"Example","name":"Builtin","description":"Metadata only","version":"1.0.0","contributions":[],"requestedCapabilities":[]}"#
}

fn unrelated_manifest() -> &'static [u8] {
    br#"{"schemaVersion":1,"id":"com.example.unrelated","publisherId":"com.example","publisher":"Example","name":"Unrelated","description":"Metadata only","version":"1.0.0","contributions":[],"requestedCapabilities":[]}"#
}

fn changed_manifest() -> &'static [u8] {
    br#"{"schemaVersion":1,"id":"com.example.changed","publisherId":"com.example","publisher":"Example","name":"Changed","description":"Metadata only","version":"2.0.0","contributions":[],"requestedCapabilities":[]}"#
}

fn manifest_from_bytes(bytes: &[u8]) -> PluginManifestV1 {
    serde_json::from_slice(bytes).unwrap()
}

fn write_package(local_root: &Path, name: &str, bytes: &[u8]) {
    let package = local_root.join(name);
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("manifest.json"), bytes).unwrap();
}

fn wire(result: CommitImportResult) -> Value {
    serde_json::to_value(result).unwrap()
}

fn error_code(error: AppError) -> Value {
    serde_json::to_value(error).unwrap()["code"].clone()
}

fn expect_commit_error(result: AppResult<CommitImportResult>) -> AppError {
    match result {
        Err(error) => error,
        Ok(_) => panic!("commit error required"),
    }
}

fn imported_item<'a>(snapshot: &'a Value, id: &str) -> &'a Value {
    snapshot["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["manifest"]["id"] == id)
        .expect("catalog item")
}

#[tokio::test]
async fn imported_package_stays_disabled_after_primary_corruption_and_backup_recovery() {
    let mut fixture = ImportFixture::with_real_disk().await;
    fixture.seed_enabled_orphan().unwrap();
    fixture.runtime = fixture.restart_runtime();

    let preview = fixture.prepare_valid().await;
    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );
    assert_eq!(result["status"], "imported");

    let backup: PluginStateFileV2 =
        serde_json::from_slice(&fs::read(sidecar(&fixture.state_path(), ".bak")).unwrap()).unwrap();
    assert!(state_enabled(
        &backup,
        "com.example.notes",
        PluginSource::LocalDeclarative
    ));
    assert!(state_enabled(
        &backup,
        "com.example.builtin",
        PluginSource::BuiltIn
    ));

    fixture.corrupt_primary().unwrap();
    let restarted = fixture.restart_runtime();
    let snapshot = serde_json::to_value(restarted.get_catalog().await.unwrap()).unwrap();

    let local = imported_item(&snapshot, "com.example.notes");
    assert_eq!(local["source"], "localDeclarative");
    assert_eq!(local["status"], "disabled");
    let builtin = imported_item(&snapshot, "com.example.builtin");
    assert_eq!(builtin["source"], "builtIn");
    assert_eq!(builtin["status"], "enabled");
}

#[tokio::test]
async fn process_crash_checkpoints_reconstruct_only_committed_disk_state() {
    for checkpoint in ["stage-ready", "disabled-saved", "promoted", "published"] {
        let mut fixture = ImportFixture::with_real_disk().await;
        fixture.seed_enabled_orphan().unwrap();
        fixture.runtime = fixture.restart_runtime();
        let old_preview = fixture.prepare_valid().await;
        let root = fixture.root().to_owned();
        let owned_checkpoint = checkpoint.to_owned();

        let status = tokio::task::spawn_blocking(move || run_crash_child(&root, &owned_checkpoint))
            .await
            .unwrap();
        assert_eq!(
            status.code(),
            Some(CRASH_EXIT_CODE),
            "checkpoint {checkpoint}"
        );

        if matches!(checkpoint, "promoted" | "published") {
            fixture.corrupt_primary().unwrap();
        }
        fixture.runtime = fixture.restart_runtime();
        let before_targets = fixture.target_manifests();
        let before_stages = direct_entry_count(&fixture.plugins_root.join("import-staging"));
        let replay = expect_commit_error(
            fixture
                .runtime
                .commit_import(&old_preview.token, &old_preview.catalog_generation)
                .await,
        );
        assert_eq!(error_code(replay), "plugin_import_token_invalid");
        assert_eq!(fixture.target_manifests(), before_targets);
        assert_eq!(
            direct_entry_count(&fixture.plugins_root.join("import-staging")),
            before_stages
        );

        let snapshot = serde_json::to_value(fixture.runtime.get_catalog().await.unwrap()).unwrap();
        let builtin = imported_item(&snapshot, "com.example.builtin");
        assert_eq!(builtin["source"], "builtIn");
        assert_eq!(builtin["status"], "enabled");

        if matches!(checkpoint, "stage-ready" | "disabled-saved") {
            assert!(snapshot["plugins"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["manifest"]["id"] != "com.example.notes"));
            assert!(fixture.target_manifests().is_empty());
            assert_eq!(
                persisted_enabled(
                    &fixture.plugins_root,
                    "com.example.notes",
                    PluginSource::LocalDeclarative
                ),
                checkpoint == "stage-ready"
            );
            assert_abandoned_stage_fills_capacity(&fixture.plugins_root);
        } else {
            let local = imported_item(&snapshot, "com.example.notes");
            assert_eq!(local["source"], "localDeclarative");
            assert_eq!(local["status"], "disabled");
            assert_eq!(fixture.target_manifests().len(), 1);
            assert!(!persisted_enabled(
                &fixture.plugins_root,
                "com.example.notes",
                PluginSource::LocalDeclarative
            ));
            assert_eq!(
                direct_entry_count(&fixture.plugins_root.join("import-staging")),
                0
            );
        }
    }
}

#[tokio::test]
#[ignore = "launched only by process_crash_checkpoints_reconstruct_only_committed_disk_state"]
async fn import_crash_child() {
    let Some(root) = std::env::var_os(CRASH_ROOT_ENV).map(PathBuf::from) else {
        return;
    };
    let Some(checkpoint) =
        std::env::var_os(CRASH_CHECKPOINT_ENV).and_then(|value| value.into_string().ok())
    else {
        return;
    };
    assert!(matches!(
        checkpoint.as_str(),
        "stage-ready" | "disabled-saved" | "promoted" | "published"
    ));
    let runtime = crash_runtime(&root, &checkpoint);
    let preview = match runtime
        .prepare_import(Arc::new(FixedSelector(Some(root.join("selected.json")))))
        .await
        .unwrap()
    {
        PrepareImportResult::Ready(preview) => preview,
        PrepareImportResult::Cancelled(_) => panic!("ready preview required"),
    };
    let result = runtime
        .commit_import(&preview.token, &preview.catalog_generation)
        .await
        .unwrap();
    assert_eq!(wire(result)["status"], "imported");
    exit_at_checkpoint(&checkpoint, "published");
    panic!("checkpoint {checkpoint} was not reached");
}

#[test]
fn runtime_and_import_services_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PluginRuntime>();
    assert_send_sync::<SystemLocalManifestReader>();
    assert_send_sync::<SystemLocalManifestImportStorage>();
    assert_send_sync::<Arc<dyn LocalPluginDiscovery>>();
    assert_send_sync::<Arc<dyn LocalManifestReader>>();
    assert_send_sync::<Arc<dyn LocalManifestImportStorage>>();
}

#[tokio::test]
async fn owned_runtime_import_future_can_be_spawned() {
    let fixture = ImportFixture::new().await;
    let runtime = Arc::clone(&fixture.runtime);
    let source = fixture.source.clone();
    let result = tokio::spawn(async move {
        let preview = match runtime
            .prepare_import(Arc::new(FixedSelector(Some(source))))
            .await?
        {
            PrepareImportResult::Ready(preview) => preview,
            PrepareImportResult::Cancelled(_) => panic!("ready preview required"),
        };
        runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(wire(result)["status"], "imported");
}

#[tokio::test]
async fn prepare_cancellation_has_no_filesystem_writes() {
    let fixture = ImportFixture::new().await;
    fixture.clear_events();
    let result = fixture
        .runtime
        .prepare_import(Arc::new(FixedSelector(None)))
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(result).unwrap()["status"], "cancelled");
    assert!(fixture.events.lock().unwrap().is_empty());
    assert!(!fixture.plugins_root.join("import-staging").exists());
    assert_eq!(fixture.persistence.saves.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cancelling_ready_is_idempotent_without_gate_or_storage() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    fixture.clear_events();
    let held_gate = fixture.runtime.operation_gate.lock().await;
    let first = fixture.runtime.cancel_import(&preview.token).unwrap();
    let second = fixture.runtime.cancel_import(&preview.token).unwrap();
    assert_eq!(serde_json::to_value(first).unwrap()["status"], "cancelled");
    assert_eq!(serde_json::to_value(second).unwrap()["status"], "cancelled");
    assert!(fixture.events.lock().unwrap().is_empty());
    assert_eq!(fixture.persistence.saves.load(Ordering::SeqCst), 0);
    drop(held_gate);
    assert!(matches!(
        fixture
            .runtime
            .prepare_import(Arc::new(FixedSelector(None)))
            .await
            .unwrap(),
        PrepareImportResult::Cancelled(_)
    ));
}

#[tokio::test]
async fn commit_orders_disable_before_promotion_and_publishes_disabled_content() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    fixture.clear_events();
    let result = fixture
        .runtime
        .commit_import(&preview.token, &preview.catalog_generation)
        .await
        .unwrap();
    let result = wire(result);
    assert_eq!(result["status"], "imported");
    assert_eq!(
        imported_item(&result["snapshot"], "com.example.notes")["status"],
        "disabled"
    );
    assert_eq!(
        *fixture.events.lock().unwrap(),
        ["scan", "stage", "disable", "promote", "scan"]
    );
}

#[tokio::test]
async fn source_replaced_after_preview_does_not_change_import() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    fs::write(&fixture.source, changed_manifest()).unwrap();
    fixture.clear_events();
    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );
    assert_eq!(result["status"], "imported");
    assert_eq!(result["pluginId"], "com.example.notes");
    assert_eq!(
        fixture.target_manifests(),
        vec![PreparedManifest::parse(VALID).unwrap().bytes().to_vec()]
    );
}

#[tokio::test]
async fn stale_confirmation_is_consumed_without_stage() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    write_package(
        &fixture.local_root,
        "pkg-00000000000000000000000000000002",
        unrelated_manifest(),
    );
    assert_eq!(
        fixture
            .runtime
            .reload_catalog()
            .await
            .unwrap()
            .catalog_generation,
        "2"
    );
    fixture.clear_events();
    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );
    assert_eq!(result["status"], "notImported");
    assert_eq!(result["reasonCode"], "plugin_catalog_stale");
    assert_eq!(result["disabledDecisionSaved"], false);
    assert!(fixture.events.lock().unwrap().is_empty());
    let replay = expect_commit_error(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await,
    );
    assert_eq!(error_code(replay), "plugin_import_token_invalid");
}

#[tokio::test]
async fn preflight_change_is_published_then_rejected() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    write_package(
        &fixture.local_root,
        "pkg-00000000000000000000000000000002",
        unrelated_manifest(),
    );
    fixture.clear_events();
    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );
    assert_eq!(result["status"], "notImported");
    assert_eq!(result["reasonCode"], "plugin_catalog_stale");
    assert_eq!(result["snapshot"]["catalogGeneration"], "2");
    assert_eq!(
        imported_item(&result["snapshot"], "com.example.unrelated")["status"],
        "disabled"
    );
    assert_eq!(*fixture.events.lock().unwrap(), ["scan"]);
}

#[tokio::test]
async fn duplicate_identity_is_never_overwritten() {
    let fixture = ImportFixture::with_existing_manifest(Some(VALID)).await;
    let before = fixture.target_manifests();
    let preview = fixture.prepare_valid().await;
    fixture.clear_events();
    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );
    assert_eq!(result["status"], "notImported");
    assert_eq!(result["reasonCode"], "plugin_import_id_conflict");
    assert_eq!(result["disabledDecisionSaved"], false);
    assert_eq!(fixture.target_manifests(), before);
    assert_eq!(*fixture.events.lock().unwrap(), ["scan"]);
}

#[tokio::test]
async fn state_failure_prevents_promotion() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    fixture.persistence.fail_next_save();
    fixture.clear_events();
    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );
    assert_eq!(result["status"], "notImported");
    assert_eq!(result["reasonCode"], "plugin_state_persist_failed");
    assert_eq!(result["disabledDecisionSaved"], false);
    assert!(fixture.target_manifests().is_empty());
    assert_eq!(
        *fixture.events.lock().unwrap(),
        ["scan", "stage", "disable", "cleanup"]
    );
}

#[tokio::test]
async fn stage_preparation_failure_never_saves_state_or_promotes() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    fixture.storage_controls.fail_next_prepare();
    fixture.clear_events();
    let before_saves = fixture.persistence.saves.load(Ordering::SeqCst);

    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );

    assert_eq!(result["status"], "notImported");
    assert_eq!(result["reasonCode"], "plugin_import_write_failed");
    assert_eq!(result["disabledDecisionSaved"], false);
    assert_eq!(
        fixture.persistence.saves.load(Ordering::SeqCst),
        before_saves
    );
    assert_eq!(
        fixture
            .storage_controls
            .promote_calls
            .load(Ordering::SeqCst),
        0
    );
    assert_eq!(*fixture.events.lock().unwrap(), ["scan", "stage"]);
}

#[cfg(unix)]
#[tokio::test]
async fn staging_parent_sync_failure_never_saves_state() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    fixture.storage_controls.fail_next_prepare();
    fixture.clear_events();
    let before = fixture.persistence.saves.load(Ordering::SeqCst);
    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );
    assert_eq!(result["status"], "notImported");
    assert_eq!(result["reasonCode"], "plugin_import_write_failed");
    assert_eq!(result["disabledDecisionSaved"], false);
    assert_eq!(fixture.persistence.saves.load(Ordering::SeqCst), before);
    assert_eq!(*fixture.events.lock().unwrap(), ["scan", "stage"]);
}

#[tokio::test]
async fn promotion_failure_reports_disabled_saved() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    fixture.storage_controls.fail_next_promotion();
    fixture.clear_events();
    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );
    assert_eq!(result["status"], "notImported");
    assert_eq!(result["reasonCode"], "plugin_import_write_failed");
    assert_eq!(result["disabledDecisionSaved"], true);
    assert!(fixture.target_manifests().is_empty());
    assert_eq!(
        *fixture.events.lock().unwrap(),
        ["scan", "stage", "disable", "promote", "cleanup"]
    );
}

#[tokio::test]
async fn postscan_failure_reports_imported_not_visible() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    fixture.discovery.panic_on(3);
    fixture.clear_events();
    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );
    assert_eq!(result["status"], "importedNotVisible");
    assert_eq!(result["pluginId"], "com.example.notes");
    assert_eq!(
        result["reasonCode"],
        "plugin_import_publication_unconfirmed"
    );
    assert_eq!(
        result["snapshot"]["localDiscovery"]["status"],
        "unavailable"
    );
    assert_eq!(fixture.target_manifests().len(), 1);
}

#[tokio::test]
async fn unverified_promoted_identity_is_imported_not_visible() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    fixture.storage_controls.make_next_promotion_unverified();
    fixture.clear_events();
    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );
    assert_eq!(result["status"], "importedNotVisible");
    assert_eq!(
        imported_item(&result["snapshot"], "com.example.notes")["status"],
        "disabled"
    );
    assert_eq!(fixture.target_manifests().len(), 1);
}

#[tokio::test]
async fn promotion_worker_panic_is_outcome_unknown_without_cleanup() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    fixture.storage_controls.panic_after_next_promotion();
    fixture.clear_events();
    let error = expect_commit_error(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await,
    );
    let serialized = serde_json::to_value(error).unwrap();
    assert_eq!(serialized, json!("内部错误: 插件目录请求暂不可用"));
    assert!(!serialized.to_string().contains("private"));
    assert_eq!(fixture.target_manifests().len(), 1);
    assert!(!fixture.events.lock().unwrap().contains(&"cleanup"));
}

#[tokio::test]
async fn unrelated_degraded_postscan_still_allows_exact_imported_item() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    fixture.discovery.degrade_on(3);
    fixture.clear_events();
    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );
    assert_eq!(result["status"], "imported");
    assert_eq!(result["snapshot"]["localDiscovery"]["status"], "degraded");
    assert_eq!(
        imported_item(&result["snapshot"], "com.example.notes")["status"],
        "disabled"
    );
}

#[tokio::test]
async fn lost_commit_caller_still_finishes_once() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    let mut promotion = fixture.storage_controls.block_next_promotion();
    fixture.clear_events();
    let mut commit = Box::pin(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation),
    );
    assert!(futures_util::poll!(&mut commit).is_pending());
    promotion.wait_started().await;
    drop(commit);
    promotion.release();
    let finished = tokio::time::timeout(WATCHDOG, fixture.runtime.operation_gate.lock())
        .await
        .expect("owned commit must release the operation gate");
    drop(finished);
    assert_eq!(
        fixture
            .storage_controls
            .promote_calls
            .load(Ordering::SeqCst),
        1
    );
    assert_eq!(fixture.target_manifests().len(), 1);
    let snapshot = serde_json::to_value(fixture.runtime.get_catalog().await.unwrap()).unwrap();
    assert_eq!(
        imported_item(&snapshot, "com.example.notes")["status"],
        "disabled"
    );
}

#[tokio::test]
async fn expired_or_replayed_token_never_writes() {
    let expired = ImportFixture::new().await;
    let past = Instant::now() - Duration::from_secs(301);
    let generation = expired.runtime.registry.read().await.catalog_generation();
    let preview = expired
        .runtime
        .import_sessions
        .reserve_prepare(past)
        .unwrap()
        .publish(PreparedManifest::parse(VALID).unwrap(), generation, past)
        .unwrap();
    expired.clear_events();
    let error = expect_commit_error(
        expired
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await,
    );
    assert_eq!(error_code(error), "plugin_import_token_invalid");
    assert!(expired.events.lock().unwrap().is_empty());

    let replayed = ImportFixture::new().await;
    let preview = replayed.prepare_valid().await;
    replayed
        .runtime
        .commit_import(&preview.token, &preview.catalog_generation)
        .await
        .unwrap();
    replayed.clear_events();
    let error = expect_commit_error(
        replayed
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await,
    );
    assert_eq!(error_code(error), "plugin_import_token_invalid");
    assert!(replayed.events.lock().unwrap().is_empty());
    assert_eq!(
        replayed
            .storage_controls
            .promote_calls
            .load(Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn toggle_cannot_enter_disable_promotion_window() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    let mut promotion = fixture.storage_controls.block_next_promotion();
    fixture.clear_events();
    let commit = tokio::spawn({
        let runtime = Arc::clone(&fixture.runtime);
        let token = preview.token.clone();
        let generation = preview.catalog_generation.clone();
        async move { runtime.commit_import(&token, &generation).await }
    });
    promotion.wait_started().await;
    assert_eq!(fixture.persistence.saves.load(Ordering::SeqCst), 1);
    let toggle =
        fixture
            .runtime
            .set_enabled("com.example.builtin", true, &preview.catalog_generation);
    tokio::pin!(toggle);
    assert!(futures_util::poll!(&mut toggle).is_pending());
    assert_eq!(fixture.persistence.saves.load(Ordering::SeqCst), 1);
    promotion.release();
    assert_eq!(wire(commit.await.unwrap().unwrap())["status"], "imported");
    assert_eq!(
        error_code(toggle.await.unwrap_err()),
        "plugin_catalog_stale"
    );
    assert_eq!(fixture.persistence.saves.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn generation_max_rejects_before_stage() {
    let fixture = ImportFixture::new().await;
    fixture
        .runtime
        .registry
        .write()
        .await
        .set_catalog_generation_for_test(u64::MAX);
    let preview = fixture.prepare_valid().await;
    assert_eq!(preview.catalog_generation, u64::MAX.to_string());
    fixture.clear_events();
    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );
    assert_eq!(result["status"], "notImported");
    assert_eq!(result["reasonCode"], "plugin_catalog_generation_exhausted");
    assert_eq!(result["disabledDecisionSaved"], false);
    assert_eq!(*fixture.events.lock().unwrap(), ["scan"]);
}

#[tokio::test]
async fn generation_max_with_changed_prescan_returns_structured_failure_before_writes() {
    let fixture = ImportFixture::new().await;
    fixture
        .runtime
        .registry
        .write()
        .await
        .set_catalog_generation_for_test(u64::MAX);
    let preview = fixture.prepare_valid().await;
    write_package(
        &fixture.local_root,
        "pkg-00000000000000000000000000000002",
        unrelated_manifest(),
    );
    fixture.clear_events();

    let result = wire(
        fixture
            .runtime
            .commit_import(&preview.token, &preview.catalog_generation)
            .await
            .unwrap(),
    );

    assert_eq!(result["status"], "notImported");
    assert_eq!(result["reasonCode"], "plugin_catalog_generation_exhausted");
    assert_eq!(result["disabledDecisionSaved"], false);
    assert_eq!(
        result["snapshot"]["catalogGeneration"],
        u64::MAX.to_string()
    );
    assert_eq!(result["snapshot"]["plugins"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["snapshot"]["plugins"][0]["manifest"]["id"],
        "com.example.builtin"
    );
    assert_eq!(*fixture.events.lock().unwrap(), ["scan"]);
    assert_eq!(fixture.persistence.saves.load(Ordering::SeqCst), 0);
}
