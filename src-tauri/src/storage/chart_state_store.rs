use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[cfg(test)]
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::models::chart_workspace::{
    ChartPreferencesV1, ChartViewStateV1, ChartWorkspaceKey, CHART_WORKSPACE_SCHEMA_VERSION,
};
use crate::models::config::APP_NAME;

const PREFERENCES_FILENAME: &str = "preferences.v1.json";
const TEMP_SUFFIX: &str = ".tmp";
const BACKUP_SUFFIX: &str = ".bak";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StateFileKind {
    View(ChartWorkspaceKey),
    Preferences,
}

pub struct ChartStateStore {
    root: PathBuf,
    write_lock: Mutex<()>,
    #[cfg(test)]
    write_hook: Option<Arc<dyn Fn(StateFileKind) + Send + Sync>>,
}

impl ChartStateStore {
    pub fn new() -> Self {
        let root = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(APP_NAME)
            .join("chart_workspace");
        Self::with_root(root)
    }

    pub(crate) fn with_root(root: PathBuf) -> Self {
        Self {
            root,
            write_lock: Mutex::new(()),
            #[cfg(test)]
            write_hook: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root_and_write_hook(
        root: PathBuf,
        write_hook: Arc<dyn Fn(StateFileKind) + Send + Sync>,
    ) -> Self {
        Self {
            root,
            write_lock: Mutex::new(()),
            write_hook: Some(write_hook),
        }
    }

    pub fn load_view(&self, key: &ChartWorkspaceKey) -> AppResult<Option<ChartViewStateV1>> {
        let path = self.view_path(key);
        load_candidates(&path, |state: &ChartViewStateV1| validate_view(state, key))
    }

    pub fn save_view(&self, state: &ChartViewStateV1) -> AppResult<()> {
        let key =
            ChartWorkspaceKey::parse(&state.symbol, &state.interval).map_err(AppError::Storage)?;
        validate_view(state, &key)?;
        let _guard = self
            .write_lock
            .lock()
            .map_err(|error| AppError::Storage(error.to_string()))?;
        self.save_value(
            &self.view_path(&key),
            state,
            |candidate: &ChartViewStateV1| validate_view(candidate, &key),
            #[cfg(test)]
            StateFileKind::View(key.clone()),
        )
    }

    pub fn load_preferences(&self) -> AppResult<Option<ChartPreferencesV1>> {
        load_candidates(&self.preferences_path(), validate_preferences)
    }

    pub fn save_preferences(&self, preferences: &ChartPreferencesV1) -> AppResult<()> {
        validate_preferences(preferences)?;
        let _guard = self
            .write_lock
            .lock()
            .map_err(|error| AppError::Storage(error.to_string()))?;
        self.save_value(
            &self.preferences_path(),
            preferences,
            validate_preferences,
            #[cfg(test)]
            StateFileKind::Preferences,
        )
    }

    fn save_value<T, F>(
        &self,
        main_path: &Path,
        value: &T,
        validate: F,
        #[cfg(test)] kind: StateFileKind,
    ) -> AppResult<()>
    where
        T: DeserializeOwned + Serialize,
        F: Fn(&T) -> AppResult<()>,
    {
        let paths = StatePaths::new(main_path);
        let main_is_valid = candidate_is_valid(&paths.main, &validate);
        let backup_is_valid = candidate_is_valid(&paths.backup, &validate);
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|error| AppError::Storage(error.to_string()))?;

        if let Some(parent) = paths.main.parent() {
            fs::create_dir_all(parent).map_err(storage_error)?;
        }
        #[cfg(test)]
        if let Some(hook) = &self.write_hook {
            hook(kind);
        }
        write_and_sync(&paths.temp, &bytes)?;

        if main_is_valid {
            remove_old_backup_if_present(&paths.backup)?;
            rename_and_sync(&paths.main, &paths.backup)?;
        } else {
            remove_invalid_main_if_present(&paths.main)?;
        }

        rename_temp_to_main_or_restore_backup(
            &paths.temp,
            &paths.main,
            &paths.backup,
            main_is_valid || backup_is_valid,
        )
    }

    fn view_path(&self, key: &ChartWorkspaceKey) -> PathBuf {
        self.root
            .join("views")
            .join(format!("{}_{}.v1.json", key.symbol, key.interval))
    }

    fn preferences_path(&self) -> PathBuf {
        self.root.join(PREFERENCES_FILENAME)
    }

    #[cfg(test)]
    pub(crate) fn view_path_for_test(&self, key: &ChartWorkspaceKey) -> PathBuf {
        self.view_path(key)
    }

    #[cfg(test)]
    pub(crate) fn view_temp_path_for_test(&self, key: &ChartWorkspaceKey) -> PathBuf {
        StatePaths::new(&self.view_path(key)).temp
    }

    #[cfg(test)]
    pub(crate) fn view_backup_path_for_test(&self, key: &ChartWorkspaceKey) -> PathBuf {
        StatePaths::new(&self.view_path(key)).backup
    }

    #[cfg(test)]
    pub(crate) fn preferences_path_for_test(&self) -> PathBuf {
        self.preferences_path()
    }

    #[cfg(test)]
    pub(crate) fn load_backup_view_for_test(
        &self,
        key: &ChartWorkspaceKey,
    ) -> AppResult<ChartViewStateV1> {
        let path = self.view_backup_path_for_test(key);
        read_candidate(&path, |state: &ChartViewStateV1| validate_view(state, key))
    }
}

impl Default for ChartStateStore {
    fn default() -> Self {
        Self::new()
    }
}

struct StatePaths {
    main: PathBuf,
    temp: PathBuf,
    backup: PathBuf,
}

impl StatePaths {
    fn new(main: &Path) -> Self {
        Self {
            main: main.to_path_buf(),
            temp: sibling_path(main, TEMP_SUFFIX),
            backup: sibling_path(main, BACKUP_SUFFIX),
        }
    }
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn load_candidates<T, F>(main_path: &Path, validate: F) -> AppResult<Option<T>>
where
    T: DeserializeOwned,
    F: Fn(&T) -> AppResult<()>,
{
    let paths = StatePaths::new(main_path);
    let mut found_candidate = false;

    for path in [&paths.main, &paths.temp, &paths.backup] {
        match fs::symlink_metadata(path) {
            Ok(_) => found_candidate = true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                found_candidate = true;
                continue;
            }
        }
        if let Ok(value) = read_candidate(path, &validate) {
            return Ok(Some(value));
        }
    }

