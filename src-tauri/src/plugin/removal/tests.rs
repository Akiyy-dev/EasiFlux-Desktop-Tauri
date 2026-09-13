use super::*;
use crate::plugin::manifest::{LocalDiscoverySummary, PluginAvailability, PluginCatalogSnapshot};

fn snapshot() -> PluginCatalogSnapshot {
    PluginCatalogSnapshot::new(
        "0".into(),
        "0".into(),
        PluginAvailability::Available,
        None,
        LocalDiscoverySummary::available(),
        crate::plugin::manifest::ManagedOwnershipSummary::available(),
        vec![],
    )
}

#[test]
fn all_closed_failure_literals_and_phases_have_exact_keys() {
    use BeforeDisabledFailure::*;
    let cases = [
        (CatalogStale, "plugin_catalog_stale"),
        (CatalogInvalid, "plugin_catalog_invalid"),
        (
            CatalogGenerationExhausted,
            "plugin_catalog_generation_exhausted",
        ),
        (StateUnavailable, "plugin_state_unavailable"),
        (StatePersistFailed, "plugin_state_persist_failed"),
        (StateCapacityExceeded, "plugin_state_capacity_exceeded"),
        (RevisionExhausted, "plugin_revision_exhausted"),
        (OwnershipUnavailable, "plugin_ownership_unavailable"),
        (
            OwnershipCapacityExceeded,
            "plugin_ownership_capacity_exceeded",
        ),
        (
            OwnershipRevisionExhausted,
            "plugin_ownership_revision_exhausted",
        ),
        (OwnershipConflict, "plugin_ownership_conflict"),
        (DiscoveryUnavailable, "plugin_remove_discovery_unavailable"),
        (NotManaged, "plugin_remove_not_managed"),
        (RequiresDisabled, "plugin_remove_requires_disabled"),
        (StorageUnavailable, "plugin_remove_storage_unavailable"),
        (IdentityChanged, "plugin_remove_identity_changed"),
        (
            StagingCapacityExceeded,
            "plugin_remove_staging_capacity_exceeded",
        ),
    ];
    for (failure, code) in cases {
        assert_eq!(
            serde_json::to_value(RemoveManagedLocalPluginResult::not_removed(
                failure,
                snapshot()
            ))
            .unwrap(),
            serde_json::json!({"schemaVersion":1,"status":"notRemoved","disabledDecisionSaved":false,"reasonCode":code,"snapshot":snapshot()})
        );
    }
    for (failure, code) in [
        (
            AfterDisabledFailure::OwnershipPersistFailed,
            "plugin_ownership_persist_failed",
        ),
        (
            AfterDisabledFailure::RemoveWriteFailed,
            "plugin_remove_write_failed",
        ),
        (
            AfterDisabledFailure::IdentityChanged,
            "plugin_remove_identity_changed",
        ),
    ] {
        assert_eq!(
            serde_json::to_value(RemoveManagedLocalPluginResult::not_removed_after_disabled(
                failure,
                snapshot()
            ))
            .unwrap(),
            serde_json::json!({"schemaVersion":1,"status":"notRemoved","disabledDecisionSaved":true,"reasonCode":code,"snapshot":snapshot()})
        );
    }
}

#[test]
fn removed_branches_exact_keys_and_fixed_unconfirmed_reason() {
    for (result, status) in [
        (
            RemoveManagedLocalPluginResult::removed("com.example.notes".into(), snapshot()),
            "removed",
        ),
        (
            RemoveManagedLocalPluginResult::removed_cleanup_pending(
                "com.example.notes".into(),
                snapshot(),
            ),
            "removedCleanupPending",
        ),
        (
            RemoveManagedLocalPluginResult::removed_catalog_unconfirmed(
                "com.example.notes".into(),
                snapshot(),
            ),
            "removedCatalogUnconfirmed",
        ),
    ] {
        let mut expected = serde_json::json!({"schemaVersion":1,"status":status,"pluginId":"com.example.notes","snapshot":snapshot()});
        if status == "removedCatalogUnconfirmed" {
            expected["reasonCode"] = "plugin_remove_publication_unconfirmed".into();
        }
        assert_eq!(serde_json::to_value(result).unwrap(), expected);
    }
}

#[test]
fn not_removed_has_no_plugin_id_and_derives_disabled_decision_phase() {
    let before = serde_json::to_value(RemoveManagedLocalPluginResult::not_removed(
        BeforeDisabledFailure::RequiresDisabled,
        snapshot(),
    ))
    .unwrap();
    let after = serde_json::to_value(RemoveManagedLocalPluginResult::not_removed_after_disabled(
        AfterDisabledFailure::RemoveWriteFailed,
        snapshot(),
    ))
    .unwrap();
    assert_eq!(before["disabledDecisionSaved"], false);
    assert!(before.get("pluginId").is_none());
    assert_eq!(after["disabledDecisionSaved"], true);
}
