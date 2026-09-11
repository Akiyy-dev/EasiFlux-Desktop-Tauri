use std::{io, path::PathBuf};

use crate::{
    models::config::APP_NAME,
    plugin::{
        discovery::LocalPackageLocator,
        ownership::{FileIdentity, ManagedOwnershipEntryV1, RemovalSlot},
    },
};

pub(crate) mod platform;

/// Shared fixed-root configuration. Root resolution is deferred until an operation.
pub(crate) struct SystemLocalPluginPackageStorage {
    plugins_root: Option<PathBuf>,
    pub(super) hooks: Hooks,
}

impl SystemLocalPluginPackageStorage {
    pub(crate) fn new() -> Self {
        Self {
            plugins_root: None,
            hooks: Hooks::default(),
        }
    }
    pub(crate) fn with_plugins_root(root: PathBuf) -> Self {
        Self {
            plugins_root: Some(root),
            hooks: Hooks::default(),
        }
    }
    pub(crate) fn open(&self) -> io::Result<platform::ImportDirectories> {
        let root = self
            .plugins_root
            .clone()
            .or_else(|| dirs::config_dir().map(|root| root.join(APP_NAME).join("plugins")))
            .ok_or(io::ErrorKind::NotFound)?;
        let mut parents = platform::ImportDirectories::open_or_create(&root)?;
        parents.hooks = self.hooks.clone();
        Ok(parents)
    }
}