    if found_candidate {
        Err(AppError::Storage(format!(
            "no valid chart state candidate for {}",
            main_path.display()
        )))
    } else {
        Ok(None)
    }
}

fn read_candidate<T, F>(path: &Path, validate: F) -> AppResult<T>
where
    T: DeserializeOwned,
    F: Fn(&T) -> AppResult<()>,
{
    let bytes = fs::read(path).map_err(storage_error)?;
    let value =
        serde_json::from_slice(&bytes).map_err(|error| AppError::Storage(error.to_string()))?;
    validate(&value)?;
    Ok(value)
}

fn candidate_is_valid<T, F>(path: &Path, validate: F) -> bool
where
    T: DeserializeOwned,
    F: Fn(&T) -> AppResult<()>,
{
    read_candidate(path, validate).is_ok()
}

fn validate_view(state: &ChartViewStateV1, key: &ChartWorkspaceKey) -> AppResult<()> {
    validate_schema(state.schema_version)?;
    if state.symbol != key.symbol || state.interval != key.interval {
        return Err(AppError::Storage(format!(
            "chart state key mismatch: expected {}_{}, found {}_{}",
            key.symbol, key.interval, state.symbol, state.interval
        )));
    }
    Ok(())
}

fn validate_preferences(preferences: &ChartPreferencesV1) -> AppResult<()> {
    validate_schema(preferences.schema_version)
}

fn validate_schema(schema_version: u32) -> AppResult<()> {
    if schema_version == CHART_WORKSPACE_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(AppError::Storage(format!(
            "unsupported chart state schema version {schema_version}"
        )))
    }
}

fn write_and_sync(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .map_err(storage_error)?;
    file.write_all(bytes).map_err(storage_error)?;
    file.sync_all().map_err(storage_error)
}

fn remove_old_backup_if_present(path: &Path) -> AppResult<()> {
    match fs::remove_file(path) {
        Ok(()) => sync_parent_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error(error)),
    }
}

fn remove_invalid_main_if_present(path: &Path) -> AppResult<()> {
    match fs::remove_file(path) {
        Ok(()) => sync_parent_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_error(error)),
    }
}

fn rename_temp_to_main_or_restore_backup(
    temp_path: &Path,
    main_path: &Path,
    backup_path: &Path,
    backup_is_valid: bool,
) -> AppResult<()> {
    if let Err(replace_error) = rename_and_sync(temp_path, main_path) {
        if backup_is_valid {
            if let Err(restore_error) = rename_and_sync(backup_path, main_path) {
                tracing::warn!(
                    replace_error = %replace_error,
                    restore_error = %restore_error,
                    "failed to promote chart state temp file and restore backup"
                );
            }
        }
        return Err(replace_error);
    }
    Ok(())
}

fn rename_and_sync(from: &Path, to: &Path) -> AppResult<()> {
    fs::rename(from, to).map_err(storage_error)?;
    sync_parent_directory(to)
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(storage_error)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> AppResult<()> {
    Ok(())
}

fn storage_error(error: std::io::Error) -> AppError {
    AppError::Storage(error.to_string())
}

#[cfg(test)]
mod tests;
