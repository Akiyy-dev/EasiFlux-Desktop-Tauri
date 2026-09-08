use std::collections::BTreeSet;
use std::fmt;

use serde::de::{value::MapAccessDeserializer, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use uuid::{Uuid, Variant, Version};

use crate::plugin::manifest::{PluginId, PluginPublisherId, PluginSource};
use crate::plugin::record::PluginRecord;

pub(crate) const MAX_OWNERSHIP_RECEIPT_BYTES: usize = 4 * 1024;
pub(crate) const MAX_MANAGED_OWNERSHIP_BYTES: usize = 256 * 1024;
pub(crate) const MAX_MANAGED_OWNERSHIP_ENTRIES: usize = 160;

const OWNERSHIP_SCHEMA_VERSION_V1: u32 = 1;
const RECEIPT_ID_HEX_LEN: usize = 32;
const PACKAGE_SLOT_PREFIX: &str = "pkg-";
const REMOVAL_SLOT_PREFIX: &str = "remove-";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) struct FileIdentity {
    pub(crate) volume: u64,
    pub(crate) object: u128,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) struct ReceiptId(String);

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) struct PackageSlot(String);

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub(crate) struct RemovalSlot(String);

impl ReceiptId {
    pub(crate) fn parse(value: &str) -> Result<Self, OwnershipFailure> {
        validate_uuid_v4_simple(value)?;
        Ok(Self(value.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl PackageSlot {
    pub(crate) fn parse(value: &str) -> Result<Self, OwnershipFailure> {
        validate_prefixed_slot(value, PACKAGE_SLOT_PREFIX)?;
        Ok(Self(value.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl RemovalSlot {
    pub(crate) fn parse(value: &str) -> Result<Self, OwnershipFailure> {
        validate_prefixed_slot(value, REMOVAL_SLOT_PREFIX)?;
        Ok(Self(value.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_prefixed_slot(value: &str, prefix: &str) -> Result<(), OwnershipFailure> {
    let suffix = value
        .strip_prefix(prefix)
        .ok_or(OwnershipFailure::Conflict)?;
    validate_lower_hex_32(suffix)
}

fn validate_lower_hex_32(value: &str) -> Result<(), OwnershipFailure> {
    if value.len() != RECEIPT_ID_HEX_LEN
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OwnershipFailure::Conflict);
    }
    Ok(())
}

fn validate_uuid_v4_simple(value: &str) -> Result<(), OwnershipFailure> {
    validate_lower_hex_32(value)?;
    let parsed = Uuid::parse_str(value).map_err(|_| OwnershipFailure::Conflict)?;
    if parsed.get_version() != Some(Version::Random) || parsed.get_variant() != Variant::RFC4122 {
        return Err(OwnershipFailure::Conflict);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OwnershipReceiptV1 {
    schema_version: u32,
    receipt_id: ReceiptId,
    package_slot: PackageSlot,
    source: PluginSource,
    plugin_id: PluginId,
    publisher_id: PluginPublisherId,
    approval_fingerprint: String,
}

impl OwnershipReceiptV1 {
    pub(crate) fn new(
        receipt_id: ReceiptId,
        package_slot: PackageSlot,
        record: &PluginRecord,
    ) -> Result<Self, OwnershipFailure> {
        if record.source() != PluginSource::LocalDeclarative {
            return Err(OwnershipFailure::Conflict);
        }
        let identity = record.identity();
        let receipt = Self {
            schema_version: OWNERSHIP_SCHEMA_VERSION_V1,
            receipt_id,
            package_slot,
            source: identity.source,
            plugin_id: identity.id,
            publisher_id: identity.publisher_id,
            approval_fingerprint: identity.approval_fingerprint,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, OwnershipFailure> {
        if bytes.len() > MAX_OWNERSHIP_RECEIPT_BYTES {
            return Err(OwnershipFailure::CapacityExceeded);
        }
        let receipt: Self =
            serde_json::from_slice(bytes).map_err(|_| OwnershipFailure::Conflict)?;
        receipt.validate()?;
        Ok(receipt)
    }

    pub(crate) fn canonical_bytes(&self) -> Result<Vec<u8>, OwnershipFailure> {
        self.validate()?;
        let bytes = serde_json::to_vec(&ReceiptCanonicalDto::from(self))
            .map_err(|_| OwnershipFailure::PersistFailed)?;
        if bytes.len() > MAX_OWNERSHIP_RECEIPT_BYTES {
            return Err(OwnershipFailure::CapacityExceeded);
        }
        Ok(bytes)
    }

    pub(crate) fn canonical_sha256(&self) -> [u8; 32] {
        let bytes = self
            .canonical_bytes()
            .expect("validated ownership receipt must serialize");
        Sha256::digest(bytes).into()
    }

    pub(crate) fn matches_record(&self, record: &PluginRecord) -> bool {
        let identity = record.identity();
        self.source == identity.source
            && self.plugin_id == identity.id
            && self.publisher_id == identity.publisher_id
            && self.approval_fingerprint == identity.approval_fingerprint
    }

    pub(crate) fn receipt_id(&self) -> &ReceiptId {
        &self.receipt_id
    }

    pub(crate) fn package_slot(&self) -> &PackageSlot {
        &self.package_slot
    }

    fn validate(&self) -> Result<(), OwnershipFailure> {
        if self.schema_version != OWNERSHIP_SCHEMA_VERSION_V1
            || self.source != PluginSource::LocalDeclarative
            || !valid_local_fingerprint(&self.approval_fingerprint)
        {
            return Err(OwnershipFailure::Conflict);
        }
        validate_uuid_v4_simple(self.receipt_id.as_str())?;
        validate_prefixed_slot(self.package_slot.as_str(), PACKAGE_SLOT_PREFIX)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReceiptCanonicalDto<'a> {
    schema_version: u32,
    receipt_id: &'a str,
    package_slot: &'a str,
    source: PluginSource,
    plugin_id: &'a PluginId,
    publisher_id: &'a PluginPublisherId,
    approval_fingerprint: &'a str,
}

impl<'a> From<&'a OwnershipReceiptV1> for ReceiptCanonicalDto<'a> {
    fn from(receipt: &'a OwnershipReceiptV1) -> Self {
        Self {
            schema_version: receipt.schema_version,
            receipt_id: receipt.receipt_id.as_str(),
            package_slot: receipt.package_slot.as_str(),
            source: receipt.source,
            plugin_id: &receipt.plugin_id,
            publisher_id: &receipt.publisher_id,
            approval_fingerprint: &receipt.approval_fingerprint,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReceiptWireDto {
    schema_version: u32,
    receipt_id: String,
    package_slot: String,
    source: PluginSource,
    plugin_id: PluginId,
    publisher_id: PluginPublisherId,
    approval_fingerprint: String,
}

impl<'de> Deserialize<'de> for OwnershipReceiptV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ReceiptVisitor;

        impl<'de> Visitor<'de> for ReceiptVisitor {
            type Value = OwnershipReceiptV1;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an ownership receipt object")
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let dto = ReceiptWireDto::deserialize(MapAccessDeserializer::new(map))?;
                let receipt = OwnershipReceiptV1 {
                    schema_version: dto.schema_version,
                    receipt_id: ReceiptId::parse(&dto.receipt_id)
                        .map_err(serde::de::Error::custom)?,
                    package_slot: PackageSlot::parse(&dto.package_slot)
                        .map_err(serde::de::Error::custom)?,
                    source: dto.source,
                    plugin_id: dto.plugin_id,
                    publisher_id: dto.publisher_id,
                    approval_fingerprint: dto.approval_fingerprint,
                };
                receipt.validate().map_err(serde::de::Error::custom)?;
                Ok(receipt)
            }
        }

        deserializer.deserialize_map(ReceiptVisitor)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedPackageReceipt {
    pub(crate) model: OwnershipReceiptV1,
    pub(crate) canonical_sha256: [u8; 32],
    pub(crate) file_identity: FileIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OwnershipLifecycleV1 {
    Managed,
    Removing { removal_slot: RemovalSlot },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManagedOwnershipEntryV1 {
    receipt_id: ReceiptId,
    lifecycle: OwnershipLifecycleV1,
    package_slot: PackageSlot,
    plugin_id: PluginId,
    source: PluginSource,
    publisher_id: PluginPublisherId,
    approval_fingerprint: String,
    receipt_sha256: [u8; 32],
    pub(crate) directory_identity: FileIdentity,
    pub(crate) manifest_identity: FileIdentity,
    pub(crate) receipt_identity: FileIdentity,
}

impl ManagedOwnershipEntryV1 {
    pub(crate) fn matches_receipt(&self, receipt: &OwnershipReceiptV1, digest: [u8; 32]) -> bool {
        self.receipt_id == receipt.receipt_id
            && self.package_slot == *receipt.package_slot()
            && self.plugin_id == receipt.plugin_id
            && self.source == receipt.source
            && self.publisher_id == receipt.publisher_id
            && self.approval_fingerprint == receipt.approval_fingerprint
            && self.receipt_sha256 == digest
    }

    pub(crate) fn matches_locator(&self, locator: &super::discovery::LocalPackageLocator) -> bool {
        self.package_slot == locator.package_slot
            && self.directory_identity == locator.directory_identity
            && self.manifest_identity == locator.manifest_identity
            && locator
                .receipt
                .as_ref()
                .is_some_and(|receipt| self.receipt_identity == receipt.file_identity)
    }

    pub(crate) fn matches_package(
        &self,
        package: &super::discovery::DiscoveredLocalPlugin,
    ) -> bool {
        package.locator.receipt.as_ref().is_some_and(|receipt| {
            self.matches_receipt(&receipt.model, receipt.canonical_sha256)
                && self.matches_locator(&package.locator)
                && receipt.model.matches_record(&package.record)
        })
    }

    pub(crate) fn proves_managed(&self, package: &super::discovery::DiscoveredLocalPlugin) -> bool {
        self.removal_slot().is_none() && self.matches_package(package)
    }
    pub(crate) fn managed(
        receipt: VerifiedPackageReceipt,
        record: &PluginRecord,
        directory_identity: FileIdentity,
        manifest_identity: FileIdentity,
    ) -> Result<Self, OwnershipFailure> {
        if !receipt.model.matches_record(record)
            || receipt.model.canonical_sha256() != receipt.canonical_sha256
        {
            return Err(OwnershipFailure::Conflict);
        }
        let identity = record.identity();
        let entry = Self {
            receipt_id: receipt.model.receipt_id.clone(),
            lifecycle: OwnershipLifecycleV1::Managed,
            package_slot: receipt.model.package_slot.clone(),
            plugin_id: identity.id,
            source: identity.source,
            publisher_id: identity.publisher_id,
            approval_fingerprint: identity.approval_fingerprint,
            receipt_sha256: receipt.canonical_sha256,
            directory_identity,
            manifest_identity,
            receipt_identity: receipt.file_identity,
        };
        entry.validate()?;
        Ok(entry)
    }

    pub(crate) fn receipt_id(&self) -> &ReceiptId {
        &self.receipt_id
    }

    pub(crate) fn package_slot(&self) -> &PackageSlot {
        &self.package_slot
    }

    pub(crate) fn removal_slot(&self) -> Option<&RemovalSlot> {
        match &self.lifecycle {
            OwnershipLifecycleV1::Managed => None,
            OwnershipLifecycleV1::Removing { removal_slot } => Some(removal_slot),
        }
    }

    pub(crate) fn begin_removal(&self, removal_slot: RemovalSlot) -> Self {
        let mut next = self.clone();
        next.lifecycle = OwnershipLifecycleV1::Removing { removal_slot };
        next
    }

    pub(crate) fn restore_managed(&self) -> Self {
        let mut next = self.clone();
        next.lifecycle = OwnershipLifecycleV1::Managed;
        next
    }

    fn validate(&self) -> Result<(), OwnershipFailure> {
        if self.source != PluginSource::LocalDeclarative
            || !valid_local_fingerprint(&self.approval_fingerprint)
        {
            return Err(OwnershipFailure::Conflict);
        }
        validate_uuid_v4_simple(self.receipt_id.as_str())?;
        validate_prefixed_slot(self.package_slot.as_str(), PACKAGE_SLOT_PREFIX)?;
        if let Some(slot) = self.removal_slot() {
            validate_prefixed_slot(slot.as_str(), REMOVAL_SLOT_PREFIX)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManagedOwnershipIndexV1 {
    schema_version: u32,
    revision: u64,
    entries: Vec<ManagedOwnershipEntryV1>,
}

impl ManagedOwnershipIndexV1 {
    pub(crate) fn entries(&self) -> &[ManagedOwnershipEntryV1] {
        &self.entries
    }
    pub(crate) fn empty() -> Self {
        Self {
            schema_version: OWNERSHIP_SCHEMA_VERSION_V1,
            revision: 0,
            entries: Vec::new(),
        }
    }

    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, OwnershipFailure> {
        if bytes.len() > MAX_MANAGED_OWNERSHIP_BYTES {
            return Err(OwnershipFailure::CapacityExceeded);
        }
        let index: Self = serde_json::from_slice(bytes).map_err(|_| OwnershipFailure::Conflict)?;
        index.validate()?;
        Ok(index)
    }

    pub(crate) fn canonical_bytes(&self) -> Result<Vec<u8>, OwnershipFailure> {
        self.validate()?;
        let bytes = self.serialize_unchecked()?;
        if bytes.len() > MAX_MANAGED_OWNERSHIP_BYTES {
            return Err(OwnershipFailure::CapacityExceeded);
        }
        Ok(bytes)
    }

    pub(crate) fn validate(&self) -> Result<(), OwnershipFailure> {
        if self.schema_version != OWNERSHIP_SCHEMA_VERSION_V1 {
            return Err(OwnershipFailure::Conflict);
        }
        if self.entries.len() > MAX_MANAGED_OWNERSHIP_ENTRIES {
            return Err(OwnershipFailure::CapacityExceeded);
        }

        let mut receipt_ids = BTreeSet::new();
        let mut package_slots = BTreeSet::new();
        let mut removal_slots = BTreeSet::new();
        let mut plugins = BTreeSet::new();
        let mut object_identities = BTreeSet::new();
        let mut previous_receipt_id: Option<&ReceiptId> = None;

        for entry in &self.entries {
            entry.validate()?;
            if previous_receipt_id.is_some_and(|previous| previous >= entry.receipt_id()) {
                return Err(OwnershipFailure::Conflict);
            }
            previous_receipt_id = Some(entry.receipt_id());

            if !receipt_ids.insert(entry.receipt_id().clone())
                || !package_slots.insert(entry.package_slot().clone())
                || !plugins.insert((entry.plugin_id.clone(), entry.source))
            {
                return Err(OwnershipFailure::Conflict);
            }
            if let Some(removal_slot) = entry.removal_slot() {
                if !removal_slots.insert(removal_slot.clone()) {
                    return Err(OwnershipFailure::Conflict);
                }
            }
            for identity in [
                entry.directory_identity,
                entry.manifest_identity,
                entry.receipt_identity,
            ] {
                if !object_identities.insert(identity) {
                    return Err(OwnershipFailure::Conflict);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn validate_for_persistence(&self) -> Result<(), OwnershipFailure> {
        self.validate()?;
        if self.serialize_unchecked()?.len() > MAX_MANAGED_OWNERSHIP_BYTES {
            return Err(OwnershipFailure::CapacityExceeded);
        }
        Ok(())
    }

    pub(crate) fn require_revision_headroom(&self, steps: u64) -> Result<(), OwnershipFailure> {
        self.revision
            .checked_add(steps)
            .map(|_| ())
            .ok_or(OwnershipFailure::RevisionExhausted)
    }

    pub(crate) fn register(
        &self,
        entry: ManagedOwnershipEntryV1,
    ) -> Result<Self, OwnershipFailure> {
        if self.entries.iter().any(|current| current == &entry) {
            return Ok(self.clone());
        }
        let mut next = self.clone();
        next.entries.push(entry);
        Self::finish_changed(next, self)
    }

    pub(crate) fn begin_removal(
        &self,
        receipt_id: &ReceiptId,
        removal_slot: RemovalSlot,
    ) -> Result<Self, OwnershipFailure> {
        let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.receipt_id() == receipt_id)
        else {
            return Err(OwnershipFailure::Conflict);
        };
        match &entry.lifecycle {
            OwnershipLifecycleV1::Removing {
                removal_slot: current,
            } if current == &removal_slot => return Ok(self.clone()),
            OwnershipLifecycleV1::Removing { .. } => return Err(OwnershipFailure::Conflict),
            OwnershipLifecycleV1::Managed => self.require_revision_headroom(2)?,
        }

        let mut next = self.clone();
        let entry = next
            .entries
            .iter_mut()
            .find(|entry| entry.receipt_id() == receipt_id)
            .ok_or(OwnershipFailure::Conflict)?;
        *entry = entry.begin_removal(removal_slot);
        Self::finish_changed(next, self)
    }

    pub(crate) fn restore_managed(&self, receipt_id: &ReceiptId) -> Result<Self, OwnershipFailure> {
        let mut next = self.clone();
        if let Some(entry) = next
            .entries
            .iter_mut()
            .find(|entry| entry.receipt_id() == receipt_id)
        {
            *entry = entry.restore_managed();
        }
        Self::finish_changed(next, self)
    }

    pub(crate) fn remove(&self, receipt_id: &ReceiptId) -> Result<Self, OwnershipFailure> {
        let mut next = self.clone();
        next.entries
            .retain(|entry| entry.receipt_id() != receipt_id);
        Self::finish_changed(next, self)
    }

    fn finish_changed(mut next: Self, previous: &Self) -> Result<Self, OwnershipFailure> {
        next.entries
            .sort_by(|a, b| a.receipt_id().cmp(b.receipt_id()));
        next.validate_for_persistence()?;
        if next.entries == previous.entries {
            return Ok(previous.clone());
        }
        next.revision = previous
            .revision
            .checked_add(1)
            .ok_or(OwnershipFailure::RevisionExhausted)?;
        Ok(next)
    }

    fn serialize_unchecked(&self) -> Result<Vec<u8>, OwnershipFailure> {
        let entries = self.entries.iter().map(EntryCanonicalDto::from).collect();
        serde_json::to_vec(&IndexCanonicalDto {
            schema_version: self.schema_version,
            revision: self.revision,
            entries,
        })
        .map_err(|_| OwnershipFailure::PersistFailed)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IndexCanonicalDto<'a> {
    schema_version: u32,
    #[serde(with = "decimal_revision")]
    revision: u64,
    entries: Vec<EntryCanonicalDto<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryCanonicalDto<'a> {
    receipt_id: &'a str,
    lifecycle: LifecycleWire,
    package_slot: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    removal_slot: Option<&'a str>,
    plugin_id: &'a PluginId,
    source: PluginSource,
    publisher_id: &'a PluginPublisherId,
    approval_fingerprint: &'a str,
    receipt_sha256: String,
    directory_identity: FileIdentityCanonicalDto,
    manifest_identity: FileIdentityCanonicalDto,
    receipt_identity: FileIdentityCanonicalDto,
}

impl<'a> From<&'a ManagedOwnershipEntryV1> for EntryCanonicalDto<'a> {
    fn from(entry: &'a ManagedOwnershipEntryV1) -> Self {
        let (lifecycle, removal_slot) = match &entry.lifecycle {
            OwnershipLifecycleV1::Managed => (LifecycleWire::Managed, None),
            OwnershipLifecycleV1::Removing { removal_slot } => {
                (LifecycleWire::Removing, Some(removal_slot.as_str()))
            }
        };
        Self {
            receipt_id: entry.receipt_id.as_str(),
            lifecycle,
            package_slot: entry.package_slot.as_str(),
            removal_slot,
            plugin_id: &entry.plugin_id,
            source: entry.source,
            publisher_id: &entry.publisher_id,
            approval_fingerprint: &entry.approval_fingerprint,
            receipt_sha256: hex::encode(entry.receipt_sha256),
            directory_identity: entry.directory_identity.into(),
            manifest_identity: entry.manifest_identity.into(),
            receipt_identity: entry.receipt_identity.into(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileIdentityCanonicalDto {
    volume: String,
    object: String,
}

impl From<FileIdentity> for FileIdentityCanonicalDto {
    fn from(identity: FileIdentity) -> Self {
        Self {
            volume: format!("{:016x}", identity.volume),
            object: format!("{:032x}", identity.object),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum LifecycleWire {
    Managed,
    Removing,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IndexWireDto {
    schema_version: u32,
    #[serde(with = "decimal_revision")]
    revision: u64,
    entries: Vec<EntryWireDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EntryWireDto {
    receipt_id: String,
    lifecycle: LifecycleWire,
    package_slot: String,
    #[serde(default)]
    removal_slot: OptionalField<String>,
    plugin_id: PluginId,
    source: PluginSource,
    publisher_id: PluginPublisherId,
    approval_fingerprint: String,
    receipt_sha256: String,
    directory_identity: FileIdentityWireDto,
    manifest_identity: FileIdentityWireDto,
    receipt_identity: FileIdentityWireDto,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileIdentityWireDto {
    volume: String,
    object: String,
}

enum OptionalField<T> {
    Missing,
    Present(T),
}

impl<T> Default for OptionalField<T> {
    fn default() -> Self {
        Self::Missing
    }
}

impl<'de, T> Deserialize<'de> for OptionalField<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        T::deserialize(deserializer).map(Self::Present)
    }
}

impl FileIdentityWireDto {
    fn parse(self) -> Result<FileIdentity, OwnershipFailure> {
        Ok(FileIdentity {
            volume: parse_fixed_lower_hex::<16, u64>(&self.volume)?,
            object: parse_fixed_lower_hex::<32, u128>(&self.object)?,
        })
    }
}

trait FromLowerHex: Sized {
    fn from_lower_hex(value: &str) -> Result<Self, OwnershipFailure>;
}

impl FromLowerHex for u64 {
    fn from_lower_hex(value: &str) -> Result<Self, OwnershipFailure> {
        u64::from_str_radix(value, 16).map_err(|_| OwnershipFailure::Conflict)
    }
}

impl FromLowerHex for u128 {
    fn from_lower_hex(value: &str) -> Result<Self, OwnershipFailure> {
        u128::from_str_radix(value, 16).map_err(|_| OwnershipFailure::Conflict)
    }
}

fn parse_fixed_lower_hex<const WIDTH: usize, T: FromLowerHex>(
    value: &str,
) -> Result<T, OwnershipFailure> {
    if value.len() != WIDTH
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OwnershipFailure::Conflict);
    }
    T::from_lower_hex(value)
}

impl TryFrom<EntryWireDto> for ManagedOwnershipEntryV1 {
    type Error = OwnershipFailure;

    fn try_from(dto: EntryWireDto) -> Result<Self, Self::Error> {
        let lifecycle = match (dto.lifecycle, dto.removal_slot) {
            (LifecycleWire::Managed, OptionalField::Missing) => OwnershipLifecycleV1::Managed,
            (LifecycleWire::Removing, OptionalField::Present(removal_slot)) => {
                OwnershipLifecycleV1::Removing {
                    removal_slot: RemovalSlot::parse(&removal_slot)?,
                }
            }
            _ => return Err(OwnershipFailure::Conflict),
        };
        let receipt_sha256 = parse_sha256(&dto.receipt_sha256)?;
        let entry = ManagedOwnershipEntryV1 {
            receipt_id: ReceiptId::parse(&dto.receipt_id)?,
            lifecycle,
            package_slot: PackageSlot::parse(&dto.package_slot)?,
            plugin_id: dto.plugin_id,
            source: dto.source,
            publisher_id: dto.publisher_id,
            approval_fingerprint: dto.approval_fingerprint,
            receipt_sha256,
            directory_identity: dto.directory_identity.parse()?,
            manifest_identity: dto.manifest_identity.parse()?,
            receipt_identity: dto.receipt_identity.parse()?,
        };
        entry.validate()?;
        Ok(entry)
    }
}

impl<'de> Deserialize<'de> for ManagedOwnershipIndexV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct IndexVisitor;

        impl<'de> Visitor<'de> for IndexVisitor {
            type Value = ManagedOwnershipIndexV1;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a managed ownership index object")
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let dto = IndexWireDto::deserialize(MapAccessDeserializer::new(map))?;
                let entries = dto
                    .entries
                    .into_iter()
                    .map(ManagedOwnershipEntryV1::try_from)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(serde::de::Error::custom)?;
                let index = ManagedOwnershipIndexV1 {
                    schema_version: dto.schema_version,
                    revision: dto.revision,
                    entries,
                };
                index.validate().map_err(serde::de::Error::custom)?;
                Ok(index)
            }
        }

        deserializer.deserialize_map(IndexVisitor)
    }
}

fn parse_sha256(value: &str) -> Result<[u8; 32], OwnershipFailure> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OwnershipFailure::Conflict);
    }
    let decoded = hex::decode(value).map_err(|_| OwnershipFailure::Conflict)?;
    decoded.try_into().map_err(|_| OwnershipFailure::Conflict)
}

fn valid_local_fingerprint(value: &str) -> bool {
    value.strip_prefix("v1:sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

mod decimal_revision {
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum OwnershipFailure {
    #[error("ownership unavailable")]
    Unavailable,
    #[error("ownership persistence failed")]
    PersistFailed,
    #[error("ownership capacity exceeded")]
    CapacityExceeded,
    #[error("ownership revision exhausted")]
    RevisionExhausted,
    #[error("ownership conflict")]
    Conflict,
}

impl OwnershipFailure {
    pub(crate) fn as_code(self) -> &'static str {
        match self {
            Self::Unavailable => "plugin_ownership_unavailable",
            Self::PersistFailed => "plugin_ownership_persist_failed",
            Self::CapacityExceeded => "plugin_ownership_capacity_exceeded",
            Self::RevisionExhausted => "plugin_ownership_revision_exhausted",
            Self::Conflict => "plugin_ownership_conflict",
        }
    }
}

#[derive(Clone)]
pub(crate) enum OwnershipRuntime {
    Available {
        index: ManagedOwnershipIndexV1,
        requires_rewrite: bool,
        persistence: std::sync::Arc<
            dyn crate::storage::managed_plugin_ownership::ManagedOwnershipPersistence,
        >,
    },
    Unavailable {
        persistence: std::sync::Arc<
            dyn crate::storage::managed_plugin_ownership::ManagedOwnershipPersistence,
        >,
    },
}

impl OwnershipRuntime {
    pub(crate) fn initialize(
        persistence: std::sync::Arc<
            dyn crate::storage::managed_plugin_ownership::ManagedOwnershipPersistence,
        >,
    ) -> Self {
        match persistence.load() {
            Ok(load) if load.index.validate_for_persistence().is_ok() => Self::Available {
                index: load.index,
                requires_rewrite: load.requires_rewrite,
                persistence,
            },
            _ => Self::Unavailable { persistence },
        }
    }

    pub(crate) fn requires_retry(&self) -> bool {
        !matches!(
            self,
            Self::Available {
                requires_rewrite: false,
                ..
            }
        )
    }

    pub(crate) fn entries(&self) -> Option<&[ManagedOwnershipEntryV1]> {
        match self {
            Self::Available { index, .. } => Some(index.entries()),
            _ => None,
        }
    }

    /// Clone/persist/adopt; a post-commit error must not resurrect the old index.
    fn save(&mut self, next: ManagedOwnershipIndexV1) -> bool {
        let Self::Available {
            index,
            requires_rewrite,
            persistence,
        } = self
        else {
            return false;
        };
        let result = persistence.save(&next);
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(failure) => failure.outcome,
        };
        if crate::storage::safe_plugin_document::persist_outcome_committed(outcome) {
            *index = next;
            *requires_rewrite = false;
            true
        } else {
            false
        }
    }

    pub(crate) fn reconcile(
        &self,
        outcome: &super::discovery::LocalDiscoveryOutcome,
        allow_writes: bool,
    ) -> (
        Self,
        std::collections::BTreeMap<PluginId, super::manifest::PluginManagement>,
        super::manifest::ManagedOwnershipSummary,
    ) {
        let persistence = match self {
            Self::Available { persistence, .. } | Self::Unavailable { persistence } => persistence,
        };
        // Each authoritative scan reloads the index too. A formerly healthy
        // in-memory index cannot conceal later loss or damage to disk authority.
        Self::initialize(std::sync::Arc::clone(persistence)).reconcile_loaded(outcome, allow_writes)
    }

    fn reconcile_loaded(
        &self,
        outcome: &super::discovery::LocalDiscoveryOutcome,
        allow_writes: bool,
    ) -> (
        Self,
        std::collections::BTreeMap<PluginId, super::manifest::PluginManagement>,
        super::manifest::ManagedOwnershipSummary,
    ) {
        use super::discovery::RemovalObservationShape;
        use super::manifest::{LocalDiscoveryStatus, ManagedOwnershipSummary, PluginManagement};
        let mut runtime = self.clone();
        let unavailable = |runtime| {
            let classes = outcome
                .plugins
                .iter()
                .map(|package| {
                    (
                        package.record.manifest().id.clone(),
                        if package.locator.receipt.is_some() {
                            PluginManagement::OwnershipUnavailable
                        } else {
                            PluginManagement::External
                        },
                    )
                })
                .collect();
            (runtime, classes, ManagedOwnershipSummary::unavailable())
        };
        if outcome.removals.status == LocalDiscoveryStatus::Unavailable
            || outcome.summary.status == LocalDiscoveryStatus::Unavailable
        {
            return unavailable(runtime);
        }
        let Self::Available {
            index,
            requires_rewrite,
            persistence,
        } = &runtime
        else {
            return unavailable(runtime);
        };
        let mut index = index.clone();
        let persistence = std::sync::Arc::clone(persistence);
        if *requires_rewrite {
            // Classify recovered evidence without any convergence writes first.
            // Its classifications stay private until the equivalent rewrite commits.
            let recovered = Self::Available {
                index: index.clone(),
                requires_rewrite: false,
                persistence: persistence.clone(),
            };
            let (mut observed, _, _) = recovered.reconcile_loaded(outcome, false);
            if !allow_writes || !observed.save(index) {
                return unavailable(Self::Unavailable { persistence });
            }
            return observed.reconcile_loaded(outcome, allow_writes);
        }
        let mut conflicts = 0u32;
        let mut rollback = 0u32;
        let mut cleanup = 0u32;
        let mut classes = std::collections::BTreeMap::new();
        for entry in index.entries().to_vec() {
            let claimants: Vec<_> =
                outcome
                    .plugins
                    .iter()
                    .filter(|package| {
                        package.locator.package_slot == entry.package_slot
                            || package.record.manifest().id == entry.plugin_id
                            || package.locator.receipt.as_ref().is_some_and(|receipt| {
                                receipt.model.receipt_id() == entry.receipt_id()
                            })
                    })
                    .collect();
            if claimants.len() > 1 {
                conflicts += 1;
                for package in claimants {
                    classes.insert(
                        package.record.manifest().id.clone(),
                        PluginManagement::OwnershipConflict,
                    );
                }
                continue;
            }
            let source = outcome
                .plugins
                .iter()
                .find(|package| package.locator.package_slot == entry.package_slot);
            let source_present =
                source.is_some() || outcome.occupied_slots.contains(&entry.package_slot);
            let Some(target_slot) = entry.removal_slot() else {
                if !source.is_some_and(|package| entry.proves_managed(package)) {
                    conflicts += 1;
                }
                continue;
            };
            let target_present = outcome.removals.occupied_slots.contains(target_slot);
            let target = outcome
                .removals
                .observations
                .iter()
                .find(|object| &object.removal_slot == target_slot);
            if source_present
                && !target_present
                && source.is_some_and(|package| entry.matches_package(package))
            {
                let next = index.restore_managed(entry.receipt_id());
                if allow_writes && next.as_ref().is_ok_and(|next| runtime.save(next.clone())) {
                    index = next.unwrap();
                } else {
                    rollback += 1;
                    classes.insert(
                        source.unwrap().record.manifest().id.clone(),
                        PluginManagement::RemovalPending,
                    );
                }
            } else if !source_present && !target_present {
                let next = index.remove(entry.receipt_id());
                if allow_writes && next.as_ref().is_ok_and(|next| runtime.save(next.clone())) {
                    index = next.unwrap();
                } else {
                    cleanup += 1;
                }
            } else if !source_present
                && target.is_some_and(|target| {
                    if target.directory_identity != entry.directory_identity {
                        return false;
                    }
                    let receipt_matches = target.receipt.as_ref().is_some_and(|receipt| {
                        receipt.file_identity == entry.receipt_identity
                            && entry.matches_receipt(&receipt.model, receipt.canonical_sha256)
                    });
                    match target.shape {
                        RemovalObservationShape::Full => {
                            receipt_matches
                                && target.manifest.as_ref().is_some_and(|(record, identity)| {
                                    *identity == entry.manifest_identity
                                        && target
                                            .receipt
                                            .as_ref()
                                            .unwrap()
                                            .model
                                            .matches_record(record)
                                })
                        }
                        RemovalObservationShape::ReceiptOnly => receipt_matches,
                        RemovalObservationShape::EmptyDirectory => true,
                        _ => false,
                    }
                })
            {
                cleanup += 1;
            } else {
                conflicts += 1;
            }
        }
        // An occupied quarantine is counted once: its matching entry already
        // contributed exactly one class above, even when its bytes are unreadable.
        let orphan_count = outcome
            .removals
            .occupied_slots
            .iter()
            .filter(|slot| {
                !index
                    .entries()
                    .iter()
                    .any(|entry| entry.removal_slot() == Some(*slot))
            })
            .count();
        let Ok(orphan_count) = u32::try_from(orphan_count) else {
            return unavailable(runtime);
        };
        let Some(conflicts) = conflicts
            .checked_add(outcome.removals.unknown_entry_count)
            .and_then(|count| count.checked_add(orphan_count))
        else {
            return unavailable(runtime);
        };
        for package in &outcome.plugins {
            let management = match (
                package.locator.receipt.as_ref(),
                classes.get(&package.record.manifest().id),
            ) {
                (None, _) => PluginManagement::External,
                (Some(_), Some(classification)) => *classification,
                (Some(receipt), None) => match index.entries().iter().find(|entry| {
                    entry.package_slot == package.locator.package_slot
                        || entry.receipt_id == *receipt.model.receipt_id()
                        || entry.plugin_id == package.record.manifest().id
                }) {
                    None => PluginManagement::External,
                    Some(entry) if entry.proves_managed(package) => PluginManagement::Managed,
                    Some(_) => PluginManagement::OwnershipConflict,
                },
            };
            classes.insert(package.record.manifest().id.clone(), management);
        }
        (
            runtime,
            classes,
            ManagedOwnershipSummary::counts(conflicts, rollback, cleanup),
        )
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn empty_test_persistence(
) -> Box<dyn crate::storage::managed_plugin_ownership::ManagedOwnershipPersistence> {
    struct Empty;
    impl crate::storage::managed_plugin_ownership::ManagedOwnershipPersistence for Empty {
        fn load(
            &self,
        ) -> crate::error::AppResult<crate::storage::managed_plugin_ownership::ManagedOwnershipLoad>
        {
            Ok(
                crate::storage::managed_plugin_ownership::ManagedOwnershipLoad {
                    index: ManagedOwnershipIndexV1::empty(),
                    requires_rewrite: false,
                },
            )
        }
        fn save(
            &self,
            _: &ManagedOwnershipIndexV1,
        ) -> crate::storage::safe_plugin_document::PersistResult {
            Ok(crate::storage::safe_plugin_document::PersistOutcome::CommittedProcessCrashSafe)
        }
    }
    Box::new(Empty)
}
