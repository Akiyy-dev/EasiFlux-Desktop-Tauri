use std::{collections::BTreeSet, io, path::PathBuf};

#[cfg(test)]
use super::local_plugin_package::TestControls;
use super::local_plugin_package::{
    platform, ImportFsStep, PromotedManagedPackage, SystemLocalPluginPackageStorage,
};
use crate::plugin::{
    import::ImportCommitFailure,
    ownership::{ManagedOwnershipEntryV1, OwnershipReceiptV1, PackageSlot, ReceiptId},
    record::PluginRecord,
};

pub(crate) trait LocalManifestImportStorage: Send + Sync {
    fn prepare_stage(
        &self,
        record: &PluginRecord,
        bytes: &[u8],
    ) -> Result<Box<dyn OwnedImportStage>, ImportCommitFailure>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ImportPromotionState {
    NotCommitted,
    Committed,
    CommitUnconfirmed,
}

pub(crate) trait OwnedImportStage: Send {
    fn promote(&mut self) -> Result<PromotedManagedPackage, ImportCommitFailure>;
    fn promotion_state(&self) -> ImportPromotionState;
    fn cleanup(&mut self) -> Result<(), ImportCommitFailure>;
}

pub(crate) struct SystemLocalManifestImportStorage {
    packages: SystemLocalPluginPackageStorage,
}
impl SystemLocalManifestImportStorage {
    pub(crate) fn new() -> Self {
        Self {
            packages: SystemLocalPluginPackageStorage::new(),
        }
    }
    pub(crate) fn with_plugins_root(root: PathBuf) -> Self {
        Self {
            packages: SystemLocalPluginPackageStorage::with_plugins_root(root),
        }
    }
    #[cfg(test)]
    pub(super) fn with_controls(mut self, controls: TestControls) -> Self {
        self.packages.hooks.controls = controls;
        self
    }
}

impl LocalManifestImportStorage for SystemLocalManifestImportStorage {
    fn prepare_stage(
        &self,
        record: &PluginRecord,
        bytes: &[u8],
    ) -> Result<Box<dyn OwnedImportStage>, ImportCommitFailure> {
        if record.source() != crate::plugin::manifest::PluginSource::LocalDeclarative
            || bytes.len() > 16_384
            || record
                .canonical_manifest_bytes()
                .map_err(|_| ImportCommitFailure::WriteFailed)?
                != bytes
        {
            return Err(ImportCommitFailure::WriteFailed);
        }
        let parents = self.packages.open().map_err(write_failed)?;
        if parents.staging_count(17).map_err(write_failed)? >= 16 {
            return Err(ImportCommitFailure::StagingCapacityExceeded);
        }
        let mut stage = DiskStage {
            parents,
            record: record.clone(),
            bytes: bytes.to_vec(),
            attempt: None,
            cycles: 0,
            used: [BTreeSet::new(), BTreeSet::new(), BTreeSet::new()],
            state: ImportPromotionState::NotCommitted,
            finished: false,
            cleanup_blocked: false,
        };
        stage.prepare_attempt()?;
        Ok(Box::new(stage))
    }
}

struct Attempt {
    name: String,
    receipt: OwnershipReceiptV1,
    receipt_bytes: Vec<u8>,
    identities: platform::PackageIdentities,
}
struct DiskStage {
    parents: platform::ImportDirectories,
    record: PluginRecord,
    bytes: Vec<u8>,
    attempt: Option<Attempt>,
    cycles: usize,
    used: [BTreeSet<String>; 3],
    state: ImportPromotionState,
    finished: bool,
    cleanup_blocked: bool,
}
impl DiskStage {
    fn prepare_attempt(&mut self) -> Result<(), ImportCommitFailure> {
        while self.cycles < 4 {
            self.cycles += 1;
            let ids: [String; 3] =
                std::array::from_fn(|_| self.parents.hooks.uuid().simple().to_string());
            if ids
                .iter()
                .enumerate()
                .any(|(index, id)| !self.used[index].insert(id.clone()))
            {
                return Err(ImportCommitFailure::WriteFailed);
            }
            let name = format!("stage-{}", ids[0]);
            let receipt = OwnershipReceiptV1::new(
                ReceiptId::parse(&ids[1]).map_err(|_| ImportCommitFailure::WriteFailed)?,
                PackageSlot::parse(&format!("pkg-{}", ids[2]))
                    .map_err(|_| ImportCommitFailure::WriteFailed)?,
                &self.record,
            )
            .map_err(|_| ImportCommitFailure::WriteFailed)?;
            let receipt_bytes = receipt
                .canonical_bytes()
                .map_err(|_| ImportCommitFailure::WriteFailed)?;
            match self
                .parents
                .create_stage(&name, &self.bytes, &receipt_bytes)
            {
                Ok(identities) => {
                    self.attempt = Some(Attempt {
                        name,
                        receipt,
                        receipt_bytes,
                        identities,
                    });
                    return Ok(());
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(write_failed(error)),
            }
        }
        Err(ImportCommitFailure::WriteFailed)
    }
}

impl OwnedImportStage for DiskStage {
    fn promote(&mut self) -> Result<PromotedManagedPackage, ImportCommitFailure> {
        if self.finished || self.state != ImportPromotionState::NotCommitted {
            return Err(ImportCommitFailure::WriteFailed);
        }
        self.finished = true;
        loop {
            let attempt = self
                .attempt
                .as_ref()
                .ok_or(ImportCommitFailure::WriteFailed)?;
            let target = attempt.receipt.package_slot().as_str();
            match self.parents.promote_exclusive(
                &attempt.name,
                target,
                &attempt.identities,
                &self.bytes,
                &attempt.receipt_bytes,
                &mut self.state,
            ) {
                Ok(()) => {
                    self.parents
                        .hooks
                        .checkpoint(ImportFsStep::AfterPromotion)
                        .map_err(write_failed)?;
                    let verified = self
                        .parents
                        .verify_target(target, &attempt.identities)
                        .map_err(write_failed)?;
                    verified
                        .verify_content(&self.bytes, &attempt.receipt_bytes)
                        .map_err(write_failed)?;
                    let locator = verified
                        .locator(attempt.receipt.clone())
                        .map_err(write_failed)?;
                    let entry = ManagedOwnershipEntryV1::managed(
                        locator
                            .receipt
                            .clone()
                            .ok_or(ImportCommitFailure::WriteFailed)?,
                        &self.record,
                        locator.directory_identity,
                        locator.manifest_identity,
                    )
                    .map_err(|_| ImportCommitFailure::WriteFailed)?;
                    self.parents.sync_after_promotion().map_err(write_failed)?;
                    return Ok(PromotedManagedPackage { entry, locator });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    self.parents
                        .prove_collision(
                            &attempt.name,
                            target,
                            &attempt.identities,
                            &self.bytes,
                            &attempt.receipt_bytes,
                        )
                        .map_err(write_failed)?;
                    self.state = ImportPromotionState::NotCommitted;
                    self.cleanup_blocked = true;
                    self.parents
                        .cleanup_stage(&attempt.name, &attempt.identities)
                        .map_err(write_failed)?;
                    self.cleanup_blocked = false;
                    self.attempt = None;
                    self.prepare_attempt()?;
                }
                Err(error) => return Err(write_failed(error)),
            }
        }
    }
    fn promotion_state(&self) -> ImportPromotionState {
        self.state
    }
    fn cleanup(&mut self) -> Result<(), ImportCommitFailure> {
        if self.state != ImportPromotionState::NotCommitted || self.cleanup_blocked {
            return Err(ImportCommitFailure::WriteFailed);
        }
        self.finished = true;
        self.cleanup_blocked = true;
        if let Some(attempt) = &self.attempt {
            self.parents
                .cleanup_stage(&attempt.name, &attempt.identities)
                .map_err(write_failed)?;
        }
        self.cleanup_blocked = false;
        self.attempt = None;
        self.finished = true;
        Ok(())
    }
}

// No Drop cleanup: abandoned, uncertain, and committed packages are preserved.
fn write_failed(error: io::Error) -> ImportCommitFailure {
    tracing::warn!(kind = ?error.kind(), "Local manifest import filesystem operation failed");
    ImportCommitFailure::WriteFailed
}

#[cfg(test)]
mod tests;
