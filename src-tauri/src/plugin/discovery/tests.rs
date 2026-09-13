use super::*;
use crate::plugin::manifest::LocalDiscoverySummary;
use std::fs;
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let base = std::env::current_dir().unwrap().join("target");
        fs::create_dir_all(&base).unwrap();
        let path = base
            .canonicalize()
            .unwrap()
            .join(format!("discovery-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn scan(&self, bytes: &[Vec<u8>]) -> safe_fs::PackageScan {
        for (n, bytes) in bytes.iter().enumerate() {
            let slot = self.0.join(format!("pkg-{n:032x}"));
            fs::create_dir(&slot).unwrap();
            fs::write(slot.join("manifest.json"), bytes).unwrap();
        }
        safe_fs::read_package_candidates(&self.0, safe_fs::ScanLimits::production()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn valid_manifest(id: &str) -> Vec<u8> {
    format!(r#"{{"schemaVersion":1,"id":"{id}","publisherId":"com.example","publisher":"Example","name":"Example","description":"Metadata","version":"1.0.0","contributions":[],"requestedCapabilities":[]}}"#).into_bytes()
}

// Rejecting the exact two-file shape loses a valid external candidate.
#[test]
fn scanner_returns_external_and_receipted_candidates_with_internal_identity() {
    use crate::plugin::ownership::{OwnershipReceiptV1, PackageSlot, ReceiptId};
    let fixture = Fixture::new();
    let first = valid_manifest("com.example.first");
    let second = valid_manifest("com.example.second");
    fixture.scan(&[first.clone(), second.clone()]);
    let record = PluginRecord::local_declarative(serde_json::from_slice(&second).unwrap()).unwrap();
    let slot = PackageSlot::parse("pkg-00000000000000000000000000000001").unwrap();
    let receipt = OwnershipReceiptV1::new(
        ReceiptId::parse("550e8400e29b41d4a716446655440000").unwrap(),
        slot,
        &record,
    )
    .unwrap()
    .canonical_bytes()
    .unwrap();
    fs::write(
        fixture
            .0
            .join("pkg-00000000000000000000000000000001/ownership-receipt.json"),
        &receipt,
    )
    .unwrap();
    let outcome = discover_from_root(&fixture.0);
    assert_eq!(outcome.plugins.len(), 2);
    assert!(outcome.plugins[0].locator.receipt.is_none());
    assert!(outcome.plugins[1].locator.receipt.is_some());
    assert_eq!(
        outcome.usage.unwrap().bytes_read,
        first.len() + second.len() + receipt.len()
    );
}

#[test]
fn malformed_receipt_rejects_entire_double_file_package() {
    let fixture = Fixture::new();
    fixture.scan(&[valid_manifest("com.example.a")]);
    fs::write(
        fixture
            .0
            .join("pkg-00000000000000000000000000000000/ownership-receipt.json"),
        b"{}",
    )
    .unwrap();
    let outcome = discover_from_root(&fixture.0);
    assert!(outcome.plugins.is_empty());
    assert_eq!(outcome.summary, LocalDiscoverySummary::degraded(1).unwrap());
}

#[test]
fn removal_unknown_shape_keeps_directory_evidence_without_reading_unknown_data() {
    let fixture = Fixture::new();
    let target = fixture
        .0
        .join("removal-staging/remove-00000000000000000000000000000001");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("extra"), b"private unknown data").unwrap();
    let outcome = discover_from_plugins_root(&fixture.0);
    assert_eq!(outcome.removals.observations.len(), 1);
    assert_eq!(
        outcome.removals.observations[0].shape,
        RemovalObservationShape::Unknown
    );
    assert_eq!(outcome.removals.bytes_read, 0);
    assert_eq!(
        fs::read(target.join("extra")).unwrap(),
        b"private unknown data"
    );
}

#[test]
fn local_2mib_and_removal_328kib_budgets_are_independent() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("local")).unwrap();
    let local = fixture.0.join("local/pkg-00000000000000000000000000000001");
    fs::create_dir(&local).unwrap();
    let manifest = valid_manifest("com.example.local");
    fs::write(local.join("manifest.json"), &manifest).unwrap();
    let staging = fixture.0.join("removal-staging");
    fs::create_dir(&staging).unwrap();
    for n in 0..16 {
        let target = staging.join(format!("remove-{n:032x}"));
        fs::create_dir(&target).unwrap();
        fs::write(target.join("manifest.json"), vec![b'x'; 16385]).unwrap();
        fs::write(target.join("ownership-receipt.json"), vec![b'x'; 4097]).unwrap();
    }
    let full = discover_from_plugins_root(&fixture.0);
    assert_eq!(full.removals.bytes_read, 327_712);
    assert_eq!(full.usage.unwrap().bytes_read, manifest.len());
    assert_eq!(
        full.removals.status,
        super::super::manifest::LocalDiscoveryStatus::Available
    );
    // 16*(16385+4097) cannot reach 335873. A lower injected ceiling exercises
    // exactly the same aggregate guard without relaxing any production cap.
    let limited = discover_bounded(&fixture.0, 20_000);
    assert_eq!(
        limited.removals.status,
        super::super::manifest::LocalDiscoveryStatus::Unavailable
    );
    assert_eq!(limited.summary, full.summary);
    assert_eq!(limited.plugins, full.plugins);
    assert_eq!(limited.usage, full.usage);
}

// Catches one bad package poisoning siblings or a duplicate group selecting a winner.
#[test]
fn invalid_package_is_isolated_and_duplicate_ids_have_no_winner() {
    let fixture = Fixture::new();
    let scan = fixture.scan(&[
        valid_manifest("com.example.dup"),
        valid_manifest("com.example.dup"),
        valid_manifest("com.example.good"),
        br#"{"schemaVersion":1}"#.to_vec(),
        valid_manifest("com.example.dup"),
    ]);
    let outcome = parse_package_scan(scan);
    assert_eq!(outcome.plugins.len(), 1);
    assert_eq!(
        outcome.plugins[0].record.manifest().id.as_str(),
        "com.example.good"
    );
    assert_eq!(outcome.summary, LocalDiscoverySummary::degraded(4).unwrap());
}

// Catches admitting frontend-blank display text or rejecting its valid sibling.
#[test]
fn feff_only_display_text_rejects_each_bad_package_without_poisoning_siblings() {
    for field in ["name", "publisher", "description"] {
        let mut bad: serde_json::Value =
            serde_json::from_slice(&valid_manifest("com.example.bad")).unwrap();
        bad[field] = serde_json::json!("\u{feff}");
        let fixture = Fixture::new();
        let outcome = parse_package_scan(fixture.scan(&[
            serde_json::to_vec(&bad).unwrap(),
            valid_manifest("com.example.good"),
        ]));
        assert_eq!(outcome.plugins.len(), 1, "blank {field} package survived");
        assert_eq!(
            outcome.plugins[0].record.manifest().id.as_str(),
            "com.example.good"
        );
        assert_eq!(outcome.summary, LocalDiscoverySummary::degraded(1).unwrap());
    }
}

// Catches parsing through a lossy Value/string intermediary or accepting reserved functionality.
#[test]
fn strict_package_errors_are_isolated_and_counted() {
    let good = String::from_utf8(valid_manifest("com.example.good")).unwrap();
    let mut invalid_utf8 = good.as_bytes().to_vec();
    invalid_utf8[good.find("Example").unwrap()] = 0xff;
    let invalid = [
        format!("[{good}]"),
        r#"[1,"com.example.a","com.example","Example","Example","Metadata","1.0.0",[],[]]"#.into(),
        good.replacen("{", r#"{"schemaVersion":1,"#, 1),
        good.replacen("{", r#"{"extra":true,"#, 1),
        good.replace(r#""contributions":[]"#, r#""contributions":[{}]"#),
        good.replace(
            r#""requestedCapabilities":[]"#,
            r#""requestedCapabilities":["network"]"#,
        ),
        format!("\u{feff}{good}"),
    ];
    for bytes in invalid
        .into_iter()
        .map(String::into_bytes)
        .chain([invalid_utf8])
    {
        let fixture = Fixture::new();
        let outcome = parse_package_scan(fixture.scan(&[bytes, good.as_bytes().to_vec()]));
        assert_eq!(outcome.plugins.len(), 1);
        assert_eq!(
            outcome.plugins[0].record.manifest().id.as_str(),
            "com.example.good"
        );
        assert_eq!(outcome.summary, LocalDiscoverySummary::degraded(1).unwrap());
    }
}

// Catches package slot order leaking into output or filesystem rejections disappearing.
#[test]
fn canonical_ids_and_trust_records_preserve_scan_rejections() {
    let fixture = Fixture::new();
    let mut scan = fixture.scan(&[valid_manifest("com.zeta"), valid_manifest("com.alpha")]);
    scan.rejected_package_count = 2;
    let outcome = parse_package_scan(scan);
    assert_eq!(
        outcome
            .plugins
            .iter()
            .map(|r| r.record.manifest().id.as_str())
            .collect::<Vec<_>>(),
        ["com.alpha", "com.zeta"]
    );
    assert!(outcome.plugins.iter().all(|r| r.record.source()
        == super::super::manifest::PluginSource::LocalDeclarative
        && r.record.approval_fingerprint().starts_with("v1:sha256:")));
    assert_eq!(outcome.summary, LocalDiscoverySummary::degraded(2).unwrap());
}

// Catches caching a resolver result or conflating missing directories with failures.
#[test]
fn each_discovery_resolves_again_and_root_failures_are_unavailable() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let fixture = Fixture::new();
    let calls = AtomicUsize::new(0);
    let resolve = || {
        let n = calls.fetch_add(1, Ordering::SeqCst);
        (n == 0).then(|| fixture.0.join("missing"))
    };
    assert_eq!(
        discover_with_root(resolve),
        LocalDiscoveryOutcome::available(vec![])
    );
    assert_eq!(
        discover_with_root(resolve),
        LocalDiscoveryOutcome::unavailable()
    );
    fs::write(fixture.0.join("file"), b"{}").unwrap();
    assert_eq!(
        discover_with_root(|| Some(fixture.0.join("file"))),
        LocalDiscoveryOutcome::unavailable()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

// Catches off-by-one admission and overflowing arithmetic in the add-one budget.
#[test]
fn import_budget_accepts_exact_limits_and_rejects_each_excess() {
    let usage = ScanUsage {
        root_entries: 255,
        packages: 127,
        bytes_read: 2_097_151,
    };
    assert!(usage.can_add_manifest(1));
    assert!(!ScanUsage {
        root_entries: 256,
        ..usage
    }
    .can_add_manifest(1));
    assert!(!ScanUsage {
        packages: 128,
        ..usage
    }
    .can_add_manifest(1));
    assert!(!usage.can_add_manifest(2));
    assert!(ScanUsage::default().can_add_manifest(16_384));
    assert!(!ScanUsage::default().can_add_manifest(16_385));
    assert!(!ScanUsage {
        bytes_read: usize::MAX,
        ..usage
    }
    .can_add_manifest(1));
    assert!(!usage.can_add_manifest(usize::MAX));
}

// Catches conflating unknown root usage with authoritative empty-root capacity.
#[test]
fn unavailable_never_reports_zero_usage_as_authoritative() {
    let fixture = Fixture::new();
    let missing = discover_from_root(&fixture.0.join("missing"));
    assert_eq!(missing.summary, LocalDiscoverySummary::available());
    assert_eq!(missing.usage, Some(ScanUsage::default()));
    fs::write(fixture.0.join("file"), b"{}").unwrap();
    for outcome in [
        discover_from_root(&fixture.0.join("file")),
        discover_with_root(|| None),
        LocalDiscoveryOutcome::unavailable(),
    ] {
        assert_eq!(outcome.summary, LocalDiscoverySummary::unavailable());
        assert_eq!(outcome.usage, None);
    }
}

// Catches reconstructing usage from accepted records after strict parsing.
#[test]
fn malformed_packages_and_rejected_probes_remain_in_discovery_usage() {
    let fixture = Fixture::new();
    let outcome = parse_package_scan(fixture.scan(&[b"bad".to_vec(), vec![0; 16_400]]));
    assert!(outcome.plugins.is_empty());
    assert_eq!(outcome.summary, LocalDiscoverySummary::degraded(2).unwrap());
    assert_eq!(
        outcome.usage,
        Some(ScanUsage {
            root_entries: 2,
            packages: 2,
            bytes_read: 16_388,
        })
    );
}
