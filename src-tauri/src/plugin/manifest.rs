use std::fmt;

use semver::Version;
use serde::{Deserialize, Deserializer, Serialize};

pub const APPROVAL_FINGERPRINT_NONE: &str = "v1:none";
pub const PLUGIN_MANIFEST_SCHEMA_V1: &str = "easiflux.plugin.manifest.v1";

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
                formatter.write_str(&self.0)
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
    pub schema: String,
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
    schema: String,
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
        let wire = PluginManifestV1Wire::deserialize(deserializer)?;
        let manifest = Self {
            schema: wire.schema,
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
        if self.schema != PLUGIN_MANIFEST_SCHEMA_V1 {
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum PluginSource {
    BuiltIn,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogItem {
    pub manifest: PluginManifestV1,
    pub source: PluginSource,
    pub status: PluginStatus,
    pub availability: PluginAvailability,
    pub availability_reason: Option<PluginAvailabilityReason>,
    pub approval_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogSnapshot {
    pub revision: String,
    pub plugins: Vec<PluginCatalogItem>,
}

impl PluginCatalogSnapshot {
    pub fn new(revision: u64, plugins: Vec<PluginCatalogItem>) -> Self {
        Self {
            revision: revision.to_string(),
            plugins,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogMutationResult {
    pub revision: String,
    pub plugin: PluginCatalogItem,
}

impl PluginCatalogMutationResult {
    pub fn new(revision: u64, plugin: PluginCatalogItem) -> Self {
        Self {
            revision: revision.to_string(),
            plugin,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::builtin::builtin_manifests;

    const VALID_MANIFEST: &str = r#"{
        "schema": "easiflux.plugin.manifest.v1",
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
        let overlong_id = format!("com.{}", "a".repeat(125));
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
            overlong_id.as_str(),
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
        let publisher_overlong_id = format!("com.{}", "a".repeat(125));
        for invalid in [
            "com.EasiFlux",
            "easiflux",
            "com..easiflux",
            "com.-easiflux",
            "com.easiflux-",
            "com.发布者",
            publisher_overlong_segment.as_str(),
            publisher_overlong_id.as_str(),
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
        assert_eq!(manifest.version.to_string(), "1.2.3");
    }

    #[test]
    fn manifest_rejects_invalid_schema_text_version_reserved_fields_and_unknown_fields() {
        let cases = [
            VALID_MANIFEST.replace("easiflux.plugin.manifest.v1", "other.schema.v1"),
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
        ];

        for document in cases {
            assert!(
                serde_json::from_str::<PluginManifestV1>(&document).is_err(),
                "{document} should be rejected"
            );
        }
    }

    #[test]
    fn catalog_transport_serializes_strict_camel_case_with_string_revisions() {
        let manifest: PluginManifestV1 = serde_json::from_str(VALID_MANIFEST).unwrap();
        let item = PluginCatalogItem {
            manifest,
            source: PluginSource::BuiltIn,
            status: PluginStatus::Disabled,
            availability: PluginAvailability::Unavailable,
            availability_reason: Some(PluginAvailabilityReason::CatalogInvalid),
            approval_fingerprint: APPROVAL_FINGERPRINT_NONE.to_owned(),
        };

        let snapshot = PluginCatalogSnapshot::new(42, vec![item.clone()]);
        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            serde_json::json!({
                "revision": "42",
                "plugins": [{
                    "manifest": serde_json::from_str::<serde_json::Value>(VALID_MANIFEST).unwrap(),
                    "source": "builtIn",
                    "status": "disabled",
                    "availability": "unavailable",
                    "availabilityReason": "catalogInvalid",
                    "approvalFingerprint": "v1:none"
                }]
            })
        );

        let mutation = PluginCatalogMutationResult::new(43, item);
        assert_eq!(
            serde_json::to_value(mutation).unwrap(),
            serde_json::json!({
                "revision": "43",
                "plugin": {
                    "manifest": serde_json::from_str::<serde_json::Value>(VALID_MANIFEST).unwrap(),
                    "source": "builtIn",
                    "status": "disabled",
                    "availability": "unavailable",
                    "availabilityReason": "catalogInvalid",
                    "approvalFingerprint": "v1:none"
                }
            })
        );
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
