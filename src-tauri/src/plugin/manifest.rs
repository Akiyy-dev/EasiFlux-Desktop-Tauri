use std::fmt;

use semver::Version;
use serde::de::{value::MapAccessDeserializer, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

pub const APPROVAL_FINGERPRINT_NONE: &str = "v1:none";
pub const PLUGIN_MANIFEST_SCHEMA_VERSION_V1: u32 = 1;

const CATALOG_TRANSPORT_SCHEMA_VERSION: u8 = 2;

const MAX_IDENTIFIER_LENGTH: usize = 128;
const MAX_IDENTIFIER_SEGMENT_LENGTH: usize = 63;
const MAX_NAME_LENGTH: usize = 80;
const MAX_DESCRIPTION_LENGTH: usize = 500;
const MAX_PUBLISHER_DISPLAY_LENGTH: usize = 80;
const MAX_VERSION_LENGTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct PluginId(String);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct PluginPublisherId(String);

macro_rules! impl_reverse_domain_id {
    ($type:ident) => {
        impl $type {
            pub fn parse(value: impl AsRef<str>) -> Result<Self, String> {
                let value = value.as_ref();
                validate_reverse_domain(value)?;
                Ok(Self(value.to_owned()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

impl_reverse_domain_id!(PluginId);
impl_reverse_domain_id!(PluginPublisherId);

fn validate_reverse_domain(value: &str) -> Result<(), String> {
    if value.len() > MAX_IDENTIFIER_LENGTH {
        return Err("identifier exceeds 128 bytes".into());
    }

    let segments: Vec<_> = value.split('.').collect();
    if segments.len() < 2 {
        return Err("identifier must contain at least two segments".into());
    }

    for segment in segments {
        if segment.is_empty() || segment.len() > MAX_IDENTIFIER_SEGMENT_LENGTH {
            return Err("identifier has an empty or oversized segment".into());
        }
        if !segment
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || segment.starts_with('-')
            || segment.ends_with('-')
        {
            return Err("identifier segments must be lowercase ASCII domain labels".into());
        }
    }

    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginManifestV1 {
    pub schema_version: u32,
    pub id: PluginId,
    pub publisher_id: PluginPublisherId,
    pub publisher: String,
    pub name: String,
    pub description: String,
    pub version: Version,
    pub contributions: Vec<serde_json::Value>,
    pub requested_capabilities: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginManifestV1Wire {
    schema_version: u32,
    id: PluginId,
    publisher_id: PluginPublisherId,
    publisher: String,
    name: String,
    description: String,
    version: Version,
    contributions: Vec<serde_json::Value>,
    requested_capabilities: Vec<String>,
}

impl<'de> Deserialize<'de> for PluginManifestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(PluginManifestV1Visitor)
    }
}

struct PluginManifestV1Visitor;

impl<'de> Visitor<'de> for PluginManifestV1Visitor {
    type Value = PluginManifestV1;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a plugin manifest object")
    }

    fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let wire = PluginManifestV1Wire::deserialize(MapAccessDeserializer::new(map))?;
        let manifest = PluginManifestV1 {
            schema_version: wire.schema_version,
            id: wire.id,
            publisher_id: wire.publisher_id,
            publisher: wire.publisher,
            name: wire.name,
            description: wire.description,
            version: wire.version,
            contributions: wire.contributions,
            requested_capabilities: wire.requested_capabilities,
        };
        manifest.validate().map_err(serde::de::Error::custom)?;
        Ok(manifest)
    }
}

impl PluginManifestV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PLUGIN_MANIFEST_SCHEMA_VERSION_V1 {
            return Err("unsupported plugin manifest schema".into());
        }
        validate_display_text("publisher", &self.publisher, MAX_PUBLISHER_DISPLAY_LENGTH)?;
        validate_display_text("name", &self.name, MAX_NAME_LENGTH)?;
        validate_display_text("description", &self.description, MAX_DESCRIPTION_LENGTH)?;
        if self.version.to_string().len() > MAX_VERSION_LENGTH {
            return Err("plugin version exceeds 64 bytes".into());
        }
        if !self.contributions.is_empty() {
            return Err("plugin contributions are reserved".into());
        }
        if !self.requested_capabilities.is_empty() {
            return Err("plugin requested capabilities are reserved".into());
        }
        Ok(())
    }
}

fn validate_display_text(label: &str, value: &str, max_length: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max_length {
        return Err(format!(
            "plugin {label} must be non-blank and at most {max_length} bytes"
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum PluginSource {
    BuiltIn,
    LocalDeclarative,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum LocalDiscoveryStatus {
    Available,
    Degraded,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDiscoverySummary {
    pub status: LocalDiscoveryStatus,
    pub rejected_package_count: u32,
}

impl LocalDiscoverySummary {
    pub fn available() -> Self {
        Self {
            status: LocalDiscoveryStatus::Available,
            rejected_package_count: 0,
        }
    }

    #[allow(dead_code)]
    pub fn degraded(rejected_package_count: u32) -> Result<Self, String> {
        if !(1..=256).contains(&rejected_package_count) {
            return Err("degraded discovery count must be between 1 and 256".to_owned());
        }
        Ok(Self {
            status: LocalDiscoveryStatus::Degraded,
            rejected_package_count,
        })
    }

    #[allow(dead_code)]
    pub fn unavailable() -> Self {
        Self {
            status: LocalDiscoveryStatus::Unavailable,
            rejected_package_count: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum PluginStatus {
    Enabled,
    Disabled,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum PluginAvailability {
    Available,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum PluginAvailabilityReason {
    StateUnavailable,
    CatalogInvalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogItem {
    manifest: PluginManifestV1,
    source: PluginSource,
    status: PluginStatus,
    status_reason_code: Option<PluginAvailabilityReason>,
    can_toggle: bool,
    granted_capabilities: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginCatalogItemWire {
    manifest: PluginManifestV1,
    source: PluginSource,
    status: PluginStatus,
    status_reason_code: Option<PluginAvailabilityReason>,
    can_toggle: bool,
    granted_capabilities: Vec<String>,
}

impl<'de> Deserialize<'de> for PluginCatalogItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = PluginCatalogItemWire::deserialize(deserializer)?;
        validate_catalog_item_state(&wire.status, wire.can_toggle, &wire.status_reason_code)
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            manifest: wire.manifest,
            source: wire.source,
            status: wire.status,
            status_reason_code: wire.status_reason_code,
            can_toggle: wire.can_toggle,
            granted_capabilities: wire.granted_capabilities,
        })
    }
}

impl PluginCatalogItem {
    pub fn enabled(manifest: PluginManifestV1, source: PluginSource) -> Self {
        Self {
            manifest,
            source,
            status: PluginStatus::Enabled,
            status_reason_code: None,
            can_toggle: true,
            granted_capabilities: Vec::new(),
        }
    }

    pub fn disabled(manifest: PluginManifestV1, source: PluginSource) -> Self {
        Self {
            manifest,
            source,
            status: PluginStatus::Disabled,
            status_reason_code: None,
            can_toggle: true,
            granted_capabilities: Vec::new(),
        }
    }

    pub fn blocked(
        manifest: PluginManifestV1,
        source: PluginSource,
        status_reason_code: PluginAvailabilityReason,
    ) -> Self {
        Self {
            manifest,
            source,
            status: PluginStatus::Blocked,
            status_reason_code: Some(status_reason_code),
            can_toggle: false,
            granted_capabilities: Vec::new(),
        }
    }
}

fn validate_catalog_item_state(
    status: &PluginStatus,
    can_toggle: bool,
    status_reason_code: &Option<PluginAvailabilityReason>,
) -> Result<(), &'static str> {
    match status {
        PluginStatus::Enabled | PluginStatus::Disabled
            if can_toggle && status_reason_code.is_none() =>
        {
            Ok(())
        }
        PluginStatus::Blocked if !can_toggle && status_reason_code.is_some() => Ok(()),
        PluginStatus::Enabled | PluginStatus::Disabled => {
            Err("toggleable plugin items must not have a status reason code")
        }
        PluginStatus::Blocked => {
            Err("blocked plugin items must be non-toggleable with a reason code")
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogSnapshot {
    pub schema_version: u8,
    pub revision: String,
    pub catalog_generation: String,
    pub availability: PluginAvailability,
    pub availability_reason_code: Option<PluginAvailabilityReason>,
    pub local_discovery: LocalDiscoverySummary,
    pub plugins: Vec<PluginCatalogItem>,
}

impl PluginCatalogSnapshot {
    pub fn new(
        revision: String,
        catalog_generation: String,
        availability: PluginAvailability,
        availability_reason_code: Option<PluginAvailabilityReason>,
        local_discovery: LocalDiscoverySummary,
        plugins: Vec<PluginCatalogItem>,
    ) -> Self {
        Self {
            schema_version: CATALOG_TRANSPORT_SCHEMA_VERSION,
            revision,
            catalog_generation,
            availability,
            availability_reason_code,
            local_discovery,
            plugins,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogMutationResult {
    pub schema_version: u8,
    pub revision: String,
    pub catalog_generation: String,
    pub plugin: PluginCatalogItem,
}

impl PluginCatalogMutationResult {
    pub fn new(revision: String, catalog_generation: String, plugin: PluginCatalogItem) -> Self {
        Self {
            schema_version: CATALOG_TRANSPORT_SCHEMA_VERSION,
            revision,
            catalog_generation,
            plugin,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::builtin::builtin_manifests;

    const VALID_MANIFEST: &str = r#"{
        "schemaVersion": 1,
        "id": "com.easiflux.analytics",
        "publisherId": "com.easiflux",
        "publisher": "EasiFlux",
        "name": "Analytics",
        "description": "Analytics commands",
        "version": "1.2.3",
        "contributions": [],
        "requestedCapabilities": []
    }"#;

    #[test]
    fn plugin_ids_accept_lowercase_reverse_domains_and_63_byte_segments() {
        assert_eq!(
            PluginId::parse("com.easiflux.analytics").unwrap().as_str(),
            "com.easiflux.analytics"
        );
        let legal_segment = format!("a{}", "b".repeat(62));
        let id = format!("com.{legal_segment}");
        assert_eq!(PluginId::parse(&id).unwrap().to_string(), id);
        let longest_id = format!("{}.{}.c", "a".repeat(63), "b".repeat(62));
        assert_eq!(longest_id.len(), 128);
        assert_eq!(PluginId::parse(&longest_id).unwrap().as_str(), longest_id);
    }

    #[test]
    fn plugin_ids_reject_noncanonical_or_oversized_domains() {
        let overlong_segment = format!("com.{}", "a".repeat(64));
        let overlong_total = format!("{}.{}.a", "a".repeat(63), "b".repeat(63));
        assert_eq!(overlong_total.len(), 129);
        for invalid in [
            "com.EasiFlux.analytics",
            "analytics",
            "com..analytics",
            ".com.analytics",
            "com.analytics.",
            "com.-analytics",
            "com.analytics-",
            "com.分析",
            overlong_segment.as_str(),
            overlong_total.as_str(),
        ] {
            assert!(
                PluginId::parse(invalid).is_err(),
                "{invalid} should be rejected"
            );
        }
    }

    #[test]
    fn publisher_ids_enforce_the_same_domain_boundaries() {
        assert_eq!(
            PluginPublisherId::parse("com.easiflux").unwrap().as_str(),
            "com.easiflux"
        );
        let longest_publisher = format!("{}.{}.c", "a".repeat(63), "b".repeat(62));
        assert_eq!(longest_publisher.len(), 128);
        assert_eq!(
            PluginPublisherId::parse(&longest_publisher)
                .unwrap()
                .as_str(),
            longest_publisher
        );
        let publisher_overlong_segment = format!("com.{}", "a".repeat(64));
        let publisher_overlong_total = format!("{}.{}.a", "a".repeat(63), "b".repeat(63));
        assert_eq!(publisher_overlong_total.len(), 129);
        for invalid in [
            "com.EasiFlux",
            "easiflux",
            "com..easiflux",
            "com.-easiflux",
            "com.easiflux-",
            "com.发布者",
            publisher_overlong_segment.as_str(),
            publisher_overlong_total.as_str(),
        ] {
            assert!(
                PluginPublisherId::parse(invalid).is_err(),
                "{invalid} should be rejected"
            );
        }
    }

    #[test]
    fn manifest_deserializes_a_complete_strict_v1_document() {
        let manifest: PluginManifestV1 = serde_json::from_str(VALID_MANIFEST).unwrap();
        assert_eq!(manifest.id.as_str(), "com.easiflux.analytics");
        assert_eq!(manifest.publisher_id.as_str(), "com.easiflux");
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.version.to_string(), "1.2.3");
    }

    #[test]
    fn manifest_rejects_invalid_schema_text_version_reserved_fields_and_unknown_fields() {
        let cases = [
            VALID_MANIFEST.replace("\"schemaVersion\": 1", "\"schemaVersion\": 2"),
            VALID_MANIFEST.replace("\"Analytics\"", "\"   \""),
            VALID_MANIFEST.replace("Analytics commands", "   "),
            VALID_MANIFEST.replace("\"EasiFlux\"", &format!("\"{}\"", "x".repeat(81))),
            VALID_MANIFEST.replace("\"Analytics\"", &format!("\"{}\"", "x".repeat(81))),
            VALID_MANIFEST.replace("Analytics commands", &"x".repeat(501)),
            VALID_MANIFEST.replace("\"1.2.3\"", "\"not-semver\""),
            VALID_MANIFEST.replace("\"contributions\": []", "\"contributions\": [{}]"),
            VALID_MANIFEST.replace(
                "\"requestedCapabilities\": []",
                "\"requestedCapabilities\": [\"network\"]",
            ),
            VALID_MANIFEST.replace(
                "\"version\": \"1.2.3\",",
                "\"version\": \"1.2.3\", \"extra\": true,",
            ),
            VALID_MANIFEST.replace(
                "\"schemaVersion\": 1,",
                "\"schemaVersion\": 1, \"schema\": \"legacy\",",
            ),
        ];

        for document in cases {
            assert!(
                serde_json::from_str::<PluginManifestV1>(&document).is_err(),
                "{document} should be rejected"
            );
        }
    }

    // Catches accepting serde's positional struct encoding across this trust boundary.
    #[test]
    fn manifest_rejects_positional_struct_representation() {
        assert!(serde_json::from_str::<PluginManifestV1>(
            r#"[1,"com.example.alpha","com.example","Example","Alpha","Metadata","1.0.0",[],[]]"#,
        )
        .is_err());
    }

    // Catches a catalog transport schema tied to the manifest schema or missing discovery state.
    #[test]
    fn catalog_transport_v2_carries_generation_and_discovery() {
        let snapshot = PluginCatalogSnapshot::new(
            "9".to_owned(),
            "3".to_owned(),
            PluginAvailability::Available,
            None,
            LocalDiscoverySummary::available(),
            Vec::new(),
        );
        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            serde_json::json!({
                "schemaVersion": 2,
                "revision": "9",
                "catalogGeneration": "3",
                "availability": "available",
                "availabilityReasonCode": null,
                "localDiscovery": {"status": "available", "rejectedPackageCount": 0},
                "plugins": []
            }),
        );
    }

    // Catches accepting invalid degraded counts or preserving rejected packages when unavailable.
    #[test]
    fn local_discovery_summary_enforces_status_invariants() {
        assert_eq!(
            serde_json::to_value(LocalDiscoverySummary::available()).unwrap(),
            serde_json::json!({"status": "available", "rejectedPackageCount": 0}),
        );
        assert_eq!(
            serde_json::to_value(LocalDiscoverySummary::degraded(1).unwrap()).unwrap(),
            serde_json::json!({"status": "degraded", "rejectedPackageCount": 1}),
        );
        assert_eq!(
            serde_json::to_value(LocalDiscoverySummary::unavailable()).unwrap(),
            serde_json::json!({"status": "unavailable", "rejectedPackageCount": 0}),
        );
        assert!(LocalDiscoverySummary::degraded(0).is_err());
        assert!(LocalDiscoverySummary::degraded(257).is_err());
    }

    #[test]
    fn catalog_transport_serializes_strict_camel_case_with_string_revisions() {
        let manifest: PluginManifestV1 = serde_json::from_str(VALID_MANIFEST).unwrap();
        let item = PluginCatalogItem::blocked(
            manifest,
            PluginSource::BuiltIn,
            PluginAvailabilityReason::CatalogInvalid,
        );

        let snapshot = PluginCatalogSnapshot::new(
            "42".to_owned(),
            "3".to_owned(),
            PluginAvailability::Unavailable,
            Some(PluginAvailabilityReason::CatalogInvalid),
            LocalDiscoverySummary::degraded(2).unwrap(),
            vec![item.clone()],
        );
        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            serde_json::json!({
                "schemaVersion": 2,
                "revision": "42",
                "catalogGeneration": "3",
                "availability": "unavailable",
                "availabilityReasonCode": "catalogInvalid",
                "localDiscovery": {"status": "degraded", "rejectedPackageCount": 2},
                "plugins": [{
                    "manifest": serde_json::from_str::<serde_json::Value>(VALID_MANIFEST).unwrap(),
                    "source": "builtIn",
                    "status": "blocked",
                    "statusReasonCode": "catalogInvalid",
                    "canToggle": false,
                    "grantedCapabilities": []
                }]
            })
        );

        let mutation = PluginCatalogMutationResult::new("43".to_owned(), "3".to_owned(), item);
        assert_eq!(
            serde_json::to_value(mutation).unwrap(),
            serde_json::json!({
                "schemaVersion": 2,
                "revision": "43",
                "catalogGeneration": "3",
                "plugin": {
                    "manifest": serde_json::from_str::<serde_json::Value>(VALID_MANIFEST).unwrap(),
                    "source": "builtIn",
                    "status": "blocked",
                    "statusReasonCode": "catalogInvalid",
                    "canToggle": false,
                    "grantedCapabilities": []
                }
            })
        );
    }

    #[test]
    fn enabled_catalog_items_are_toggleable_without_a_reason_code() {
        let manifest: PluginManifestV1 = serde_json::from_str(VALID_MANIFEST).unwrap();
        let item = PluginCatalogItem::enabled(manifest, PluginSource::BuiltIn);

        assert_eq!(
            serde_json::to_value(item).unwrap(),
            serde_json::json!({
                "manifest": serde_json::from_str::<serde_json::Value>(VALID_MANIFEST).unwrap(),
                "source": "builtIn",
                "status": "enabled",
                "statusReasonCode": null,
                "canToggle": true,
                "grantedCapabilities": []
            })
        );
    }

    #[test]
    fn disabled_catalog_items_are_toggleable_without_a_reason_code() {
        let manifest: PluginManifestV1 = serde_json::from_str(VALID_MANIFEST).unwrap();
        let item = PluginCatalogItem::disabled(manifest, PluginSource::BuiltIn);

        assert_eq!(
            serde_json::to_value(item).unwrap(),
            serde_json::json!({
                "manifest": serde_json::from_str::<serde_json::Value>(VALID_MANIFEST).unwrap(),
                "source": "builtIn",
                "status": "disabled",
                "statusReasonCode": null,
                "canToggle": true,
                "grantedCapabilities": []
            })
        );
    }

    #[test]
    fn catalog_items_reject_contradictory_state_or_unknown_reason_codes() {
        let manifest = serde_json::from_str::<serde_json::Value>(VALID_MANIFEST).unwrap();
        for invalid in [
            serde_json::json!({
                "manifest": manifest.clone(),
                "source": "builtIn",
                "status": "blocked",
                "statusReasonCode": "catalogInvalid",
                "canToggle": true,
                "grantedCapabilities": []
            }),
            serde_json::json!({
                "manifest": manifest.clone(),
                "source": "builtIn",
                "status": "blocked",
                "statusReasonCode": null,
                "canToggle": false,
                "grantedCapabilities": []
            }),
            serde_json::json!({
                "manifest": manifest.clone(),
                "source": "builtIn",
                "status": "disabled",
                "statusReasonCode": "stateUnavailable",
                "canToggle": true,
                "grantedCapabilities": []
            }),
            serde_json::json!({
                "manifest": manifest,
                "source": "builtIn",
                "status": "blocked",
                "statusReasonCode": "policyBlocked",
                "canToggle": false,
                "grantedCapabilities": []
            }),
        ] {
            assert!(serde_json::from_value::<PluginCatalogItem>(invalid).is_err());
        }
    }

    #[test]
    fn builtin_manifests_are_valid_unique_and_deterministically_ordered() {
        let manifests = builtin_manifests();
        let ids: Vec<_> = manifests
            .iter()
            .map(|manifest| manifest.id.as_str())
            .collect();
        let mut sorted_ids = ids.clone();
        sorted_ids.sort_unstable();
        sorted_ids.dedup();

        assert_eq!(ids, sorted_ids);
        for manifest in manifests {
            let serialized = serde_json::to_string(&manifest).unwrap();
            assert!(serde_json::from_str::<PluginManifestV1>(&serialized).is_ok());
        }
    }
}
