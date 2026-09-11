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
pub(crate) struct PluginStateFileV2 {
    pub schema_version: u32,
    #[serde(with = "decimal_revision")]
    pub revision: u64,
    pub entries: Vec<PluginStateEntryV2>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PluginStateEntryV2 {
    pub id: PluginId,
    pub source: PluginSource,
    pub publisher_id: PluginPublisherId,
    pub approval_fingerprint: String,
    pub enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginStateEntryV2Dto {
    id: PluginId,
    source: PluginSource,
    publisher_id: PluginPublisherId,
    approval_fingerprint: String,
    enabled: bool,
}

impl<'de> Deserialize<'de> for PluginStateEntryV2 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntryVisitor;
        impl<'de> serde::de::Visitor<'de> for EntryVisitor {
            type Value = PluginStateEntryV2;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a plugin state entry object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                map: M,
            ) -> Result<Self::Value, M::Error> {
                let dto = PluginStateEntryV2Dto::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                Ok(PluginStateEntryV2 {
                    id: dto.id,
                    source: dto.source,
                    publisher_id: dto.publisher_id,
                    approval_fingerprint: dto.approval_fingerprint,
                    enabled: dto.enabled,
                })
            }
        }
        deserializer.deserialize_map(EntryVisitor)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginStateFileV2Dto {
    schema_version: u32,
    #[serde(with = "decimal_revision")]
    revision: u64,
    entries: Vec<PluginStateEntryV2>,
}

impl<'de> Deserialize<'de> for PluginStateFileV2 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FileVisitor;
        impl<'de> serde::de::Visitor<'de> for FileVisitor {
            type Value = PluginStateFileV2;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a plugin state object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                map: M,
            ) -> Result<Self::Value, M::Error> {
                let dto = PluginStateFileV2Dto::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                let state = PluginStateFileV2 {
                    schema_version: dto.schema_version,
                    revision: dto.revision,
                    entries: dto.entries,
                };
                state.validate().map_err(serde::de::Error::custom)?;
                Ok(state)
            }
        }
        deserializer.deserialize_map(FileVisitor)
    }
}

impl PluginStateFileV2 {
    pub(crate) fn empty() -> Self {
        Self {
            schema_version: 2,
            revision: 0,
            entries: Vec::new(),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 2 {
            return Err("unsupported state schema");
        }
        if self.entries.len() > MAX_PLUGIN_STATE_ENTRIES {
            return Err("too many state entries");
        }
        let mut identities = BTreeSet::new();
        for entry in &self.entries {
            if !valid_fingerprint(entry.source, &entry.approval_fingerprint) {
                return Err("invalid state trust identity");
            }
            if !identities.insert((
                entry.id.clone(),
                entry.source,
                entry.publisher_id.clone(),
                entry.approval_fingerprint.clone(),
            )) {
                return Err("duplicate state trust identity");
            }
        }
        Ok(())
    }

    pub(crate) fn validate_for_persistence(&self) -> Result<(), &'static str> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| "state serialization failed")?;
        if bytes.len() > MAX_PLUGIN_STATE_BYTES {
            return Err("state exceeds byte limit");
        }
        Ok(())
    }
}

