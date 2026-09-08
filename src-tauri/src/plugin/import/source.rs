use std::path::Path;

use super::PreparedManifest;
use crate::error::{AppError, AppResult};
use crate::plugin::discovery::safe_fs;

pub(crate) trait LocalManifestReader: Send + Sync {
    fn read(&self, path: &Path) -> AppResult<PreparedManifest>;
}

pub(crate) struct SystemLocalManifestReader;

impl LocalManifestReader for SystemLocalManifestReader {
    fn read(&self, path: &Path) -> AppResult<PreparedManifest> {
        let bytes = safe_fs::read_manifest_source(path).map_err(|_| source_rejected())?;
        PreparedManifest::parse(&bytes)
    }
}

fn source_rejected() -> AppError {
    AppError::Plugin {
        code: "plugin_import_source_rejected",
        message: "无法安全读取所选文件，请选择普通本地 JSON 文件。",
        diagnostic: None,
    }
}

#[cfg(test)]
mod tests;
