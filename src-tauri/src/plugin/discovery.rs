use std::collections::BTreeMap;
use std::path::PathBuf;

use super::manifest::{LocalDiscoverySummary, PluginManifestV1};
use super::record::PluginRecord;
use crate::models::config::APP_NAME;

pub(crate) mod safe_fs;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalDiscoveryOutcome {
    pub(crate) records: Vec<PluginRecord>,
    pub(crate) summary: LocalDiscoverySummary,
}

impl LocalDiscoveryOutcome {
    pub(crate) fn available(records: Vec<PluginRecord>) -> Self {
        Self {
            records,
            summary: LocalDiscoverySummary::available(),
        }
    }

    pub(crate) fn degraded(records: Vec<PluginRecord>, rejected: u32) -> Result<Self, String> {
        Ok(Self {
            records,
            summary: LocalDiscoverySummary::degraded(rejected)?,
        })
    }

    pub(crate) fn unavailable() -> Self {
        Self {
            records: Vec::new(),
            summary: LocalDiscoverySummary::unavailable(),
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
    if rejected == 0 {
        LocalDiscoveryOutcome::available(records)
    } else {
        // Safe-reader root entry bounds also bound every aggregate rejection.
        LocalDiscoveryOutcome::degraded(records, rejected)
            .unwrap_or_else(|_| LocalDiscoveryOutcome::unavailable())
    }
}

#[cfg(test)]
mod tests;
