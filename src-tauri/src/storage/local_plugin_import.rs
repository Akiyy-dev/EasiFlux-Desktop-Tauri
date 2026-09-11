use std::{io, path::PathBuf};

use crate::{models::config::APP_NAME, plugin::import::ImportCommitFailure};

mod platform;

pub(crate) trait LocalManifestImportStorage: Send + Sync {
    fn prepare_stage(&self, bytes: &[u8])
        -> Result<Box<dyn OwnedImportStage>, ImportCommitFailure>;
}

pub(crate) trait OwnedImportStage: Send {
    fn promote(&mut self) -> Result<Promotion, ImportCommitFailure>;
    fn cleanup(&mut self) -> Result<(), ImportCommitFailure>;
}

#[derive(Debug)]
pub(crate) struct Promotion {
    pub(crate) object_identity_verified: bool,
}

/// The system root is resolved only when preparing a stage, never at construction.
pub(crate) struct SystemLocalManifestImportStorage {
    plugins_root: Option<PathBuf>,
    hooks: Hooks,
}

impl SystemLocalManifestImportStorage {
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

    #[cfg(test)]
    fn with_controls(mut self, controls: TestControls) -> Self {
        self.hooks.controls = controls;
        self
    }
}

impl LocalManifestImportStorage for SystemLocalManifestImportStorage {
    fn prepare_stage(
        &self,
        bytes: &[u8],
    ) -> Result<Box<dyn OwnedImportStage>, ImportCommitFailure> {
        let root = self
            .plugins_root
            .clone()
            .or_else(|| dirs::config_dir().map(|root| root.join(APP_NAME).join("plugins")))
            .ok_or(ImportCommitFailure::WriteFailed)?;
        let mut parents =
            platform::ImportDirectories::open_or_create(&root).map_err(write_failed)?;
        parents.hooks = self.hooks.clone();
        // Includes unknown entries and abandoned stages. Nothing is adopted or swept.
        if parents.staging_count(17).map_err(write_failed)? >= 16 {
            return Err(ImportCommitFailure::StagingCapacityExceeded);
        }
        for _ in 0..4 {
            let stage_name = format!("stage-{}", self.hooks.uuid().simple());
            match parents.create_stage(&stage_name, bytes) {
                Ok((stage_identity, manifest_identity)) => {
                    return Ok(Box::new(DiskStage {
                        parents,
                        stage_name,
                        stage_identity,
                        manifest_identity,
                        state: StageState::Prepared,
                    }));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(write_failed(error)),
            }
        }
        Err(ImportCommitFailure::WriteFailed)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum StageState {
    Prepared,
    Promoted,
    Cleaned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    volume: u64,
    object: u128,
}

struct DiskStage {
    parents: platform::ImportDirectories,
    stage_name: String,
    stage_identity: FileIdentity,
    manifest_identity: FileIdentity,
    state: StageState,
}

impl OwnedImportStage for DiskStage {
    fn promote(&mut self) -> Result<Promotion, ImportCommitFailure> {
        if self.state != StageState::Prepared {
            return Err(ImportCommitFailure::WriteFailed);
        }
        self.parents
            .verify_stage(
                &self.stage_name,
                &self.stage_identity,
                &self.manifest_identity,
            )
            .map_err(write_failed)?;
        let mut target = None;
        for _ in 0..4 {
            let name = format!("pkg-{}", self.parents.hooks.uuid().simple());
            match self.parents.promote_exclusive(&self.stage_name, &name) {
                Ok(()) => {
                    // The rename is the commit point: no subsequent error is precommit.
                    self.state = StageState::Promoted;
                    target = Some(name);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(write_failed(error)),
            }
        }
        let target = target.ok_or(ImportCommitFailure::WriteFailed)?;
        let verified = self
            .parents
            .hooks
            .checkpoint(ImportFsStep::AfterPromotion)
            .and_then(|()| {
                self.parents
                    .verify_target(&target, &self.stage_identity, &self.manifest_identity)
            });
        let object_identity_verified = match verified {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(kind = ?error.kind(), "Imported manifest identity could not be confirmed");
                false
            }
        };
        if let Err(error) = self.parents.sync_after_promotion() {
            tracing::warn!(kind = ?error.kind(), "Imported manifest directory sync failed");
        }
        Ok(Promotion {
            object_identity_verified,
        })
    }

    fn cleanup(&mut self) -> Result<(), ImportCommitFailure> {
        match self.state {
            StageState::Promoted => Err(ImportCommitFailure::WriteFailed),
            StageState::Cleaned => Ok(()),
            StageState::Prepared => {
                self.parents
                    .cleanup_stage(
                        &self.stage_name,
                        &self.stage_identity,
                        &self.manifest_identity,
                    )
                    .map_err(write_failed)?;
                self.state = StageState::Cleaned;
                Ok(())
            }
        }
    }
}

// Intentionally no Drop cleanup: forgotten stages are preserved across restart.
fn write_failed(error: io::Error) -> ImportCommitFailure {
    tracing::warn!(kind = ?error.kind(), "Local manifest import filesystem operation failed");
    ImportCommitFailure::WriteFailed
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImportFsStep {
    CreateStage,
    WriteManifest,
    SyncManifest,
    #[cfg(unix)]
    SyncStageDirectory,
    #[cfg(unix)]
    SyncStagingParent,
    BeforePromotion,
    AfterPromotion,
    SyncDestinationDirectory,
}

#[derive(Clone, Default)]
struct Hooks {
    #[cfg(test)]
    controls: TestControls,
}

impl Hooks {
    fn checkpoint(&self, _step: ImportFsStep) -> io::Result<()> {
        #[cfg(test)]
        if let Some(hook) = &self.controls.hook {
            return hook(_step);
        }
        Ok(())
    }

    fn uuid(&self) -> uuid::Uuid {
        #[cfg(test)]
        if let Some(supplier) = &self.controls.uuid {
            return supplier();
        }
        uuid::Uuid::new_v4()
    }
}

#[cfg(test)]
#[derive(Clone, Default)]
struct TestControls {
    hook: Option<std::sync::Arc<dyn Fn(ImportFsStep) -> io::Result<()> + Send + Sync>>,
    uuid: Option<std::sync::Arc<dyn Fn() -> uuid::Uuid + Send + Sync>>,
    rename_error: Option<io::ErrorKind>,
}

#[cfg(test)]
mod tests;
