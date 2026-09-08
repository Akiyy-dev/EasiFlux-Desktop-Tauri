use std::{io, path::PathBuf};

use crate::{
    models::config::APP_NAME,
    plugin::{
        discovery::LocalPackageLocator,
        ownership::{FileIdentity, ManagedOwnershipEntryV1},
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
    pub(super) hook: Option<std::sync::Arc<dyn Fn(ImportFsStep) -> io::Result<()> + Send + Sync>>,
    pub(super) uuid: Option<std::sync::Arc<dyn Fn() -> uuid::Uuid + Send + Sync>>,
    pub(super) rename_error: Option<io::ErrorKind>,
}

#[cfg(test)]
mod tests;
