use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::models::config::APP_NAME;
use crate::plugin::manifest::{
    PluginId, PluginPublisherId, PluginSource, APPROVAL_FINGERPRINT_NONE,
};

use super::atomic_file::{read_bounded, AtomicFile, CandidateBytes};

pub(crate) const MAX_PLUGIN_STATE_BYTES: usize = 256 * 1024;
pub(crate) const MAX_PLUGIN_STATE_ENTRIES: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginStateFileV1 {
    pub schema_version: u32,
    #[serde(with = "decimal_revision")]
    pub revision: u64,
    pub entries: Vec<PluginStateEntryV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginStateEntryV1 {
    pub id: PluginId,
    pub source: PluginSource,
    pub publisher_id: PluginPublisherId,
    pub approval_fingerprint: String,
    pub enabled: bool,
}

impl PluginStateFileV1 {
    pub fn empty() -> Self {
        Self {
            schema_version: 1,
            revision: 0,
            entries: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1 {
            return Err("unsupported state schema");
        }
        if self.entries.len() > MAX_PLUGIN_STATE_ENTRIES {
            return Err("too many state entries");
        }
        let mut identities = BTreeSet::new();
        for entry in &self.entries {
            // Both identifier types can only be constructed through validated
            // parsing. Phase 0 accepts no external source or approval token.
            if entry.source != PluginSource::BuiltIn
                || entry.approval_fingerprint != APPROVAL_FINGERPRINT_NONE
            {
                return Err("invalid state trust identity");
            }
            // Source and fingerprint are fixed by v1. They are therefore implicit
            // in this tuple; a new source requires a schema migration, not a cast.
            if !identities.insert((&entry.id, &entry.publisher_id)) {
                return Err("duplicate state trust identity");
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StateWire {
    schema_version: u32,
    #[serde(with = "decimal_revision")]
    revision: u64,
    entries: Vec<PluginStateEntryV1>,
}

impl<'de> Deserialize<'de> for PluginStateFileV1 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = StateWire::deserialize(deserializer)?;
        let state = Self {
            schema_version: wire.schema_version,
            revision: wire.revision,
            entries: wire.entries,
        };
        state.validate().map_err(serde::de::Error::custom)?;
        Ok(state)
    }
}

mod decimal_revision {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(serde::de::Error::custom(
                "revision must be a decimal string",
            ));
        }
        value.parse().map_err(serde::de::Error::custom)
    }
}

pub(crate) trait PluginStatePersistence: Send + Sync {
    fn load(&self) -> AppResult<PluginStateFileV1>;
    fn save(&self, state: &PluginStateFileV1) -> AppResult<()>;
}

pub(crate) struct PluginStateStore {
    file: AtomicFile,
    transaction: Mutex<()>,
}

impl PluginStateStore {
    pub fn try_new() -> AppResult<Self> {
        let config_dir = dirs::config_dir().ok_or_else(unavailable)?;
        Ok(Self::with_path(
            config_dir.join(APP_NAME).join("plugins").join("state.json"),
        ))
    }

    fn with_path(path: PathBuf) -> Self {
        Self {
            file: AtomicFile::new(path),
            transaction: Mutex::new(()),
        }
    }

    fn load_locked(&self) -> AppResult<Option<PluginStateFileV1>> {
        let mut invalid = false;
        for (index, path) in [&self.file.main, &self.file.temp(), &self.file.backup()]
            .into_iter()
            .enumerate()
        {
            let bytes = match read_bounded(path, MAX_PLUGIN_STATE_BYTES).map_err(io_error)? {
                CandidateBytes::Missing => continue,
                CandidateBytes::Oversized if index == 0 => {
                    // We cannot safely identify an oversized main's schema
                    // without parsing beyond the bound. Never downgrade it.
                    return Err(unavailable());
                }
                CandidateBytes::Oversized => {
                    invalid = true;
                    continue;
                }
                CandidateBytes::Bounded(bytes) => bytes,
            };
            // Sniff only bounded bytes, and do not require the future document
            // to conform to today's layout. Deserialize from the original bytes
            // below so duplicate JSON fields are not silently collapsed.
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if value
                    .get("schemaVersion")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|version| version > 1)
                {
                    return Err(AppError::Plugin {
                        code: "plugin_state_unsupported_schema",
                        message: "插件状态版本暂不受支持",
                        diagnostic: None,
                    });
                }
            }
            match serde_json::from_slice::<PluginStateFileV1>(&bytes) {
                Ok(state) => return Ok(Some(state)),
                Err(error) => {
                    tracing::warn!(error = %error, candidate = index, "invalid plugin state candidate");
                    invalid = true;
                }
            }
        }
        if invalid {
            Err(unavailable())
        } else {
            Ok(None)
        }
    }
}

impl PluginStatePersistence for PluginStateStore {
    fn load(&self) -> AppResult<PluginStateFileV1> {
        let _guard = self.transaction.lock().map_err(|_| unavailable())?;
        Ok(self.load_locked()?.unwrap_or_else(PluginStateFileV1::empty))
    }

    fn save(&self, state: &PluginStateFileV1) -> AppResult<()> {
        let _guard = self.transaction.lock().map_err(|_| unavailable())?;
        state.validate().map_err(|_| invalid_state())?;
        let bytes = serde_json::to_vec(state).map_err(|_| invalid_state())?;
        if bytes.len() > MAX_PLUGIN_STATE_BYTES {
            return Err(invalid_state());
        }
        let previous = self.load_locked()?;
        let previous = previous
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| invalid_state())?;
        self.file
            .replace(&bytes, previous.as_deref())
            .map_err(io_error)
    }
}

fn unavailable() -> AppError {
    AppError::Plugin {
        code: "plugin_state_unavailable",
        message: "插件状态存储暂不可用",
        diagnostic: None,
    }
}

fn invalid_state() -> AppError {
    AppError::Plugin {
        code: "plugin_state_invalid",
        message: "插件状态数据无效",
        diagnostic: None,
    }
}

fn io_error(error: std::io::Error) -> AppError {
    tracing::warn!(error = %error, "plugin state storage operation failed");
    unavailable()
}

#[cfg(test)]
mod tests;
