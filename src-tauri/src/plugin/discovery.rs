use std::collections::BTreeMap;
use std::path::PathBuf;

use super::manifest::{LocalDiscoverySummary, PluginManifestV1};
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
pub(crate) struct LocalDiscoveryOutcome {
    pub(crate) records: Vec<PluginRecord>,
    pub(crate) summary: LocalDiscoverySummary,
    pub(crate) usage: Option<ScanUsage>,
}

impl LocalDiscoveryOutcome {
    #[cfg(test)]
    pub(crate) fn available(records: Vec<PluginRecord>) -> Self {
        Self {
            usage: Some(ScanUsage::for_records(&records)),
            records,
            summary: LocalDiscoverySummary::available(),
        }
    }

    #[cfg(test)]
    pub(crate) fn degraded(records: Vec<PluginRecord>, rejected: u32) -> Result<Self, String> {
        let mut usage = ScanUsage::for_records(&records);
        usage.root_entries += rejected as usize;
        Ok(Self {
            usage: Some(usage),
            records,
            summary: LocalDiscoverySummary::degraded(rejected)?,
        })
    }

    pub(crate) fn unavailable() -> Self {
        Self {
            records: Vec::new(),
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
        discover_with_root(|| {
            dirs::config_dir().map(|dir| dir.join(APP_NAME).join("plugins").join("local"))
        })
    }
}

#[cfg(test)]
pub(crate) fn discover_from_root(root: &std::path::Path) -> LocalDiscoveryOutcome {
    discover_with_root(|| Some(root.to_path_buf()))
}

fn discover_with_root(resolve: impl FnOnce() -> Option<PathBuf>) -> LocalDiscoveryOutcome {
    let Some(root) = resolve() else {
        return LocalDiscoveryOutcome::unavailable();
    };
    match safe_fs::read_package_candidates(&root, safe_fs::ScanLimits::production()) {
        Ok(scan) => parse_package_scan(scan),
        Err(_) => LocalDiscoveryOutcome::unavailable(),
    }
}

fn parse_package_scan(scan: safe_fs::PackageScan) -> LocalDiscoveryOutcome {
    let mut rejected = scan.rejected_package_count;
    let mut candidates = BTreeMap::new();
    for package in scan.packages {
        // Decode directly into the strict object visitor: no lossy UTF-8 or Value map.
        let record = serde_json::from_slice::<PluginManifestV1>(&package.manifest_bytes)
            .ok()
            .and_then(|manifest| PluginRecord::local_declarative(manifest).ok());
        match record {
            Some(record) => candidates
                .entry(record.manifest().id.clone())
                .or_insert_with(Vec::new)
                .push(record),
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
        records,
        summary,
        usage: Some(scan.usage),
    }
}

#[cfg(test)]
mod tests;
