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
        if let Some(hook) = &self.controls.removal_hook {
            return hook(_step);
        }
        Ok(())
    }
    pub(super) fn checkpoint(&self, _step: ImportFsStep) -> io::Result<()> {
        #[cfg(test)]
        if let Some(hook) = &self.controls.hook {
            return hook(_step);
        }
        Ok(())
    }
    pub(super) fn uuid(&self) -> uuid::Uuid {
        #[cfg(test)]
        if let Some(supplier) = &self.controls.uuid {
            return supplier();
        }
        uuid::Uuid::new_v4()
    }
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