fn valid_fingerprint(source: PluginSource, fingerprint: &str) -> bool {
    match source {
        PluginSource::BuiltIn => fingerprint == APPROVAL_FINGERPRINT_NONE,
        PluginSource::LocalDeclarative => {
            fingerprint
                .strip_prefix("v1:sha256:")
                .is_some_and(|digest| {
                    digest.len() == 64
                        && digest
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                })
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
enum PluginStateV1Source {
    BuiltIn,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginStateEntryV1Dto {
    id: PluginId,
    source: PluginStateV1Source,
    publisher_id: PluginPublisherId,
    approval_fingerprint: String,
    enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PluginStateEntryV1Wire {
    id: PluginId,
    publisher_id: PluginPublisherId,
    approval_fingerprint: String,
    enabled: bool,
}

impl<'de> Deserialize<'de> for PluginStateEntryV1Wire {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntryVisitor;
        impl<'de> serde::de::Visitor<'de> for EntryVisitor {
            type Value = PluginStateEntryV1Wire;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a plugin state v1 entry object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                map: M,
            ) -> Result<Self::Value, M::Error> {
                let dto = PluginStateEntryV1Dto::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                let PluginStateV1Source::BuiltIn = dto.source;
                Ok(PluginStateEntryV1Wire {
                    id: dto.id,
                    publisher_id: dto.publisher_id,
                    approval_fingerprint: dto.approval_fingerprint,
                    enabled: dto.enabled,
                })
            }
        }
        deserializer.deserialize_map(EntryVisitor)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginStateFileV1Dto {
    schema_version: u32,
    #[serde(with = "decimal_revision")]
    revision: u64,
    entries: Vec<PluginStateEntryV1Wire>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PluginStateFileV1Wire {
    schema_version: u32,
    revision: u64,
    entries: Vec<PluginStateEntryV1Wire>,
}

impl<'de> Deserialize<'de> for PluginStateFileV1Wire {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FileVisitor;
        impl<'de> serde::de::Visitor<'de> for FileVisitor {
            type Value = PluginStateFileV1Wire;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a plugin state v1 object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                map: M,
            ) -> Result<Self::Value, M::Error> {
                let dto = PluginStateFileV1Dto::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                let state = PluginStateFileV1Wire {
                    schema_version: dto.schema_version,
                    revision: dto.revision,
                    entries: dto.entries,
                };
                state.validate().map_err(serde::de::Error::custom)?;
                Ok(state)
            }
        }
        deserializer.deserialize_map(FileVisitor)
    }
}

impl PluginStateFileV1Wire {
    fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1 {
            return Err("unsupported state schema");
        }
        if self.entries.len() > MAX_PLUGIN_STATE_ENTRIES {
            return Err("too many state entries");
        }
        let mut identities = BTreeSet::new();
        for entry in &self.entries {
            if entry.approval_fingerprint != APPROVAL_FINGERPRINT_NONE {
                return Err("invalid state trust identity");
            }
            if !identities.insert((&entry.id, &entry.publisher_id)) {
                return Err("duplicate state trust identity");
            }
        }
        Ok(())
    }

    fn migrate(self) -> PluginStateFileV2 {
        PluginStateFileV2 {
            schema_version: 2,
            revision: self.revision,
            entries: self
                .entries
                .into_iter()
                .map(|entry| PluginStateEntryV2 {
                    id: entry.id,
                    source: PluginSource::BuiltIn,
                    publisher_id: entry.publisher_id,
                    approval_fingerprint: entry.approval_fingerprint,
                    enabled: entry.enabled,
                })
                .collect(),
        }
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

/// Inspects only schemaVersion without materializing a future layout.
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
        deserializer.deserialize_map(SchemaVisitor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PluginStateLoad {
    pub(crate) state: PluginStateFileV2,
    pub(crate) requires_rewrite: bool,
}

pub(crate) trait PluginStatePersistence: Send + Sync {
    fn load(&self) -> AppResult<PluginStateLoad>;
    fn save(&self, state: &PluginStateFileV2) -> AppResult<()>;
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

    fn load_locked(&self) -> AppResult<Option<PluginStateLoad>> {
        let mut invalid = false;
        for (index, path) in [&self.file.main, &self.file.temp(), &self.file.backup()]
            .into_iter()
            .enumerate()
        {
            let bytes = match read_bounded(path, MAX_PLUGIN_STATE_BYTES).map_err(io_error)? {
                CandidateBytes::Missing => continue,
                CandidateBytes::Oversized if index == 0 => return Err(unavailable()),
                CandidateBytes::Oversized => {
                    invalid = true;
                    continue;
                }
                CandidateBytes::Bounded(bytes) => bytes,
            };
            let schema = match serde_json::from_slice::<StateSchema>(&bytes) {
                Ok(schema) => schema.0,
                Err(error) => {
                    tracing::warn!(error = %error, candidate = index, "invalid plugin state header");
                    invalid = true;
                    continue;
                }
            };
            if schema > 2 {
                return Err(unsupported_schema());
            }
            let decoded = match schema {
                1 => serde_json::from_slice::<PluginStateFileV1Wire>(&bytes).map(|state| {
                    PluginStateLoad {
                        state: state.migrate(),
                        requires_rewrite: true,
                    }
                }),
                2 => serde_json::from_slice::<PluginStateFileV2>(&bytes).map(|state| {
                    PluginStateLoad {
                        state,
                        requires_rewrite: false,
                    }
                }),
                _ => Err(serde_json::Error::io(std::io::Error::other(
                    "unsupported state schema",
                ))),
            };
            match decoded {
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
    fn load(&self) -> AppResult<PluginStateLoad> {
        let _guard = self.transaction.lock().map_err(|_| unavailable())?;
        Ok(self.load_locked()?.unwrap_or(PluginStateLoad {
            state: PluginStateFileV2::empty(),
            requires_rewrite: false,
        }))
    }
    fn save(&self, state: &PluginStateFileV2) -> AppResult<()> {
        let _guard = self.transaction.lock().map_err(|_| unavailable())?;
        state
            .validate_for_persistence()
            .map_err(|_| invalid_state())?;
        let bytes = serde_json::to_vec(state).map_err(|_| invalid_state())?;
        let previous = self
            .load_locked()?
            .as_ref()
            .map(|loaded| serde_json::to_vec(&loaded.state))
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
fn unsupported_schema() -> AppError {
    AppError::Plugin {
        code: "plugin_state_unsupported_schema",
        message: "插件状态版本暂不受支持",
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
