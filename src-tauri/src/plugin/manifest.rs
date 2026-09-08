use std::fmt;

use semver::Version;
use serde::de::{value::MapAccessDeserializer, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

pub const APPROVAL_FINGERPRINT_NONE: &str = "v1:none";
pub const PLUGIN_MANIFEST_SCHEMA_VERSION_V1: u32 = 1;

const CATALOG_TRANSPORT_SCHEMA_VERSION: u8 = 3;

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
    // Shared with the frontend: Unicode White_Space plus ECMAScript's U+FEFF.
    if value
        .chars()
        .all(|ch| ch.is_whitespace() || ch == '\u{feff}')
        || value.len() > max_length
    {
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalDiscoverySummary {
    pub status: LocalDiscoveryStatus,
    pub rejected_package_count: u32,
}

impl LocalDiscoverySummary {
    pub(crate) fn with_additional_rejections(mut self, rejected: u32) -> Self {
        if rejected != 0 && self.status != LocalDiscoveryStatus::Unavailable {
            self.status = LocalDiscoveryStatus::Degraded;
            self.rejected_package_count = self
                .rejected_package_count
                .saturating_add(rejected)
                .min(256);
        }
        self
    }

    pub fn available() -> Self {
        Self {
            status: LocalDiscoveryStatus::Available,
            rejected_package_count: 0,
        }
    }

    pub fn degraded(rejected_package_count: u32) -> Result<Self, String> {
        if !(1..=256).contains(&rejected_package_count) {
            return Err("degraded discovery count must be between 1 and 256".to_owned());
        }
        Ok(Self {
            status: LocalDiscoveryStatus::Degraded,
            rejected_package_count,
        })
    }

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginManagement {
    BuiltIn,
    Managed,
    External,
    RemovalPending,
    OwnershipConflict,
    OwnershipUnavailable,
}

impl PluginManagement {
    fn for_source(source: PluginSource) -> Self {
        match source {
            PluginSource::BuiltIn => Self::BuiltIn,
            PluginSource::LocalDeclarative => Self::External,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginToggleBlockReason {
    RemovalPending,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagedOwnershipSummary {
    pub status: LocalDiscoveryStatus,
    pub conflicting_entry_count: u32,
    pub rollback_pending_count: u32,
    pub cleanup_pending_count: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedOwnershipSummaryWire {
    status: LocalDiscoveryStatus,
    conflicting_entry_count: u32,
    rollback_pending_count: u32,
    cleanup_pending_count: u32,
}
impl TryFrom<ManagedOwnershipSummaryWire> for ManagedOwnershipSummary {
    type Error = &'static str;
    fn try_from(wire: ManagedOwnershipSummaryWire) -> Result<Self, Self::Error> {
        let summary = Self {
            status: wire.status,
            conflicting_entry_count: wire.conflicting_entry_count,
            rollback_pending_count: wire.rollback_pending_count,
            cleanup_pending_count: wire.cleanup_pending_count,
        };
        summary.validate()?;
        Ok(summary)
    }
}

impl<'de> Deserialize<'de> for ManagedOwnershipSummary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_object::<D, ManagedOwnershipSummaryWire>(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}
impl ManagedOwnershipSummary {
    pub fn available() -> Self {
        Self {
            status: LocalDiscoveryStatus::Available,
            conflicting_entry_count: 0,
            rollback_pending_count: 0,
            cleanup_pending_count: 0,
        }
    }
    pub fn unavailable() -> Self {
        Self {
            status: LocalDiscoveryStatus::Unavailable,
            ..Self::available()
        }
    }
    pub(crate) fn counts(conflicts: u32, rollback: u32, cleanup: u32) -> Self {
        let result = Self {
            status: if conflicts == 0 && rollback == 0 && cleanup == 0 {
                LocalDiscoveryStatus::Available
            } else {
                LocalDiscoveryStatus::Degraded
            },
            conflicting_entry_count: conflicts,
            rollback_pending_count: rollback,
            cleanup_pending_count: cleanup,
        };
        debug_assert!(result.validate().is_ok());
        result
    }
    fn validate(&self) -> Result<(), &'static str> {
        let total = self
            .conflicting_entry_count
            .checked_add(self.rollback_pending_count)
            .and_then(|n| n.checked_add(self.cleanup_pending_count))
            .ok_or("ownership count overflow")?;
        if total > 176
            || self.rollback_pending_count > 160
            || self.cleanup_pending_count > 160
            || (self.status == LocalDiscoveryStatus::Degraded) != (total > 0)
        {
            return Err("invalid ownership summary");
        }
        Ok(())
    }
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
    management: PluginManagement,
    can_remove: bool,
    toggle_block_reason_code: Option<PluginToggleBlockReason>,
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

fn deserialize_object<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct ObjectVisitor<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
        type Value = T;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an exact catalog object")
        }
        fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<T, M::Error> {
            T::deserialize(MapAccessDeserializer::new(map))
        }
    }
    deserializer.deserialize_map(ObjectVisitor(std::marker::PhantomData))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginCatalogItemWire {
    manifest: PluginManifestV1,
    source: PluginSource,
    status: PluginStatus,
    #[serde(deserialize_with = "required_option")]
    status_reason_code: Option<PluginAvailabilityReason>,
    can_toggle: bool,
    granted_capabilities: Vec<String>,
    management: PluginManagement,
    can_remove: bool,
    #[serde(deserialize_with = "required_option")]
    toggle_block_reason_code: Option<PluginToggleBlockReason>,
}

impl<'de> Deserialize<'de> for PluginCatalogItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = deserialize_object::<D, PluginCatalogItemWire>(deserializer)?;
        let item = Self {
            manifest: wire.manifest,
            source: wire.source,
            status: wire.status,
            status_reason_code: wire.status_reason_code,
            can_toggle: wire.can_toggle,
            granted_capabilities: wire.granted_capabilities,
            management: wire.management,
            can_remove: wire.can_remove,
            toggle_block_reason_code: wire.toggle_block_reason_code,
        };
        item.validate().map_err(serde::de::Error::custom)?;
        Ok(item)
    }
}
impl PluginCatalogItem {
    pub fn enabled(manifest: PluginManifestV1, source: PluginSource) -> Self {
        Self::project(
            manifest,
            source,
            PluginStatus::Enabled,
            None,
            PluginManagement::for_source(source),
        )
    }
    pub fn disabled(manifest: PluginManifestV1, source: PluginSource) -> Self {
        Self::project(
            manifest,
            source,
            PluginStatus::Disabled,
            None,
            PluginManagement::for_source(source),
        )
    }
    pub fn blocked(
        manifest: PluginManifestV1,
        source: PluginSource,
        reason: PluginAvailabilityReason,
    ) -> Self {
        Self::project(
            manifest,
            source,
            PluginStatus::Blocked,
            Some(reason),
            PluginManagement::for_source(source),
        )
    }
    pub(crate) fn with_management(self, management: PluginManagement) -> Self {
        Self::project(
            self.manifest,
            self.source,
            self.status,
            self.status_reason_code,
            management,
        )
    }
    fn project(
        manifest: PluginManifestV1,
        source: PluginSource,
        mut status: PluginStatus,
        mut reason: Option<PluginAvailabilityReason>,
        management: PluginManagement,
    ) -> Self {
        let pending = management == PluginManagement::RemovalPending;
        if pending {
            status = PluginStatus::Disabled;
            reason = None;
        }
        Self {
            manifest,
            source,
            can_toggle: status != PluginStatus::Blocked && !pending,
            can_remove: management == PluginManagement::Managed && status == PluginStatus::Disabled,
            status,
            status_reason_code: reason,
            granted_capabilities: vec![],
            management,
            toggle_block_reason_code: pending.then_some(PluginToggleBlockReason::RemovalPending),
        }
    }
    fn validate(&self) -> Result<(), &'static str> {
        if (self.source == PluginSource::BuiltIn) != (self.management == PluginManagement::BuiltIn)
            || (self.status == PluginStatus::Blocked) != self.status_reason_code.is_some()
            || self.can_toggle
                != (self.status != PluginStatus::Blocked && self.toggle_block_reason_code.is_none())
            || (self.management == PluginManagement::RemovalPending)
                != self.toggle_block_reason_code.is_some()
            || (self.management == PluginManagement::RemovalPending
                && self.status != PluginStatus::Disabled)
            || self.can_remove
                != (self.management == PluginManagement::Managed
                    && self.status == PluginStatus::Disabled)
            || !self.granted_capabilities.is_empty()
        {
            return Err("invalid catalog management or status");
        }
        Ok(())
    }
    pub(crate) fn same_structure(&self, other: &Self) -> bool {
        self.manifest == other.manifest
            && self.source == other.source
            && self.management == other.management
            && self.toggle_block_reason_code == other.toggle_block_reason_code
            && self.granted_capabilities == other.granted_capabilities
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogSnapshot {
    pub schema_version: u8,
    pub revision: String,
    pub catalog_generation: String,
    pub availability: PluginAvailability,
    pub availability_reason_code: Option<PluginAvailabilityReason>,
    pub local_discovery: LocalDiscoverySummary,
    pub managed_ownership: ManagedOwnershipSummary,
    pub plugins: Vec<PluginCatalogItem>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginCatalogSnapshotWire {
    schema_version: u8,
    revision: String,
    catalog_generation: String,
    availability: PluginAvailability,
    #[serde(deserialize_with = "required_option")]
    availability_reason_code: Option<PluginAvailabilityReason>,
    local_discovery: LocalDiscoverySummary,
    managed_ownership: ManagedOwnershipSummary,
    plugins: Vec<PluginCatalogItem>,
}
fn valid_version(value: &str) -> bool {
    value.parse::<u64>().is_ok_and(|n| n.to_string() == value)
}
impl<'de> Deserialize<'de> for PluginCatalogSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_object::<D, PluginCatalogSnapshotWire>(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}
impl TryFrom<PluginCatalogSnapshotWire> for PluginCatalogSnapshot {
    type Error = &'static str;
    fn try_from(w: PluginCatalogSnapshotWire) -> Result<Self, Self::Error> {
        if w.schema_version != 3
            || !valid_version(&w.revision)
            || !valid_version(&w.catalog_generation)
            || (w.availability == PluginAvailability::Unavailable)
                != w.availability_reason_code.is_some()
            || w.local_discovery.rejected_package_count > 256
            || (w.local_discovery.status == LocalDiscoveryStatus::Degraded)
                != (w.local_discovery.rejected_package_count > 0)
            || w.plugins.iter().any(|item| {
                (w.availability == PluginAvailability::Available
                    && item.status == PluginStatus::Blocked)
                    || (w.availability == PluginAvailability::Unavailable
                        && item.management != PluginManagement::RemovalPending
                        && (item.status != PluginStatus::Blocked
                            || item.status_reason_code != w.availability_reason_code))
                    || (w.managed_ownership.status == LocalDiscoveryStatus::Unavailable
                        && item.management == PluginManagement::Managed)
                    || (w.managed_ownership.status != LocalDiscoveryStatus::Unavailable
                        && item.management == PluginManagement::OwnershipUnavailable)
                    || (item.management == PluginManagement::RemovalPending
                        && w.managed_ownership.rollback_pending_count == 0)
                    || (item.management == PluginManagement::OwnershipConflict
                        && w.managed_ownership.conflicting_entry_count == 0)
                    || (w.local_discovery.status == LocalDiscoveryStatus::Unavailable
                        && item.source == PluginSource::LocalDeclarative)
            })
            || w.plugins
                .windows(2)
                .any(|items| items[0].manifest.id >= items[1].manifest.id)
        {
            return Err("invalid catalog snapshot");
        }
        Ok(Self {
            schema_version: w.schema_version,
            revision: w.revision,
            catalog_generation: w.catalog_generation,
            availability: w.availability,
            availability_reason_code: w.availability_reason_code,
            local_discovery: w.local_discovery,
            managed_ownership: w.managed_ownership,
            plugins: w.plugins,
        })
    }
}
impl PluginCatalogSnapshot {
    pub fn new(
        revision: String,
        catalog_generation: String,
        availability: PluginAvailability,
        availability_reason_code: Option<PluginAvailabilityReason>,
        local_discovery: LocalDiscoverySummary,
        managed_ownership: ManagedOwnershipSummary,
        plugins: Vec<PluginCatalogItem>,
    ) -> Self {
        Self {
            schema_version: CATALOG_TRANSPORT_SCHEMA_VERSION,
            revision,
            catalog_generation,
            availability,
            availability_reason_code,
            local_discovery,
            managed_ownership,
            plugins,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginCatalogMutationResult {
    pub schema_version: u8,
    pub revision: String,
    pub catalog_generation: String,
    pub plugin: PluginCatalogItem,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginCatalogMutationWire {
    schema_version: u8,
    revision: String,
    catalog_generation: String,
    plugin: PluginCatalogItem,
}
impl TryFrom<PluginCatalogMutationWire> for PluginCatalogMutationResult {
    type Error = &'static str;
    fn try_from(w: PluginCatalogMutationWire) -> Result<Self, Self::Error> {
        if w.schema_version != 3
            || !valid_version(&w.revision)
            || !valid_version(&w.catalog_generation)
        {
            return Err("invalid catalog mutation");
        }
        Ok(Self::new(w.revision, w.catalog_generation, w.plugin))
    }
}
impl<'de> Deserialize<'de> for PluginCatalogMutationResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_object::<D, PluginCatalogMutationWire>(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
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
    #[test]
    fn catalog_v3_enforces_management_toggle_and_summary_cross_fields() {
        let snapshot = super::PluginCatalogSnapshot::new(
            "0".into(),
            "0".into(),
            super::PluginAvailability::Available,
            None,
            super::LocalDiscoverySummary::available(),
            crate::plugin::manifest::ManagedOwnershipSummary::available(),
            vec![],
        );
        let value = serde_json::to_value(snapshot).unwrap();
        assert_eq!(value["schemaVersion"], 3);
        assert_eq!(value["managedOwnership"]["status"], "available");
        let manifest: PluginManifestV1 = serde_json::from_str(VALID_MANIFEST).unwrap();
        for management in [
            PluginManagement::BuiltIn,
            PluginManagement::Managed,
            PluginManagement::External,
            PluginManagement::RemovalPending,
            PluginManagement::OwnershipConflict,
            PluginManagement::OwnershipUnavailable,
        ] {
            let source = if management == PluginManagement::BuiltIn {
                PluginSource::BuiltIn
            } else {
                PluginSource::LocalDeclarative
            };
            for item in [
                PluginCatalogItem::disabled(manifest.clone(), source),
                PluginCatalogItem::enabled(manifest.clone(), source),
                PluginCatalogItem::blocked(
                    manifest.clone(),
                    source,
                    PluginAvailabilityReason::StateUnavailable,
                ),
            ] {
                let item = item.with_management(management);
                let value = serde_json::to_value(&item).unwrap();
                assert_eq!(
                    serde_json::from_value::<PluginCatalogItem>(value.clone()).unwrap(),
                    item
                );
                for field in ["canToggle", "canRemove"] {
                    let mut invalid = value.clone();
                    invalid[field] = serde_json::json!(!invalid[field].as_bool().unwrap());
                    assert!(
                        serde_json::from_value::<PluginCatalogItem>(invalid).is_err(),
                        "{management:?}/{field}"
                    );
                }
                let mut invalid = value.clone();
                invalid["toggleBlockReasonCode"] = if management == PluginManagement::RemovalPending
                {
                    serde_json::Value::Null
                } else {
                    serde_json::json!("removalPending")
                };
                assert!(serde_json::from_value::<PluginCatalogItem>(invalid).is_err());
                let mut invalid = value;
                invalid["source"] = if source == PluginSource::BuiltIn {
                    serde_json::json!("localDeclarative")
                } else {
                    serde_json::json!("builtIn")
                };
                assert!(serde_json::from_value::<PluginCatalogItem>(invalid).is_err());
            }
        }
        for (status, counts, valid) in [
            ("available", [0, 0, 0], true),
            ("available", [1, 0, 0], false),
            ("degraded", [0, 0, 0], false),
            ("degraded", [1, 0, 0], true),
            ("unavailable", [0, 0, 0], true),
            ("unavailable", [0, 0, 1], false),
            ("degraded", [177, 0, 0], false),
            ("degraded", [1, 160, 16], false),
        ] {
            let value = serde_json::json!({"status":status,"conflictingEntryCount":counts[0],"rollbackPendingCount":counts[1],"cleanupPendingCount":counts[2]});
            assert_eq!(
                serde_json::from_value::<ManagedOwnershipSummary>(value).is_ok(),
                valid
            );
        }
        for schema in [1, 2, 4] {
            let mut invalid = value.clone();
            invalid["schemaVersion"] = serde_json::json!(schema);
            assert!(serde_json::from_value::<PluginCatalogSnapshot>(invalid).is_err());
        }
    }

    #[test]
    fn transport_never_contains_locator_receipt_slot_fingerprint_or_object_identity() {
        let item = PluginCatalogItem::disabled(
            serde_json::from_str(VALID_MANIFEST).unwrap(),
            PluginSource::LocalDeclarative,
        )
        .with_management(PluginManagement::Managed);
        let snapshot = PluginCatalogSnapshot::new(
            "9".into(),
            "7".into(),
            PluginAvailability::Available,
            None,
            LocalDiscoverySummary::available(),
            ManagedOwnershipSummary::available(),
            vec![item.clone()],
        );
        let mutation = PluginCatalogMutationResult::new("10".into(), "7".into(), item);
        for value in [
            serde_json::to_value(&snapshot).unwrap(),
            serde_json::to_value(&mutation).unwrap(),
        ] {
            let text = serde_json::to_string(&value).unwrap().to_lowercase();
            for secret in [
                "receipt",
                "slot",
                "fingerprint",
                "identity",
                "locator",
                "path",
            ] {
                assert!(!text.contains(secret));
            }
        }
        assert_eq!(
            serde_json::from_value::<PluginCatalogSnapshot>(
                serde_json::to_value(&snapshot).unwrap()
            )
            .unwrap(),
            snapshot
        );
    }

    #[test]
    fn v3_transport_rejects_positional_objects_at_every_envelope() {
        let manifest: serde_json::Value = serde_json::from_str(VALID_MANIFEST).unwrap();
        let item = serde_json::json!([
            manifest,
            "localDeclarative",
            "disabled",
            null,
            true,
            [],
            "external",
            false,
            null
        ]);
        assert!(serde_json::from_value::<PluginCatalogItem>(item).is_err());
        assert!(
            serde_json::from_value::<ManagedOwnershipSummary>(serde_json::json!([
                "available",
                0,
                0,
                0
            ]))
            .is_err()
        );
        let summary = serde_json::json!({"status":"available","conflictingEntryCount":0,"rollbackPendingCount":0,"cleanupPendingCount":0});
        assert!(serde_json::from_value::<PluginCatalogSnapshot>(
            serde_json::json!([3,"0","1","available",null,
            {"status":"available","rejectedPackageCount":0},summary,[]])
        )
        .is_err());
    }
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
    fn manifest_rejects_feff_only_name() {
        assert_blank_display_rejected("name");
    }

    #[test]
    fn manifest_rejects_feff_only_publisher() {
        assert_blank_display_rejected("publisher");
    }

    #[test]
    fn manifest_rejects_feff_only_description() {
        assert_blank_display_rejected("description");
    }

    fn assert_blank_display_rejected(field: &str) {
        for blank in ["\u{feff}", " \u{feff}\t\u{85}\n"] {
            let mut document: serde_json::Value = serde_json::from_str(VALID_MANIFEST).unwrap();
            document[field] = serde_json::json!(blank);
            assert!(
                serde_json::from_value::<PluginManifestV1>(document).is_err(),
                "{field} must reject whitespace and U+FEFF-only display text"
            );
        }
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
    fn catalog_transport_v3_carries_generation_and_discovery() {
        let snapshot = PluginCatalogSnapshot::new(
            "9".to_owned(),
            "3".to_owned(),
            PluginAvailability::Available,
            None,
            LocalDiscoverySummary::available(),
            crate::plugin::manifest::ManagedOwnershipSummary::available(),
            Vec::new(),
        );
        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            serde_json::json!({
                "schemaVersion": 3,
                "revision": "9",
                "catalogGeneration": "3",
                "availability": "available",
                "availabilityReasonCode": null,
                "localDiscovery": {"status": "available", "rejectedPackageCount": 0},
                "managedOwnership": {"status":"available","conflictingEntryCount":0,"rollbackPendingCount":0,"cleanupPendingCount":0},
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
            crate::plugin::manifest::ManagedOwnershipSummary::available(),
            vec![item.clone()],
        );
        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            serde_json::json!({
                "schemaVersion": 3,
                "revision": "42",
                "catalogGeneration": "3",
                "availability": "unavailable",
                "availabilityReasonCode": "catalogInvalid",
                "localDiscovery": {"status": "degraded", "rejectedPackageCount": 2},
                "managedOwnership": {"status":"available","conflictingEntryCount":0,"rollbackPendingCount":0,"cleanupPendingCount":0},
                "plugins": [{
                    "manifest": serde_json::from_str::<serde_json::Value>(VALID_MANIFEST).unwrap(),
                    "source": "builtIn",
                    "status": "blocked",
                    "statusReasonCode": "catalogInvalid",
                    "canToggle": false,
                    "grantedCapabilities": [], "management": "builtIn", "canRemove": false, "toggleBlockReasonCode": null
                }]
            })
        );

        let mutation = PluginCatalogMutationResult::new("43".to_owned(), "3".to_owned(), item);
        assert_eq!(
            serde_json::to_value(mutation).unwrap(),
            serde_json::json!({
                "schemaVersion": 3,
                "revision": "43",
                "catalogGeneration": "3",
                "plugin": {
                    "manifest": serde_json::from_str::<serde_json::Value>(VALID_MANIFEST).unwrap(),
                    "source": "builtIn",
                    "status": "blocked",
                    "statusReasonCode": "catalogInvalid",
                    "canToggle": false,
                    "grantedCapabilities": [], "management": "builtIn", "canRemove": false, "toggleBlockReasonCode": null
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
                "grantedCapabilities": [], "management": "builtIn", "canRemove": false, "toggleBlockReasonCode": null
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
                "grantedCapabilities": [], "management": "builtIn", "canRemove": false, "toggleBlockReasonCode": null
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
                "grantedCapabilities": [], "management": "builtIn", "canRemove": false, "toggleBlockReasonCode": null
            }),
            serde_json::json!({
                "manifest": manifest.clone(),
                "source": "builtIn",
                "status": "blocked",
                "statusReasonCode": null,
                "canToggle": false,
                "grantedCapabilities": [], "management": "builtIn", "canRemove": false, "toggleBlockReasonCode": null
            }),
            serde_json::json!({
                "manifest": manifest.clone(),
                "source": "builtIn",
                "status": "disabled",
                "statusReasonCode": "stateUnavailable",
                "canToggle": true,
                "grantedCapabilities": [], "management": "builtIn", "canRemove": false, "toggleBlockReasonCode": null
            }),
            serde_json::json!({
                "manifest": manifest,
                "source": "builtIn",
                "status": "blocked",
                "statusReasonCode": "policyBlocked",
                "canToggle": false,
                "grantedCapabilities": [], "management": "builtIn", "canRemove": false, "toggleBlockReasonCode": null
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
