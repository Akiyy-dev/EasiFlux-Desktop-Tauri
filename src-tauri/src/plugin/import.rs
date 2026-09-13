use std::{future::Future, path::PathBuf, pin::Pin};

use serde::{ser::SerializeMap, Serialize, Serializer};

use crate::error::{AppError, AppResult};
use crate::plugin::{
    manifest::{PluginCatalogSnapshot, PluginManifestV1},
    record::PluginRecord,
};

pub(crate) mod dialog;
mod session;
pub(crate) mod source;
pub(crate) use session::{CommitLease, ImportSessions, PrepareLease};
pub(crate) use source::{LocalManifestReader, SystemLocalManifestReader};

/// Only captured, validated content survives source selection. No path is retained.
#[derive(Clone)]
pub(crate) struct PreparedManifest {
    record: PluginRecord,
    bytes: Vec<u8>,
}

impl PreparedManifest {
    pub(crate) fn parse(bytes: &[u8]) -> AppResult<Self> {
        if bytes.len() > 16_384 || bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            return Err(invalid_manifest());
        }
        let manifest: PluginManifestV1 =
            serde_json::from_slice(bytes).map_err(|_| invalid_manifest())?;
        let record = PluginRecord::local_declarative(manifest).map_err(|_| invalid_manifest())?;
        let bytes = record
            .canonical_manifest_bytes()
            .map_err(|_| invalid_manifest())?;
        if bytes.len() > 16_384 {
            return Err(invalid_manifest());
        }
        Ok(Self { record, bytes })
    }

    pub(crate) fn record(&self) -> &PluginRecord {
        &self.record
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

fn invalid_manifest() -> AppError {
    AppError::Plugin {
        code: "plugin_import_manifest_invalid",
        message: "清单格式或内容不符合当前插件要求。",
        diagnostic: None,
    }
}

// Deliberately neither Debug nor Serialize: native source paths never enter IPC/logging.
pub(crate) enum SelectedManifestSource {
    Cancelled,
    Selected(PathBuf),
}

pub(crate) trait LocalManifestSelector: Send + Sync {
    fn select(
        &self,
    ) -> Pin<Box<dyn Future<Output = AppResult<SelectedManifestSource>> + Send + '_>>;
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportPreview {
    schema_version: u8,
    status: &'static str,
    pub(crate) token: String,
    expires_in_seconds: u16,
    pub(crate) catalog_generation: String,
    pub(crate) manifest: PluginManifestV1,
}

impl ImportPreview {
    fn new(token: String, generation: u64, manifest: PluginManifestV1) -> Self {
        Self {
            schema_version: 1,
            status: "ready",
            token,
            expires_in_seconds: 300,
            catalog_generation: generation.to_string(),
            manifest,
        }
    }
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum PrepareImportResult {
    Cancelled(CancelImportResult),
    Ready(ImportPreview),
}

impl PrepareImportResult {
    pub(crate) fn cancelled() -> Self {
        Self::Cancelled(CancelImportResult::new())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CancelImportResult {
    schema_version: u8,
    status: &'static str,
}

impl CancelImportResult {
    pub(crate) fn new() -> Self {
        Self {
            schema_version: 1,
            status: "cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ImportCommitFailure {
    CatalogStale,
    CatalogInvalid,
    CatalogGenerationExhausted,
    StateUnavailable,
    StatePersistFailed,
    StateCapacityExceeded,
    RevisionExhausted,
    OwnershipUnavailable,
    OwnershipCapacityExceeded,
    OwnershipRevisionExhausted,
    IdConflict,
    DiscoveryUnavailable,
    CapacityExceeded,
    StagingCapacityExceeded,
    WriteFailed,
}

impl ImportCommitFailure {
    pub(crate) fn as_code(self) -> &'static str {
        match self {
            Self::CatalogStale => "plugin_catalog_stale",
            Self::CatalogInvalid => "plugin_catalog_invalid",
            Self::CatalogGenerationExhausted => "plugin_catalog_generation_exhausted",
            Self::StateUnavailable => "plugin_state_unavailable",
            Self::StatePersistFailed => "plugin_state_persist_failed",
            Self::StateCapacityExceeded => "plugin_state_capacity_exceeded",
            Self::RevisionExhausted => "plugin_revision_exhausted",
            Self::OwnershipUnavailable => "plugin_ownership_unavailable",
            Self::OwnershipCapacityExceeded => "plugin_ownership_capacity_exceeded",
            Self::OwnershipRevisionExhausted => "plugin_ownership_revision_exhausted",
            Self::IdConflict => "plugin_import_id_conflict",
            Self::DiscoveryUnavailable => "plugin_import_discovery_unavailable",
            Self::CapacityExceeded => "plugin_import_capacity_exceeded",
            Self::StagingCapacityExceeded => "plugin_import_staging_capacity_exceeded",
            Self::WriteFailed => "plugin_import_write_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ImportPostCommitReason {
    OwnershipNotRegistered,
    PublicationUnconfirmed,
}

impl ImportPostCommitReason {
    fn as_code(self) -> &'static str {
        match self {
            Self::OwnershipNotRegistered => "plugin_import_ownership_not_registered",
            Self::PublicationUnconfirmed => "plugin_import_publication_unconfirmed",
        }
    }
}

pub(crate) enum CommitImportResult {
    Imported {
        plugin_id: String,
        snapshot: PluginCatalogSnapshot,
    },
    NotImported {
        disabled_decision_saved: bool,
        reason_code: ImportCommitFailure,
        snapshot: PluginCatalogSnapshot,
    },
    ImportedNotVisible {
        plugin_id: String,
        snapshot: PluginCatalogSnapshot,
    },
    ImportedExternal {
        plugin_id: String,
        snapshot: PluginCatalogSnapshot,
    },
}

impl CommitImportResult {
    pub(crate) fn imported_external(plugin_id: String, snapshot: PluginCatalogSnapshot) -> Self {
        Self::ImportedExternal {
            plugin_id,
            snapshot,
        }
    }
    pub(crate) fn imported(plugin_id: String, snapshot: PluginCatalogSnapshot) -> Self {
        Self::Imported {
            plugin_id,
            snapshot,
        }
    }

    pub(crate) fn not_imported(
        reason_code: ImportCommitFailure,
        disabled_decision_saved: bool,
        snapshot: PluginCatalogSnapshot,
    ) -> Self {
        Self::NotImported {
            disabled_decision_saved,
            reason_code,
            snapshot,
        }
    }

    pub(crate) fn imported_not_visible(plugin_id: String, snapshot: PluginCatalogSnapshot) -> Self {
        Self::ImportedNotVisible {
            plugin_id,
            snapshot,
        }
    }
}

// The envelope schema and publication reason are constants, not caller-provided fields.
impl Serialize for CommitImportResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let fields = if matches!(self, Self::Imported { .. }) {
            4
        } else {
            5
        };
        let mut map = serializer.serialize_map(Some(fields))?;
        map.serialize_entry("schemaVersion", &2u8)?;
        match self {
            Self::Imported {
                plugin_id,
                snapshot,
            } => {
                map.serialize_entry("status", "imported")?;
                map.serialize_entry("pluginId", plugin_id)?;
                map.serialize_entry("snapshot", snapshot)?;
            }
            Self::NotImported {
                disabled_decision_saved,
                reason_code,
                snapshot,
            } => {
                map.serialize_entry("status", "notImported")?;
                map.serialize_entry("disabledDecisionSaved", disabled_decision_saved)?;
                map.serialize_entry("reasonCode", reason_code.as_code())?;
                map.serialize_entry("snapshot", snapshot)?;
            }
            Self::ImportedNotVisible {
                plugin_id,
                snapshot,
            } => {
                map.serialize_entry("status", "importedNotVisible")?;
                map.serialize_entry("pluginId", plugin_id)?;
                map.serialize_entry(
                    "reasonCode",
                    ImportPostCommitReason::PublicationUnconfirmed.as_code(),
                )?;
                map.serialize_entry("snapshot", snapshot)?;
            }
            Self::ImportedExternal {
                plugin_id,
                snapshot,
            } => {
                map.serialize_entry("status", "importedExternal")?;
                map.serialize_entry("pluginId", plugin_id)?;
                map.serialize_entry(
                    "reasonCode",
                    ImportPostCommitReason::OwnershipNotRegistered.as_code(),
                )?;
                map.serialize_entry("snapshot", snapshot)?;
            }
        }
        map.end()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    pub(crate) const VALID: &[u8] = br#"{"schemaVersion":1,"id":"com.example.notes","publisherId":"com.example","publisher":"Example","name":"Notes","description":"Metadata only","version":"1.0.0","contributions":[],"requestedCapabilities":[]}"#;
}

#[cfg(test)]
mod tests;
