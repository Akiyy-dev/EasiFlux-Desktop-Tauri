use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

const TEMP_SUFFIX: &str = ".tmp";
const BACKUP_SUFFIX: &str = ".bak";

struct ConfigPaths {
    main: PathBuf,
    temp: PathBuf,
    backup: PathBuf,
}

impl ConfigPaths {
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

pub(super) fn load<T, F>(path: &Path, parse: F) -> AppResult<Option<T>>
where
    F: Fn(&str) -> AppResult<T>,
{
    let paths = ConfigPaths::new(path);
    let candidates = [
        ("主配置", paths.main.as_path()),
        ("临时副本", paths.temp.as_path()),
        ("备份", paths.backup.as_path()),
    ];
    let mut invalid_candidates = Vec::new();

    for (label, candidate) in candidates {
        match fs::symlink_metadata(candidate) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                invalid_candidates.push(label);
                continue;
            }
        }
        let text = match fs::read_to_string(candidate) {
            Ok(text) => text,
            Err(_) => {
                invalid_candidates.push(label);
                continue;
            }
        };
        match parse(&text) {
            Ok(config) => return Ok(Some(config)),
            Err(_) => invalid_candidates.push(label),
        }
    }

    if invalid_candidates.is_empty() {
        Ok(None)
    } else {
        Err(AppError::Config(format!(
            "配置文件及恢复副本均不可用：{}",
            invalid_candidates.join("、")
        )))
    }
}

pub(super) fn save(path: &Path, contents: &[u8]) -> AppResult<()> {
    let paths = ConfigPaths::new(path);
    if let Some(parent) = paths
        .main
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    write_temp(&paths.temp, contents)?;
    replace_main(&paths)
}

fn write_temp(path: &Path, contents: &[u8]) -> AppResult<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

fn replace_main(paths: &ConfigPaths) -> AppResult<()> {
    match fs::metadata(&paths.main) {
        Ok(metadata) if !metadata.is_file() => {
            Err(AppError::Storage("配置主路径不是普通文件".into()))
        }
        Ok(_) => replace_existing_main(paths),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::rename(&paths.temp, &paths.main)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn replace_existing_main(paths: &ConfigPaths) -> AppResult<()> {
    remove_old_backup(&paths.backup)?;
    fs::rename(&paths.main, &paths.backup)?;

    if let Err(replace_error) = fs::rename(&paths.temp, &paths.main) {
        if let Err(restore_error) = fs::rename(&paths.backup, &paths.main) {
            tracing::warn!(
                replace_error = %replace_error,
                restore_error = %restore_error,
                "替换配置文件失败，且无法恢复原主配置文件"
            );
        }
        return Err(replace_error.into());
    }

    Ok(())
}

fn remove_old_backup(path: &Path) -> AppResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
