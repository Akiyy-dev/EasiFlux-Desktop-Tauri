//! Model-independent, bounded file reads and same-directory replacement primitives.
//!
//! `.tmp` is a recovery candidate, never a staging area for a new save. New bytes
//! stay in `.pending` until promotion, so an error cannot turn them into recovery
//! input even when staging cleanup or backup restoration also fails.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WriteStep {
    TempWrite,
    BackupRotation,
    Promotion,
    Restore,
    PostCommitSync,
}

pub(super) enum CandidateBytes {
    Missing,
    Bounded(Vec<u8>),
    Oversized,
}

pub(super) struct AtomicFile {
    pub main: PathBuf,
    #[cfg(test)]
    pub hook: Option<std::sync::Arc<dyn Fn(WriteStep) -> std::io::Result<()> + Send + Sync>>,
}

impl AtomicFile {
    pub fn new(main: PathBuf) -> Self {
        Self {
            main,
            #[cfg(test)]
            hook: None,
        }
    }

    pub fn temp(&self) -> PathBuf {
        sibling(&self.main, ".tmp")
    }

    pub fn backup(&self) -> PathBuf {
        sibling(&self.main, ".bak")
    }

    /// Caller holds its transaction lock and supplies the previously validated
    /// current document, regardless of which recovery candidate contained it.
    pub fn replace(&self, bytes: &[u8], previous: Option<&[u8]>) -> io::Result<()> {
        let pending = sibling(&self.main, ".pending");
        let backup_pending = sibling(&self.main, ".bak.pending");
        let backup = self.backup();
        if let Some(parent) = self.main.parent() {
            fs::create_dir_all(parent)?;
        }

        // A failed staging write leaves the existing candidates untouched.
        write_and_sync(&pending, bytes)?;
        self.checkpoint(WriteStep::TempWrite)?;

        if let Some(previous) = previous {
            // Stage the backup as well: never truncate the only valid backup.
            write_and_sync(&backup_pending, previous)?;
            self.checkpoint(WriteStep::BackupRotation)?;
            // rename replaces an existing file on Windows and Unix. Do not
            // unlink first: this may be the only committed recovery candidate.
            fs::rename(&backup_pending, &backup)?;
            sync_parent_directory(&backup)?;
        }

        // A stale temp must not outrank the saved previous state after rollback.
        // Do this before touching main; errors here leave the old current intact.
        remove_if_present(&self.temp())?;
        remove_if_present(&self.main)?;
        let promotion = self
            .checkpoint(WriteStep::Promotion)
            .and_then(|()| fs::rename(&pending, &self.main));

        if let Err(error) = promotion {
            if let Some(previous) = previous {
                if let Err(restore_error) = self
                    .checkpoint(WriteStep::Restore)
                    .and_then(|()| write_and_sync(&self.main, previous))
                    .and_then(|()| sync_parent_directory(&self.main))
                {
                    // The already validated previous bytes are restored through
                    // a writable, synced handle; backup is never moved, so double
                    // failure leaves committed recovery input for a fresh store.
                    tracing::warn!(error = %restore_error, "atomic file backup restore failed");
                }
            }
            return Err(error);
        }

        // Rename is the commit point. Once it succeeds, reporting failure would
        // lie about the visible state. Unix parent sync is attempted for crash
        // durability; its failure is logged, not reported as a rejected commit.
        if let Err(error) = self
            .checkpoint(WriteStep::PostCommitSync)
            .and_then(|()| sync_parent_directory(&self.main))
        {
            tracing::warn!(error_kind = ?error.kind(), "atomic file committed; parent sync failed");
        }
        Ok(())
    }

    fn checkpoint(&self, _step: WriteStep) -> io::Result<()> {
        #[cfg(test)]
        if let Some(hook) = &self.hook {
            hook(_step)?;
        }
        Ok(())
    }
}

pub(super) fn read_bounded(path: &Path, max_bytes: usize) -> io::Result<CandidateBytes> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(CandidateBytes::Missing)
        }
        Err(error) => return Err(error),
    };
    if file.metadata()?.len() > max_bytes as u64 {
        return Ok(CandidateBytes::Oversized);
    }
    // Bound the read itself too: metadata is not a defense against file growth.
    let mut bytes = Vec::new();
    file.take(max_bytes as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        Ok(CandidateBytes::Oversized)
    } else {
        Ok(CandidateBytes::Bounded(bytes))
    }
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    value.into()
}

fn write_and_sync(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

// Like notification_store: Rust's portable File API cannot open directories for
// syncing on Windows. Individual files still receive sync_all before promotion.
#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}
