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
        if value.is_empty()
            || (value.len() > 1 && value.starts_with('0'))
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(serde::de::Error::custom(
                "revision must be a canonical decimal string",
            ));
        }
        value.parse().map_err(serde::de::Error::custom)
    }
}

/// Inspect only a top-level schema field while still validating the entire JSON
/// envelope. serde_json skips IgnoredAny values with an iterative stack, so an
/// unknown future layout does not hit Value deserialization's recursion limit.
/// The caller has already bounded the bytes, which also bounds the skip stack.
struct StateSchema(u64);

impl<'de> Deserialize<'de> for StateSchema {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SchemaVisitor;

        impl<'de> serde::de::Visitor<'de> for SchemaVisitor {
            type Value = StateSchema;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a state object with one integer schemaVersion")
            }

            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut schema = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key == "schemaVersion" {
                        if schema.is_some() {
                            return Err(serde::de::Error::duplicate_field("schemaVersion"));
                        }
                        schema = Some(map.next_value::<u64>()?);
                    } else {
                        map.next_value::<serde::de::IgnoredAny>()?;
                    }
                }
                schema
                    .map(StateSchema)
                    .ok_or_else(|| serde::de::Error::missing_field("schemaVersion"))
            }
        }

        // deserialize_map deliberately rejects positional arrays, unlike a
        // derived struct visitor that may also implement visit_seq.
        deserializer.deserialize_map(SchemaVisitor)
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
            // from_slice also checks trailing data. Never interpret a malformed
            // or duplicate schema header as a valid future version.
            let schema = match serde_json::from_slice::<StateSchema>(&bytes) {
                Ok(schema) => schema.0,
                Err(error) => {
                    tracing::warn!(error = %error, candidate = index, "invalid plugin state header");
                    invalid = true;
                    continue;
                }
            };
            if schema > 1 {
                return Err(AppError::Plugin {
                    code: "plugin_state_unsupported_schema",
                    message: "插件状态版本暂不受支持",
                    diagnostic: None,
                });
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
