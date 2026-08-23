use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::models::config::APP_NAME;
use crate::models::notification::{NotificationRecord, NotificationScope};

pub const NOTIFICATION_SCHEMA_VERSION: u32 = 1;
pub const MAX_NOTIFICATION_PARTITION_ITEMS: usize = 1_000;

const TEMP_SUFFIX: &str = ".tmp";
const BACKUP_SUFFIX: &str = ".bak";
const INVALID_NOTIFICATION_FILE: &str = "INVALID_NOTIFICATION_FILE";
const NOTIFICATION_STORAGE_UNAVAILABLE: &str = "NOTIFICATION_STORAGE_UNAVAILABLE";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationFileV1 {
    pub schema_version: u32,
    pub revision: u64,
    pub partitions: Vec<NotificationPartition>,
}

impl NotificationFileV1 {
    pub fn empty() -> Self {
        Self {
            schema_version: NOTIFICATION_SCHEMA_VERSION,
            revision: 0,
            partitions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPartition {
    pub scope: NotificationScope,
    pub items: Vec<NotificationRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoverySource {
    Temp,
    Backup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationLoadStatus {
    Clean,
    Recovered { source: RecoverySource },
    ResetFromCorruption,
    UnsupportedSchema { found: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct NotificationLoadOutcome {
    pub file: NotificationFileV1,
    pub status: NotificationLoadStatus,
}

pub(crate) trait NotificationPersistence: Send + Sync {
    fn save(&self, file: &NotificationFileV1) -> AppResult<()>;
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailurePoint {
    PromoteTemp,
    RestoreBackup,
    PreserveCorrupt,
}

pub struct NotificationStore {
    path: PathBuf,
    write_lock: Mutex<()>,
    #[cfg(test)]
    failures: Vec<FailurePoint>,
}

impl NotificationStore {
    pub fn new() -> Self {
        Self::with_path_inner(
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(APP_NAME)
                .join("notifications")
                .join("notifications.v1.json"),
        )
    }

    #[cfg(test)]
    pub(crate) fn with_path(path: PathBuf) -> Self {
        Self::with_path_inner(path)
    }

    fn with_path_inner(path: PathBuf) -> Self {
        Self {
            path,
            write_lock: Mutex::new(()),
            #[cfg(test)]
            failures: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_path_and_failures(path: PathBuf, failures: Vec<FailurePoint>) -> Self {
        Self {
            path,
            write_lock: Mutex::new(()),
            failures,
        }
    }

    pub fn load(&self) -> AppResult<NotificationLoadOutcome> {
        let _guard = self.lock_writes()?;
        let paths = NotificationPaths::new(&self.path);
        let candidates = [
            (&paths.main, None, "main"),
            (&paths.temp, Some(RecoverySource::Temp), "temp"),
            (&paths.backup, Some(RecoverySource::Backup), "backup"),
        ];
        let mut corrupt_candidates = Vec::new();

        for (path, source, path_kind) in candidates {
            match fs::symlink_metadata(path) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(storage_unavailable("inspect", path_kind, error));
                }
            }

            match read_candidate(path) {
                Ok(Candidate::Valid(file)) => {
                    let status = match source {
                        None => NotificationLoadStatus::Clean,
                        Some(source) => {
                            if let Err(error) = self.save_validated(&file) {
                                tracing::warn!(
                                    source = recovery_source_name(source),
                                    error_code = storage_error_code(&error),
                                    "notification recovery normalization failed"
                                );
                            }
                            NotificationLoadStatus::Recovered { source }
                        }
                    };
                    return Ok(NotificationLoadOutcome { file, status });
                }
                Ok(Candidate::Future(found)) => {
                    return Ok(NotificationLoadOutcome {
                        file: NotificationFileV1::empty(),
                        status: NotificationLoadStatus::UnsupportedSchema { found },
                    });
                }
                Ok(Candidate::Corrupt) => {
                    corrupt_candidates.push((path.to_path_buf(), path_kind));
                }
                Err(error) => return Err(storage_unavailable("read", path_kind, error)),
            }
        }

        if corrupt_candidates.is_empty() {
            Ok(NotificationLoadOutcome {
                file: NotificationFileV1::empty(),
                status: NotificationLoadStatus::Clean,
            })
        } else {
            self.preserve_corrupt_candidates(&corrupt_candidates)?;
            Ok(NotificationLoadOutcome {
                file: NotificationFileV1::empty(),
                status: NotificationLoadStatus::ResetFromCorruption,
            })
        }
    }

    pub fn save(&self, file: &NotificationFileV1) -> AppResult<()> {
        validate_file(file)?;
        let _guard = self.lock_writes()?;
        self.save_validated(file)
    }

    fn save_validated(&self, file: &NotificationFileV1) -> AppResult<()> {
        validate_file(file)?;
        let paths = NotificationPaths::new(&self.path);
        ensure_supported_save_target(&paths)?;
        let main_candidate = existing_candidate(&paths.main, "main")?;
        let main_is_valid = matches!(main_candidate, Some(Candidate::Valid(_)));
        let backup_is_valid = matches!(
            existing_candidate(&paths.backup, "backup")?,
            Some(Candidate::Valid(_))
        );
        let bytes = serde_json::to_vec_pretty(file).map_err(|_| invalid_file())?;

        if let Some(parent) = paths.main.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| storage_unavailable("create_directory", "parent", error))?;
        }
        write_and_sync(&paths.temp, &bytes)?;

        if main_is_valid {
            remove_if_present(&paths.backup, "backup")?;
            rename_and_sync(&paths.main, &paths.backup, "rotate_main")?;
        } else {
            remove_if_present(&paths.main, "main")?;
        }

        if let Err(replace_error) = self.promote_temp(&paths.temp, &paths.main) {
            if main_is_valid || backup_is_valid {
                if let Err(restore_error) = self.restore_backup(&paths.backup, &paths.main) {
                    tracing::warn!(
                        replace_error_code = storage_error_code(&replace_error),
                        restore_error_code = storage_error_code(&restore_error),
                        "notification temp promotion and backup restore failed"
                    );
                }
            }
            return Err(replace_error);
        }
        Ok(())
    }

    fn promote_temp(&self, temp_path: &Path, main_path: &Path) -> AppResult<()> {
        self.maybe_fail(FailurePointName::PromoteTemp, "promote_temp", "main")?;
        rename_and_sync(temp_path, main_path, "promote_temp")
    }

    fn restore_backup(&self, backup_path: &Path, main_path: &Path) -> AppResult<()> {
        self.maybe_fail(FailurePointName::RestoreBackup, "restore_backup", "main")?;
        rename_and_sync(backup_path, main_path, "restore_backup")
    }

    fn preserve_corrupt_candidates(&self, candidates: &[(PathBuf, &'static str)]) -> AppResult<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        for (index, (path, path_kind)) in candidates.iter().enumerate() {
            self.maybe_fail(
                FailurePointName::PreserveCorrupt,
                "preserve_corrupt",
                path_kind,
            )?;
            let evidence_path = sibling_path(path, &format!(".corrupt-{timestamp}-{index}"));
            match fs::rename(path, &evidence_path) {
                Ok(()) => {
                    if let Err(error) = sync_parent_directory(&evidence_path) {
                        tracing::warn!(
                            path_kind,
                            error_code = storage_error_code(&error),
                            "notification corrupt evidence directory sync failed"
                        );
                    }
                }
                Err(rename_error) => match fs::copy(path, &evidence_path) {
                    Ok(_) => {}
                    Err(copy_error) => {
                        tracing::warn!(
                            path_kind,
                            rename_error_kind = io_error_kind(&rename_error),
                            copy_error_kind = io_error_kind(&copy_error),
                            "notification corrupt evidence preservation failed"
                        );
                        return Err(storage_unavailable_without_io(
                            "preserve_corrupt",
                            path_kind,
                        ));
                    }
                },
            }
        }
        Ok(())
    }

    fn lock_writes(&self) -> AppResult<std::sync::MutexGuard<'_, ()>> {
        self.write_lock
            .lock()
            .map_err(|_| storage_unavailable_without_io("lock", "store"))
    }

    fn maybe_fail(
        &self,
        point: FailurePointName,
        operation: &'static str,
        path_kind: &'static str,
    ) -> AppResult<()> {
        #[cfg(test)]
        {
            let configured = match point {
                FailurePointName::PromoteTemp => FailurePoint::PromoteTemp,
                FailurePointName::RestoreBackup => FailurePoint::RestoreBackup,
                FailurePointName::PreserveCorrupt => FailurePoint::PreserveCorrupt,
            };
            if self.failures.contains(&configured) {
                return Err(storage_unavailable(
                    operation,
                    path_kind,
                    std::io::Error::other("injected notification store failure"),
                ));
            }
        }
        #[cfg(not(test))]
        let _ = (point, operation, path_kind);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn path_for_test(&self) -> &Path {
        &self.path
    }

    #[cfg(test)]
    pub(crate) fn temp_path_for_test(path: &Path) -> PathBuf {
        sibling_path(path, TEMP_SUFFIX)
    }

    #[cfg(test)]
    pub(crate) fn backup_path_for_test(path: &Path) -> PathBuf {
        sibling_path(path, BACKUP_SUFFIX)
    }
}

impl NotificationPersistence for NotificationStore {
    fn save(&self, file: &NotificationFileV1) -> AppResult<()> {
        NotificationStore::save(self, file)
    }
}

impl Default for NotificationStore {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
enum FailurePointName {
    PromoteTemp,
    RestoreBackup,
    PreserveCorrupt,
}

struct NotificationPaths {
    main: PathBuf,
    temp: PathBuf,
    backup: PathBuf,
}

impl NotificationPaths {
    fn new(main: &Path) -> Self {
        Self {
            main: main.to_path_buf(),
            temp: sibling_path(main, TEMP_SUFFIX),
            backup: sibling_path(main, BACKUP_SUFFIX),
        }
    }
}

enum Candidate {
    Valid(NotificationFileV1),
    Future(u32),
    Corrupt,
}

fn existing_candidate(path: &Path, path_kind: &'static str) -> AppResult<Option<Candidate>> {
    match fs::symlink_metadata(path) {
        Ok(_) => read_candidate(path)
            .map(Some)
            .map_err(|error| storage_unavailable("read", path_kind, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(storage_unavailable("inspect", path_kind, error)),
    }
}

fn read_candidate(path: &Path) -> std::io::Result<Candidate> {
    let bytes = fs::read(path)?;
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Ok(Candidate::Corrupt);
    };
    let Some(schema_version) = value.get("schemaVersion").and_then(|value| value.as_u64()) else {
        return Ok(Candidate::Corrupt);
    };
    let Ok(schema_version) = u32::try_from(schema_version) else {
        return Ok(Candidate::Corrupt);
    };
    if schema_version > NOTIFICATION_SCHEMA_VERSION {
        return Ok(Candidate::Future(schema_version));
    }
    if schema_version != NOTIFICATION_SCHEMA_VERSION {
        return Ok(Candidate::Corrupt);
    }
    let Ok(file) = serde_json::from_value::<NotificationFileV1>(value) else {
        return Ok(Candidate::Corrupt);
    };
    if validate_file(&file).is_err() {
        Ok(Candidate::Corrupt)
    } else {
        Ok(Candidate::Valid(file))
    }
}

fn ensure_supported_save_target(paths: &NotificationPaths) -> AppResult<()> {
    for (path, path_kind) in [
        (&paths.main, "main"),
        (&paths.temp, "temp"),
        (&paths.backup, "backup"),
    ] {
        match existing_candidate(path, path_kind)? {
            None | Some(Candidate::Corrupt) => continue,
            Some(Candidate::Valid(_)) => return Ok(()),
            Some(Candidate::Future(_)) => {
                return Err(storage_unavailable_without_io("future_schema", path_kind));
            }
        }
    }
    Ok(())
}

fn validate_file(file: &NotificationFileV1) -> AppResult<()> {
    if file.schema_version != NOTIFICATION_SCHEMA_VERSION {
        return Err(invalid_file());
    }
    let mut scopes = HashSet::new();
    let mut record_ids = HashSet::new();
    for partition in &file.partitions {
        if partition.items.len() > MAX_NOTIFICATION_PARTITION_ITEMS {
            return Err(invalid_file());
        }
        partition.scope.validate().map_err(|_| invalid_file())?;
        if !scopes.insert(partition.scope.clone()) {
            return Err(invalid_file());
        }
        for record in &partition.items {
            record.validate().map_err(|_| invalid_file())?;
            if record.scope != partition.scope || !record_ids.insert(record.id.clone()) {
                return Err(invalid_file());
            }
        }
    }
    Ok(())
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn write_and_sync(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(|error| storage_unavailable("open", "temp", error))?;
    file.write_all(bytes)
        .map_err(|error| storage_unavailable("write", "temp", error))?;
    file.sync_all()
        .map_err(|error| storage_unavailable("sync", "temp", error))
}

fn remove_if_present(path: &Path, path_kind: &'static str) -> AppResult<()> {
    match fs::remove_file(path) {
        Ok(()) => sync_parent_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_unavailable("remove", path_kind, error)),
    }
}

fn rename_and_sync(from: &Path, to: &Path, operation: &'static str) -> AppResult<()> {
    fs::rename(from, to)
        .map_err(|error| storage_unavailable(operation, "notification_file", error))?;
    sync_parent_directory(to)
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| storage_unavailable("sync", "parent", error))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> AppResult<()> {
    Ok(())
}

fn invalid_file() -> AppError {
    AppError::Storage(INVALID_NOTIFICATION_FILE.into())
}

fn storage_unavailable(
    operation: &'static str,
    path_kind: &'static str,
    error: std::io::Error,
) -> AppError {
    log_storage_failure(operation, path_kind, &error);
    AppError::Storage(NOTIFICATION_STORAGE_UNAVAILABLE.into())
}

fn storage_unavailable_without_io(operation: &'static str, path_kind: &'static str) -> AppError {
    tracing::warn!(
        operation,
        path_kind,
        error_kind = "unavailable",
        "notification storage operation failed"
    );
    AppError::Storage(NOTIFICATION_STORAGE_UNAVAILABLE.into())
}

fn log_storage_failure(operation: &'static str, path_kind: &'static str, error: &std::io::Error) {
    tracing::warn!(
        operation,
        path_kind,
        error_kind = io_error_kind(error),
        "notification storage operation failed"
    );
}

fn io_error_kind(error: &std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::NotFound => "not_found",
        std::io::ErrorKind::PermissionDenied => "permission_denied",
        std::io::ErrorKind::AlreadyExists => "already_exists",
        std::io::ErrorKind::InvalidData => "invalid_data",
        std::io::ErrorKind::InvalidInput => "invalid_input",
        std::io::ErrorKind::WriteZero => "write_zero",
        std::io::ErrorKind::Interrupted => "interrupted",
        std::io::ErrorKind::UnexpectedEof => "unexpected_eof",
        _ => "other",
    }
}

fn storage_error_code(error: &AppError) -> &'static str {
    match error {
        AppError::Storage(message) if message == NOTIFICATION_STORAGE_UNAVAILABLE => {
            NOTIFICATION_STORAGE_UNAVAILABLE
        }
        AppError::Storage(message) if message == INVALID_NOTIFICATION_FILE => {
            INVALID_NOTIFICATION_FILE
        }
        _ => "NOTIFICATION_STORAGE_ERROR",
    }
}

fn recovery_source_name(source: RecoverySource) -> &'static str {
    match source {
        RecoverySource::Temp => "temp",
        RecoverySource::Backup => "backup",
    }
}

#[cfg(test)]
mod tests;
