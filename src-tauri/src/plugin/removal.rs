use super::manifest::PluginCatalogSnapshot;
use serde::{ser::SerializeMap, Serialize, Serializer};

/// Closed pre-commit business failures. Publication uncertainty is not a failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoveManagedLocalPluginFailure {
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
    OwnershipConflict,
    DiscoveryUnavailable,
    NotManaged,
    RequiresDisabled,
    StorageUnavailable,
    IdentityChanged,
    StagingCapacityExceeded,
    OwnershipPersistFailed,
    RemoveWriteFailed,
}
impl RemoveManagedLocalPluginFailure {
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
            Self::OwnershipConflict => "plugin_ownership_conflict",
            Self::DiscoveryUnavailable => "plugin_remove_discovery_unavailable",
            Self::NotManaged => "plugin_remove_not_managed",
            Self::RequiresDisabled => "plugin_remove_requires_disabled",
            Self::StorageUnavailable => "plugin_remove_storage_unavailable",
            Self::IdentityChanged => "plugin_remove_identity_changed",
            Self::StagingCapacityExceeded => "plugin_remove_staging_capacity_exceeded",
            Self::OwnershipPersistFailed => "plugin_ownership_persist_failed",
            Self::RemoveWriteFailed => "plugin_remove_write_failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BeforeDisabledFailure {
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
    OwnershipConflict,
    DiscoveryUnavailable,
    NotManaged,
    RequiresDisabled,
    StorageUnavailable,
    IdentityChanged,
    StagingCapacityExceeded,
}
impl From<BeforeDisabledFailure> for RemoveManagedLocalPluginFailure {
    fn from(value: BeforeDisabledFailure) -> Self {
        match value {
            BeforeDisabledFailure::CatalogStale => Self::CatalogStale,
            BeforeDisabledFailure::CatalogInvalid => Self::CatalogInvalid,
            BeforeDisabledFailure::CatalogGenerationExhausted => Self::CatalogGenerationExhausted,
            BeforeDisabledFailure::StateUnavailable => Self::StateUnavailable,
            BeforeDisabledFailure::StatePersistFailed => Self::StatePersistFailed,
            BeforeDisabledFailure::StateCapacityExceeded => Self::StateCapacityExceeded,
            BeforeDisabledFailure::RevisionExhausted => Self::RevisionExhausted,
            BeforeDisabledFailure::OwnershipUnavailable => Self::OwnershipUnavailable,
            BeforeDisabledFailure::OwnershipCapacityExceeded => Self::OwnershipCapacityExceeded,
            BeforeDisabledFailure::OwnershipRevisionExhausted => Self::OwnershipRevisionExhausted,
            BeforeDisabledFailure::OwnershipConflict => Self::OwnershipConflict,
            BeforeDisabledFailure::DiscoveryUnavailable => Self::DiscoveryUnavailable,
            BeforeDisabledFailure::NotManaged => Self::NotManaged,
            BeforeDisabledFailure::RequiresDisabled => Self::RequiresDisabled,
            BeforeDisabledFailure::StorageUnavailable => Self::StorageUnavailable,
            BeforeDisabledFailure::IdentityChanged => Self::IdentityChanged,
            BeforeDisabledFailure::StagingCapacityExceeded => Self::StagingCapacityExceeded,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AfterDisabledFailure {
    OwnershipPersistFailed,
    RemoveWriteFailed,
    IdentityChanged,
}
impl From<AfterDisabledFailure> for RemoveManagedLocalPluginFailure {
    fn from(value: AfterDisabledFailure) -> Self {
        match value {
            AfterDisabledFailure::OwnershipPersistFailed => Self::OwnershipPersistFailed,
            AfterDisabledFailure::RemoveWriteFailed => Self::RemoveWriteFailed,
            AfterDisabledFailure::IdentityChanged => Self::IdentityChanged,
        }
    }
}
pub(crate) enum RemoveManagedLocalPluginResult {
    Removed {
        plugin_id: String,
        snapshot: PluginCatalogSnapshot,
    },
    RemovedCleanupPending {
        plugin_id: String,
        snapshot: PluginCatalogSnapshot,
    },
    RemovedCatalogUnconfirmed {
        plugin_id: String,
        snapshot: PluginCatalogSnapshot,
    },
    NotRemoved(NotRemoved),
}
/// Private fields prevent callers from manufacturing an impossible phase/code pair.
pub(crate) struct NotRemoved {
    disabled_decision_saved: bool,
    reason: RemoveManagedLocalPluginFailure,
    snapshot: PluginCatalogSnapshot,
}
impl RemoveManagedLocalPluginResult {
    pub(crate) fn not_removed(
        reason: BeforeDisabledFailure,
        snapshot: PluginCatalogSnapshot,
    ) -> Self {
        Self::NotRemoved(NotRemoved {
            disabled_decision_saved: false,
            reason: reason.into(),
            snapshot,
        })
    }
    pub(crate) fn not_removed_after_disabled(
        reason: AfterDisabledFailure,
        snapshot: PluginCatalogSnapshot,
    ) -> Self {
        Self::NotRemoved(NotRemoved {
            disabled_decision_saved: true,
            reason: reason.into(),
            snapshot,
        })
    }
    pub(crate) fn removed(plugin_id: String, snapshot: PluginCatalogSnapshot) -> Self {
        Self::Removed {
            plugin_id,
            snapshot,
        }
    }
    pub(crate) fn removed_cleanup_pending(
        plugin_id: String,
        snapshot: PluginCatalogSnapshot,
    ) -> Self {
        Self::RemovedCleanupPending {
            plugin_id,
            snapshot,
        }
    }
    pub(crate) fn removed_catalog_unconfirmed(
        plugin_id: String,
        snapshot: PluginCatalogSnapshot,
    ) -> Self {
        Self::RemovedCatalogUnconfirmed {
            plugin_id,
            snapshot,
        }
    }
}
impl Serialize for RemoveManagedLocalPluginResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("schemaVersion", &1u8)?;
        match self {
            Self::NotRemoved(value) => {
                map.serialize_entry("status", "notRemoved")?;
                map.serialize_entry("disabledDecisionSaved", &value.disabled_decision_saved)?;
                map.serialize_entry("reasonCode", value.reason.as_code())?;
                map.serialize_entry("snapshot", &value.snapshot)?;
            }
            Self::Removed {
                plugin_id,
                snapshot,
            }
            | Self::RemovedCleanupPending {
                plugin_id,
                snapshot,
            }
            | Self::RemovedCatalogUnconfirmed {
                plugin_id,
                snapshot,
            } => {
                let status = match self {
                    Self::Removed { .. } => "removed",
                    Self::RemovedCleanupPending { .. } => "removedCleanupPending",
                    _ => "removedCatalogUnconfirmed",
                };
                map.serialize_entry("status", status)?;
                map.serialize_entry("pluginId", plugin_id)?;
                if matches!(self, Self::RemovedCatalogUnconfirmed { .. }) {
                    map.serialize_entry("reasonCode", "plugin_remove_publication_unconfirmed")?;
                }
                map.serialize_entry("snapshot", snapshot)?;
            }
        }
        map.end()
    }
}
#[cfg(test)]
mod tests;
