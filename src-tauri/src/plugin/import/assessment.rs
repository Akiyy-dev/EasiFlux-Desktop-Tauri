use std::cmp::Ordering;

use serde::Serialize;

use crate::plugin::manifest::{PluginCatalogItem, PluginManifestV1};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ImportVersionRelation {
    IncomingLower,
    SamePrecedence,
    IncomingHigher,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ImportAssessment {
    NotInCatalog,
    ExistingId {
        current: PluginCatalogItem,
        #[serde(rename = "versionRelation")]
        version_relation: ImportVersionRelation,
    },
}

impl ImportAssessment {
    pub(crate) fn from_catalog(incoming: &PluginManifestV1, catalog: &[PluginCatalogItem]) -> Self {
        let Some(current) = catalog
            .iter()
            .find(|item| item.manifest().id == incoming.id)
        else {
            return Self::NotInCatalog;
        };
        let version_relation = match incoming.version.cmp_precedence(&current.manifest().version) {
            Ordering::Less => ImportVersionRelation::IncomingLower,
            Ordering::Equal => ImportVersionRelation::SamePrecedence,
            Ordering::Greater => ImportVersionRelation::IncomingHigher,
        };
        Self::ExistingId {
            current: current.clone(),
            version_relation,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::plugin::manifest::{
        PluginAvailabilityReason, PluginCatalogItem, PluginManagement, PluginManifestV1,
        PluginSource,
    };

    fn manifest(id: &str, version: &str) -> PluginManifestV1 {
        serde_json::from_value(json!({
            "schemaVersion": 1,
            "id": id,
            "publisherId": "com.example",
            "publisher": "Example",
            "name": "Notes",
            "description": "Metadata only",
            "version": version,
            "contributions": [],
            "requestedCapabilities": []
        }))
        .unwrap()
    }

    fn current(version: &str) -> PluginCatalogItem {
        PluginCatalogItem::disabled(
            manifest("com.example.notes", version),
            PluginSource::LocalDeclarative,
        )
    }

    #[test]
    fn not_in_catalog_has_the_closed_literal_wire_shape() {
        let assessment = ImportAssessment::from_catalog(
            &manifest("com.example.notes", "1.0.0"),
            &[current("1.0.0").with_management(PluginManagement::Managed)],
        );
        let missing = ImportAssessment::from_catalog(
            &manifest("com.example.missing", "1.0.0"),
            &[current("1.0.0")],
        );

        assert_eq!(
            serde_json::to_value(missing).unwrap(),
            json!({"kind":"notInCatalog"})
        );
        assert_eq!(
            serde_json::to_value(assessment).unwrap(),
            json!({
                "kind": "existingId",
                "current": {
                    "manifest": {
                        "schemaVersion": 1,
                        "id": "com.example.notes",
                        "publisherId": "com.example",
                        "publisher": "Example",
                        "name": "Notes",
                        "description": "Metadata only",
                        "version": "1.0.0",
                        "contributions": [],
                        "requestedCapabilities": []
                    },
                    "source": "localDeclarative",
                    "management": "managed",
                    "canRemove": true,
                    "toggleBlockReasonCode": null,
                    "status": "disabled",
                    "statusReasonCode": null,
                    "canToggle": true,
                    "grantedCapabilities": []
                },
                "versionRelation": "samePrecedence"
            })
        );
    }

    #[test]
    fn version_relation_uses_semver_precedence_and_ignores_build_metadata() {
        let current = current("1.0.0+old");
        for (incoming, expected) in [
            ("0.9.9", "incomingLower"),
            ("1.0.0-alpha.1", "incomingLower"),
            ("1.0.0+new", "samePrecedence"),
            ("1.0.1-alpha.1", "incomingHigher"),
            ("2.0.0", "incomingHigher"),
        ] {
            let wire = serde_json::to_value(ImportAssessment::from_catalog(
                &manifest("com.example.notes", incoming),
                std::slice::from_ref(&current),
            ))
            .unwrap();
            assert_eq!(wire["versionRelation"], expected, "incoming {incoming}");
            assert_eq!(wire["current"]["manifest"]["version"], "1.0.0+old");
        }
    }

    #[test]
    fn matching_id_is_reported_for_every_catalog_classification_and_status() {
        let manifest = manifest("com.example.notes", "1.0.0");
        let cases = [
            PluginCatalogItem::enabled(manifest.clone(), PluginSource::BuiltIn),
            PluginCatalogItem::disabled(manifest.clone(), PluginSource::LocalDeclarative),
            PluginCatalogItem::disabled(manifest.clone(), PluginSource::LocalDeclarative)
                .with_management(PluginManagement::Managed),
            PluginCatalogItem::blocked(
                manifest.clone(),
                PluginSource::BuiltIn,
                PluginAvailabilityReason::StateUnavailable,
            ),
        ];

        for current in cases {
            let assessment = serde_json::to_value(ImportAssessment::from_catalog(
                &manifest,
                std::slice::from_ref(&current),
            ))
            .unwrap();
            assert_eq!(assessment["kind"], "existingId");
            assert_eq!(
                assessment["current"],
                serde_json::to_value(current).unwrap()
            );
        }
    }
}
