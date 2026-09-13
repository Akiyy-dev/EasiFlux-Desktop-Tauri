//! Lifecycle documents share five reserved names. Unix name operations assume a
//! single writer and no manual edits during a transaction; identity checks are
//! best effort, not protection against a continuously racing same-user account.
use crate::error::AppError;
use std::{
    fs::File,
    io::{self, Read, Write},
    path::PathBuf,
};
pub(crate) enum CandidateBytes {
    Missing,
    Oversized,
    Bounded(Vec<u8>),
}
mod platform;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PersistOutcome {
    NotCommitted,
    CommittedProcessCrashSafe,
    CommittedDurable,
}

#[derive(Debug, thiserror::Error)]
#[error("plugin document persistence failed")]
pub(crate) struct PersistFailure {
    pub(crate) outcome: PersistOutcome,
}
pub(crate) type PersistResult = Result<PersistOutcome, PersistFailure>;
impl From<AppError> for PersistFailure {
    fn from(_: AppError) -> Self {
        Self {
            outcome: PersistOutcome::NotCommitted,
        }
    }
}
impl From<PersistFailure> for AppError {
    fn from(_: PersistFailure) -> Self {
        AppError::Plugin {
            code: "plugin_state_persist_failed",
            message: "插件状态保存失败",
            diagnostic: None,
        }
    }
}
pub(crate) fn persist_outcome_committed(outcome: PersistOutcome) -> bool {
    outcome != PersistOutcome::NotCommitted
}
pub(crate) fn outcome_satisfies_destructive_barrier(outcome: PersistOutcome) -> bool {
    if cfg!(windows) {
        persist_outcome_committed(outcome)
    } else {
        outcome == PersistOutcome::CommittedDurable
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SafeDocumentNames {
    main: &'static str,
    tmp: &'static str,
    bak: &'static str,
    pending: &'static str,
    bak_pending: &'static str,
}
impl SafeDocumentNames {
    pub(crate) fn plugin_state() -> Self {
        Self {
            main: "state.json",
            tmp: "state.json.tmp",
            bak: "state.json.bak",
            pending: "state.json.pending",
            bak_pending: "state.json.bak.pending",
        }
    }
    pub(crate) fn managed_ownership() -> Self {
        Self {
            main: "managed-ownership.json",
            tmp: "managed-ownership.json.tmp",
            bak: "managed-ownership.json.bak",
            pending: "managed-ownership.json.pending",
            bak_pending: "managed-ownership.json.bak.pending",
        }
    }
    fn all(self) -> [&'static str; 5] {
        [
            self.main,
            self.tmp,
            self.bak,
            self.pending,
            self.bak_pending,
        ]
    }
}

// Checkpoints exist only in test builds; the macro erases them in production.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SafeDocumentStep {
    CleanPending,
    CleanBackupPending,
    CreatePending,
    WritePending,
    FlushPending,
    CreateBackupPending,
    WriteBackupPending,
    FlushBackupPending,
    ReplaceBackup,
    DeleteLegacyTmp,
    SyncAfterLegacyTmp,
    ReplaceMain,
    VerifyMain,
    SyncParent,
}
macro_rules! checkpoint {
    ($self:ident, $step:ident) => {{
        #[cfg(test)]
        $self.checkpoint(SafeDocumentStep::$step)?;
    }};
}
#[cfg(test)]
pub(super) type StepHook = std::sync::Arc<dyn Fn(SafeDocumentStep) -> io::Result<()> + Send + Sync>;

pub(crate) struct SafePluginDocument {
    root: PathBuf,
    names: SafeDocumentNames,
    max_bytes: usize,
    #[cfg(test)]
    pub(crate) hook: Option<StepHook>,
}
impl SafePluginDocument {
    pub(crate) fn new(root: PathBuf, names: SafeDocumentNames, max_bytes: usize) -> Self {
        Self {
            root,
            names,
            max_bytes,
            #[cfg(test)]
            hook: None,
        }
    }
    #[cfg(test)]
    fn checkpoint(&self, step: SafeDocumentStep) -> io::Result<()> {
        if step == SafeDocumentStep::VerifyMain && self.names.main == "managed-ownership.json" {
            super::local_plugin_package::crash_checkpoint("import", "ownership-main-committed");
            super::local_plugin_package::crash_checkpoint("removal", "removing-main-committed");
        }
        self.hook.as_ref().map_or(Ok(()), |hook| hook(step))
    }
    pub(crate) fn load_candidates(&self) -> io::Result<Vec<CandidateBytes>> {
        let parent = match platform::Directory::open(&self.root, false) {
            Ok(parent) => parent,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(vec![
                    CandidateBytes::Missing,
                    CandidateBytes::Missing,
                    CandidateBytes::Missing,
                ])
            }
            Err(error) => return Err(error),
        };
        let mut candidates = Vec::new();
        for name in [self.names.main, self.names.tmp, self.names.bak] {
            candidates.push(match self.open_existing(&parent, name)? {
                None => CandidateBytes::Missing,
                Some(mut file) => {
                    let mut bytes = Vec::new();
                    // Probe one byte beyond the limit, never materialize an unbounded file.
                    (&mut file)
                        .take(self.max_bytes as u64 + 1)
                        .read_to_end(&mut bytes)?;
                    if bytes.len() > self.max_bytes {
                        CandidateBytes::Oversized
                    } else {
                        CandidateBytes::Bounded(bytes)
                    }
                }
            });
        }
        Ok(candidates)
    }
    fn open_existing(&self, parent: &platform::Directory, name: &str) -> io::Result<Option<File>> {
        parent.exact_case(name)?;
        match parent.open_file(name, false) {
            Ok(file) => {
                platform::identity(&file)?;
                Ok(Some(file))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
    fn bounded(&self, file: &File) -> io::Result<()> {
        if file.metadata()?.len() > self.max_bytes as u64 {
            Err(io::Error::other("oversized reserved object"))
        } else {
            Ok(())
        }
    }
    pub(crate) fn persist(&self, previous: Option<&[u8]>, next: &[u8]) -> PersistResult {
        let mut outcome = PersistOutcome::NotCommitted;
        let result = (|| -> io::Result<()> {
            if next.len() > self.max_bytes || previous.is_some_and(|p| p.len() > self.max_bytes) {
                return Err(io::Error::other("document exceeds limit"));
            }
            let parent = platform::Directory::open(&self.root, true)?;
            // Validate and retain every existing authority/work handle before mutation.
            let mut held = Vec::new();
            for name in self.names.all() {
                let file = self.open_existing(&parent, name)?;
                if let Some(file) = &file {
                    self.bounded(file)?;
                }
                held.push(file);
            }
            checkpoint!(self, CleanPending);
            if let Some(file) = held[3].take() {
                self.bounded(&file)?;
                parent.remove(self.names.pending, file)?;
            }
            checkpoint!(self, CleanBackupPending);
            if let Some(file) = held[4].take() {
                self.bounded(&file)?;
                parent.remove(self.names.bak_pending, file)?;
            }
            parent.sync()?;
            checkpoint!(self, CreatePending);
            let mut pending = parent.open_file(self.names.pending, true)?;
            platform::identity(&pending)?;
            checkpoint!(self, WritePending);
            pending.write_all(next)?;
            checkpoint!(self, FlushPending);
            pending.sync_all()?;
            if let Some(previous) = previous {
                checkpoint!(self, CreateBackupPending);
                let mut backup = parent.open_file(self.names.bak_pending, true)?;
                platform::identity(&backup)?;
                checkpoint!(self, WriteBackupPending);
                backup.write_all(previous)?;
                checkpoint!(self, FlushBackupPending);
                backup.sync_all()?;
                checkpoint!(self, ReplaceBackup);
                self.bounded(&backup)?;
                if let Some(file) = &held[2] {
                    self.bounded(file)?;
                }
                parent.replace(
                    self.names.bak_pending,
                    &backup,
                    self.names.bak,
                    held[2].as_ref(),
                )?;
                parent.verify(self.names.bak, &backup)?;
                self.bounded(&backup)?;
            }
            checkpoint!(self, DeleteLegacyTmp);
            if let Some(file) = held[1].take() {
                self.bounded(&file)?;
                parent.remove(self.names.tmp, file)?;
            }
            checkpoint!(self, SyncAfterLegacyTmp);
            parent.sync()?;
            checkpoint!(self, ReplaceMain);
            self.bounded(&pending)?;
            if let Some(file) = &held[0] {
                self.bounded(file)?;
            }
            parent.replace(
                self.names.pending,
                &pending,
                self.names.main,
                held[0].as_ref(),
            )?;
            outcome = PersistOutcome::CommittedProcessCrashSafe;
            checkpoint!(self, VerifyMain);
            parent.verify(self.names.main, &pending)?;
            self.bounded(&pending)?;
            checkpoint!(self, SyncParent);
            parent.sync()?;
            #[cfg(all(test, unix))]
            super::local_plugin_package::document_parent_synced(self.names.main);
            #[cfg(unix)]
            {
                outcome = PersistOutcome::CommittedDurable;
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(outcome),
            Err(error) => {
                tracing::warn!(error = %error, ?outcome, "secure plugin document transaction failed");
                Err(PersistFailure { outcome })
            }
        }
    }
}

#[cfg(test)]
mod tests;
