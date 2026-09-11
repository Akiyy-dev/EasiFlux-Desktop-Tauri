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
    assert_eq!(outcome.records.len(), 1);
    assert_eq!(
        outcome.records[0].manifest().id.as_str(),
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
        assert_eq!(outcome.records.len(), 1, "blank {field} package survived");
        assert_eq!(
            outcome.records[0].manifest().id.as_str(),
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
        assert_eq!(outcome.records.len(), 1);
        assert_eq!(
            outcome.records[0].manifest().id.as_str(),
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
            .records
            .iter()
            .map(|r| r.manifest().id.as_str())
            .collect::<Vec<_>>(),
        ["com.alpha", "com.zeta"]
    );
    assert!(outcome.records.iter().all(|r| r.source()
        == super::super::manifest::PluginSource::LocalDeclarative
        && r.approval_fingerprint().starts_with("v1:sha256:")));
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
