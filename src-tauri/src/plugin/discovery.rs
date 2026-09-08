use std::collections::BTreeMap;
use std::path::PathBuf;

use super::manifest::{LocalDiscoverySummary, PluginManifestV1};
use super::ownership::{
    FileIdentity, OwnershipReceiptV1, PackageSlot, RemovalSlot, VerifiedPackageReceipt,
};
use super::record::PluginRecord;
use crate::models::config::APP_NAME;

pub(crate) mod safe_fs;

/// Internal scan accounting, never part of the catalog transport or generation equality.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ScanUsage {
    pub(crate) root_entries: usize,
    pub(crate) packages: usize,
    pub(crate) bytes_read: usize,
}

impl ScanUsage {
    pub(crate) fn can_add_manifest(self, bytes: usize) -> bool {
        self.root_entries < 256
            && self.packages < 128
            && bytes <= 16_384
            && self
                .bytes_read
                .checked_add(bytes)
                .is_some_and(|n| n <= 2_097_152)
    }

    #[cfg(test)]
    fn for_records(records: &[PluginRecord]) -> Self {
        Self {
            root_entries: records.len(),
            packages: records.len(),
            bytes_read: records
                .iter()
                .map(|record| record.canonical_manifest_bytes().unwrap().len())
                .sum(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalPackageLocator {
    pub(crate) package_slot: PackageSlot,
    pub(crate) directory_identity: FileIdentity,
    pub(crate) manifest_identity: FileIdentity,
    pub(crate) receipt: Option<VerifiedPackageReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoveredLocalPlugin {
    pub(crate) record: PluginRecord,
    pub(crate) locator: LocalPackageLocator,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemovalObservationShape {
    Full,
    ReceiptOnly,
    ManifestOnly,
    EmptyDirectory,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemovalObservation {
    pub(crate) removal_slot: RemovalSlot,
    pub(crate) directory_identity: FileIdentity,
    pub(crate) manifest: Option<(PluginRecord, FileIdentity)>,
    pub(crate) receipt: Option<VerifiedPackageReceipt>,
    pub(crate) shape: RemovalObservationShape,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemovalDiscoveryOutcome {
    pub(crate) observations: Vec<RemovalObservation>,
    pub(crate) status: super::manifest::LocalDiscoveryStatus,
    pub(crate) bytes_read: usize,
    // Preserve occupied-but-unreadable names: absence is stronger than rejection.
    pub(crate) occupied_slots: Vec<RemovalSlot>,
    pub(crate) unknown_entry_count: u32,
}
impl RemovalDiscoveryOutcome {
    pub(crate) fn available() -> Self {
        Self {
            observations: vec![],
            status: super::manifest::LocalDiscoveryStatus::Available,
            bytes_read: 0,
            occupied_slots: vec![],
            unknown_entry_count: 0,
        }
    }
    pub(crate) fn unavailable() -> Self {
        Self {
            status: super::manifest::LocalDiscoveryStatus::Unavailable,
            ..Self::available()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalDiscoveryOutcome {
    pub(crate) plugins: Vec<DiscoveredLocalPlugin>,
    pub(crate) occupied_slots: Vec<PackageSlot>,
    pub(crate) removals: RemovalDiscoveryOutcome,
    pub(crate) summary: LocalDiscoverySummary,
    pub(crate) usage: Option<ScanUsage>,
}

impl LocalDiscoveryOutcome {
    #[cfg(test)]
    pub(crate) fn available(records: Vec<PluginRecord>) -> Self {
        Self {
            usage: Some(ScanUsage::for_records(&records)),
            occupied_slots: vec![],
            plugins: fixture_plugins(records),
            removals: RemovalDiscoveryOutcome::available(),
            summary: LocalDiscoverySummary::available(),
        }
    }

    #[cfg(test)]
    pub(crate) fn degraded(records: Vec<PluginRecord>, rejected: u32) -> Result<Self, String> {
        let mut usage = ScanUsage::for_records(&records);
        usage.root_entries += rejected as usize;
        Ok(Self {
            usage: Some(usage),
            occupied_slots: vec![],
            plugins: fixture_plugins(records),
            removals: RemovalDiscoveryOutcome::available(),
            summary: LocalDiscoverySummary::degraded(rejected)?,
        })
    }

    pub(crate) fn unavailable() -> Self {
        Self {
            plugins: Vec::new(),
            occupied_slots: vec![],
            removals: RemovalDiscoveryOutcome::unavailable(),
            summary: LocalDiscoverySummary::unavailable(),
            usage: None,
        }
    }
}

pub(crate) trait LocalPluginDiscovery: Send + Sync {
    fn discover(&self) -> LocalDiscoveryOutcome;
}

pub(crate) struct SystemLocalPluginDiscovery;

impl LocalPluginDiscovery for SystemLocalPluginDiscovery {
    fn discover(&self) -> LocalDiscoveryOutcome {
        discover_with_root(|| dirs::config_dir().map(|dir| dir.join(APP_NAME).join("plugins")))
    }
}

#[cfg(test)]
pub(crate) fn discover_from_root(root: &std::path::Path) -> LocalDiscoveryOutcome {
    match safe_fs::read_package_candidates(root, safe_fs::ScanLimits::production()) {
        Ok(scan) => parse_package_scan(scan),
        Err(_) => LocalDiscoveryOutcome::unavailable(),
    }
}

fn discover_with_root(resolve: impl FnOnce() -> Option<PathBuf>) -> LocalDiscoveryOutcome {
    let Some(root) = resolve() else {
        return LocalDiscoveryOutcome::unavailable();
    };
    discover_from_plugins_root(&root)
}

pub(crate) fn discover_from_plugins_root(root: &std::path::Path) -> LocalDiscoveryOutcome {
    discover_bounded(
        root,
        #[cfg(test)]
        safe_fs::REMOVAL_BYTE_LIMIT,
    )
}

fn discover_bounded(root: &std::path::Path, #[cfg(test)] limit: usize) -> LocalDiscoveryOutcome {
    let (local, removal) = safe_fs::read_both_roots(
        root,
        #[cfg(test)]
        limit,
    );
    let mut outcome = local
        .map(parse_package_scan)
        .unwrap_or_else(|_| LocalDiscoveryOutcome::unavailable());
    outcome.removals = removal
        .map(parse_removal_scan)
        .unwrap_or_else(|_| RemovalDiscoveryOutcome::unavailable());
    outcome
}

fn verified_receipt(bytes: Vec<u8>, file_identity: FileIdentity) -> Option<VerifiedPackageReceipt> {
    let model = OwnershipReceiptV1::parse(&bytes).ok()?;
    Some(VerifiedPackageReceipt {
        canonical_sha256: model.canonical_sha256(),
        model,
        file_identity,
    })
}

fn parse_removal_scan(scan: safe_fs::RemovalScan) -> RemovalDiscoveryOutcome {
    let observations = scan
        .objects
        .into_iter()
        .map(|(removal_slot, object)| {
            let has_manifest = object.manifest.is_some();
            let has_receipt = object.receipt.is_some();
            let manifest = object.manifest.and_then(|(bytes, identity)| {
                parse_record(&bytes).map(|record| (record, identity))
            });
            let receipt = object
                .receipt
                .and_then(|(bytes, identity)| verified_receipt(bytes, identity));
            let shape = if object.unknown_shape
                || (has_manifest && manifest.is_none())
                || (has_receipt && receipt.is_none())
            {
                RemovalObservationShape::Unknown
            } else {
                match (has_manifest, has_receipt) {
                    (true, true) => RemovalObservationShape::Full,
                    (false, true) => RemovalObservationShape::ReceiptOnly,
                    (true, false) => RemovalObservationShape::ManifestOnly,
                    (false, false) => RemovalObservationShape::EmptyDirectory,
                }
            };
            RemovalObservation {
                removal_slot,
                directory_identity: object.directory_identity,
                manifest,
                receipt,
                shape,
            }
        })
        .collect();
    RemovalDiscoveryOutcome {
        observations,
        status: super::manifest::LocalDiscoveryStatus::Available,
        bytes_read: scan.bytes_read,
        occupied_slots: scan.occupied_slots,
        unknown_entry_count: scan.unknown_count,
    }
}

fn parse_record(bytes: &[u8]) -> Option<PluginRecord> {
    serde_json::from_slice::<PluginManifestV1>(bytes)
        .ok()
        .and_then(|manifest| PluginRecord::local_declarative(manifest).ok())
}

fn parse_package_scan(scan: safe_fs::PackageScan) -> LocalDiscoveryOutcome {
    let mut rejected = scan.rejected_package_count;
    let mut candidates = BTreeMap::new();
    for package in scan.packages {
        let record = parse_record(&package.manifest_bytes);
        let receipt = package
            .receipt
            .map(|(bytes, identity)| verified_receipt(bytes, identity));
        // A malformed second file rejects the whole package, never a legacy fallback.
        if matches!(receipt, Some(None)) {
            rejected += 1;
            continue;
        }
        match record {
            Some(record) => candidates
                .entry(record.manifest().id.clone())
                .or_insert_with(Vec::new)
                .push(DiscoveredLocalPlugin {
                    record,
                    locator: LocalPackageLocator {
                        package_slot: package.package_slot,
                        directory_identity: package.directory_identity,
                        manifest_identity: package.manifest_identity,
                        receipt: receipt.flatten(),
                    },
                }),
            None => rejected += 1,
        }
    }
    let mut records = Vec::new();
    for (_, mut group) in candidates {
        if group.len() == 1 {
            records.push(group.pop().expect("single candidate"));
        } else {
            rejected += group.len() as u32;
        }
    }
    let summary = if rejected == 0 {
        LocalDiscoverySummary::available()
    } else {
        // Safe-reader root entry bounds also bound every aggregate rejection.
        let Ok(summary) = LocalDiscoverySummary::degraded(rejected) else {
            return LocalDiscoveryOutcome::unavailable();
        };
        summary
    };
    LocalDiscoveryOutcome {
        plugins: records,
        occupied_slots: scan.occupied_slots,
        removals: RemovalDiscoveryOutcome::available(),
        summary,
        usage: Some(scan.usage),
    }
}

#[cfg(test)]
fn fixture_plugins(records: Vec<PluginRecord>) -> Vec<DiscoveredLocalPlugin> {
    use sha2::{Digest, Sha256};
    records
        .into_iter()
        .map(|record| {
            let hash = Sha256::digest(record.manifest().id.as_str().as_bytes());
            let object = u128::from_le_bytes(hash[..16].try_into().unwrap()) & !3;
            DiscoveredLocalPlugin {
                locator: LocalPackageLocator {
                    package_slot: PackageSlot::parse(&format!("pkg-{object:032x}")).unwrap(),
                    directory_identity: FileIdentity { volume: 1, object },
                    manifest_identity: FileIdentity {
                        volume: 1,
                        object: object + 1,
                    },
                    receipt: None,
                },
                record,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