#[derive(Debug)]
pub(crate) struct PromotedManagedPackage {
    pub(crate) entry: ManagedOwnershipEntryV1,
    pub(crate) locator: LocalPackageLocator,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemovalStorageFailure {
    Unavailable,
    IdentityChanged,
    StagingCapacityExceeded,
    ProvenWriteFailure,
}

pub(crate) trait ManagedLocalPluginRemovalStorage: Send + Sync {
    fn prepare(
        &self,
        locator: &LocalPackageLocator,
        entry: &ManagedOwnershipEntryV1,
    ) -> Result<Box<dyn OwnedRemoval>, RemovalStorageFailure>;
}

pub(crate) trait OwnedRemoval: Send {
    fn removal_slot(&self) -> &RemovalSlot;
    fn reverify_before_rename(&self) -> Result<(), RemovalStorageFailure>;
    fn quarantine_once(&mut self) -> QuarantineRenameOutcome;
    /// Fresh read-only evidence. Never grants cleanup permission or changes state.
    fn verify_quarantine(&self) -> Result<VerifiedQuarantine, RemovalStorageFailure>;
    fn cleanup_once(&mut self) -> CleanupOutcome;
}

#[derive(Debug)]
pub(crate) struct VerifiedQuarantine {
    pub(crate) entry: Box<ManagedOwnershipEntryV1>,
    pub(crate) removal_slot: RemovalSlot,
}

#[derive(Debug)]
pub(crate) enum QuarantineRenameOutcome {
    Committed(VerifiedQuarantine),
    ProvenNotCommitted(RemovalStorageFailure),
    CommitUnconfirmed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum KnownCleanupShape {
    Full,
    ReceiptOnly,
    EmptyDirectory,
    BothAbsent,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CleanupOutcome {
    Removed,
    Pending(KnownCleanupShape),
    Conflict,
}

impl ManagedLocalPluginRemovalStorage for SystemLocalPluginPackageStorage {
    fn prepare(
        &self,
        locator: &LocalPackageLocator,
        entry: &ManagedOwnershipEntryV1,
    ) -> Result<Box<dyn OwnedRemoval>, RemovalStorageFailure> {
        let parents = self
            .open()
            .map_err(|_| RemovalStorageFailure::Unavailable)?;
        Ok(Box::new(platform::Removal::prepare(
            parents, locator, entry,
        )?))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RemovalFsStep {
    BeforeReverify,
    BeforeNativeRename,
    AfterRename,
    SyncSource,
    SyncTarget,
    ReopenTarget,
    ReadManifest,
    ReadReceipt,
    VerifyIdentities,
    BeforeCleanup,
    BeforeCleanupReopen,
    BeforeManifestRemoval,
    AfterManifestRemoval,
    BeforeReceiptRemoval,
    AfterReceiptRemoval,
    BeforeDirectoryRemoval,
    AfterDirectoryRemoval,
    SyncCleanup,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ImportFsStep {
    CreateStage,
    WriteManifest,
    SyncManifest,
    WriteReceipt,
    SyncReceipt,
    #[cfg(unix)]
    SyncStageDirectory,
    #[cfg(unix)]
    SyncStagingParent,
    BeforePromotion,
    AfterPromotion,
    SyncDestinationDirectory,
    AfterStageRemoval,
}

#[derive(Clone, Default)]
pub(super) struct Hooks {
    #[cfg(test)]
    pub(super) controls: TestControls,
}
impl Hooks {
    pub(super) fn removal_checkpoint(&self, _step: RemovalFsStep) -> io::Result<()> {
        #[cfg(test)]
        match _step {
            RemovalFsStep::AfterRename => crash_checkpoint("removal", "rename-returned"),
            RemovalFsStep::AfterManifestRemoval => crash_checkpoint("removal", "manifest-removed"),
            RemovalFsStep::AfterReceiptRemoval => crash_checkpoint("removal", "receipt-removed"),
            RemovalFsStep::AfterDirectoryRemoval => {
                crash_checkpoint("removal", "directory-removed")
            }
            _ => (),
        }
        #[cfg(test)]
        if let Some(hook) = &self.controls.removal_hook {
            return hook(_step);
        }
        Ok(())
    }
    pub(super) fn checkpoint(&self, _step: ImportFsStep) -> io::Result<()> {
        #[cfg(test)]
        {
            IMPORT_DIAGNOSTICS.with(|diagnostics| diagnostics.borrow_mut().steps.push(_step));
            match _step {
                ImportFsStep::AfterPromotion => crash_checkpoint("import", "promotion-returned"),
                ImportFsStep::SyncDestinationDirectory => {
                    crash_checkpoint("import", "target-verified")
                }
                _ => (),
            }
        }
        #[cfg(test)]
        if let Some(hook) = &self.controls.hook {
            return hook(_step);
        }
        Ok(())
    }
    pub(super) fn uuid(&self) -> uuid::Uuid {
        #[cfg(test)]
        IMPORT_DIAGNOSTICS.with(|diagnostics| diagnostics.borrow_mut().uuid_count += 1);
        #[cfg(test)]
        if let Some(supplier) = &self.controls.uuid {
            return supplier();
        }
        uuid::Uuid::new_v4()
    }
}

/// Test-only bridge. Only the exact ignored child, started with a cleared
/// environment and a canonical disposable fixture root, may terminate itself.
#[cfg(test)]
pub(crate) fn crash_checkpoint(lifecycle: &str, checkpoint: &str) {
    if std::env::var("EASIFLUX_PLUGIN_TEST_CHILD").as_deref() != Ok(lifecycle) {
        return;
    }
    #[cfg(unix)]
    if matches!(checkpoint, "promotion-returned" | "rename-returned") {
        DOCUMENT_PARENT_SYNCS.with(|synced| {
            let (state, ownership) = synced.get();
            assert!(
                state,
                "package rename must follow successful state parent sync"
            );
            if lifecycle == "removal" {
                assert!(
                    ownership,
                    "removal rename must follow successful ownership parent sync"
                );
            }
        });
    }
    if std::env::var("EASIFLUX_PLUGIN_TEST_CHECKPOINT").as_deref() != Ok(checkpoint) {
        return;
    }
    let exact = match lifecycle {
        "import" => "plugin::runtime::import::tests::import_crash_child",
        "removal" => "plugin::runtime::removal::tests::removal_crash_child",
        _ => return,
    };
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(args, ["--ignored", "--exact", exact, "--nocapture"]);
    let root = std::path::PathBuf::from(std::env::var_os("EASIFLUX_PLUGIN_TEST_ROOT").unwrap());
    assert_eq!(root.canonicalize().unwrap(), root);
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with(&format!("plugin-runtime-{lifecycle}-")));
    std::process::exit(73);
}

#[cfg(all(test, unix))]
thread_local! {
    static DOCUMENT_PARENT_SYNCS: std::cell::Cell<(bool, bool)> = const { std::cell::Cell::new((false, false)) };
}
#[cfg(all(test, unix))]
pub(super) fn document_parent_synced(name: &str) {
    DOCUMENT_PARENT_SYNCS.with(|synced| {
        let (state, ownership) = synced.get();
        synced.set((
            state || name == "state.json",
            ownership || name == "managed-ownership.json",
        ));
    });
}

#[cfg(test)]
#[derive(Debug)]
struct ImportFailureEvent {
    operation: &'static str,
    sequence: usize,
    checkpoint: Option<ImportFsStep>,
    kind: io::ErrorKind,
    raw_os_error: Option<i32>,
    ntstatus: Option<i32>,
}

#[cfg(test)]
#[derive(Debug, Default)]
pub(super) struct ImportDiagnostics {
    steps: Vec<ImportFsStep>,
    uuid_count: usize,
    errors: Vec<(io::ErrorKind, Option<i32>)>,
    events: Vec<ImportFailureEvent>,
}
#[cfg(test)]
thread_local! {
    pub(super) static IMPORT_DIAGNOSTICS: std::cell::RefCell<ImportDiagnostics> = Default::default();
}
#[cfg(test)]
pub(super) fn import_io_failure(error: &io::Error) {
    import_operation_failure("import-write-failed", error);
}
#[cfg(test)]
pub(super) fn import_operation_failure(operation: &'static str, error: &io::Error) {
    import_failure_event(operation, error, None);
}
#[cfg(all(test, windows))]
fn import_native_failure(operation: &'static str, error: &io::Error, ntstatus: i32) {
    import_failure_event(operation, error, Some(ntstatus));
}
#[cfg(test)]
fn import_failure_event(operation: &'static str, error: &io::Error, ntstatus: Option<i32>) {
    IMPORT_DIAGNOSTICS.with(|diagnostics| {
        let mut diagnostics = diagnostics.borrow_mut();
        diagnostics.errors.push((error.kind(), error.raw_os_error()));
        let event = ImportFailureEvent {
            operation,
            sequence: diagnostics.events.len() + 1,
            checkpoint: diagnostics.steps.last().copied(),
            kind: error.kind(),
            raw_os_error: error.raw_os_error(),
            ntstatus,
        };
        diagnostics.events.push(event);
        let event = diagnostics.events.last().unwrap();
        // No paths, slots, object IDs, metadata, or manifest/receipt bytes.
        eprintln!("local-package operation={} sequence={} checkpoint={:?} kind={:?} raw_os_error={:?} ntstatus={:?} uuid_count={}", event.operation, event.sequence, event.checkpoint, event.kind, event.raw_os_error, event.ntstatus, diagnostics.uuid_count);
    });
}
#[cfg(test)]
#[derive(Clone, Default)]
pub(super) struct TestControls {
    pub(super) removal_hook:
        Option<std::sync::Arc<dyn Fn(RemovalFsStep) -> io::Result<()> + Send + Sync>>,
    pub(super) hook: Option<std::sync::Arc<dyn Fn(ImportFsStep) -> io::Result<()> + Send + Sync>>,
    pub(super) uuid: Option<std::sync::Arc<dyn Fn() -> uuid::Uuid + Send + Sync>>,
    pub(super) rename_error: Option<io::ErrorKind>,
}

#[cfg(test)]
mod tests;
