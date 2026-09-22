use super::super::store::*;
use std::path::PathBuf;

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("strategy-tests")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// Catches first-valid fallback, pending omission and ambiguous same-revision acceptance.
#[test]
fn highest_revision_wins_but_any_corrupt_candidate_fails_closed() {
    let dir = Workspace::new();
    let store = FileStrategyStore::new(dir.0.clone());
    for (name, revision) in [
        ("strategy.json", 1),
        ("strategy.json.bak", 0),
        ("strategy.json.pending", 2),
    ] {
        std::fs::write(
            dir.0.join(name),
            format!(r#"{{"schemaVersion":1,"revision":"{revision}","runs":[]}}"#),
        )
        .unwrap();
    }
    assert_eq!(store.load().unwrap().revision, "2");
    std::fs::write(dir.0.join("strategy.json.bak.pending"), b"broken").unwrap();
    assert!(store.load().is_err());
}
#[test]
fn save_advances_revision_and_never_overwrites_a_newer_candidate() {
    let dir = Workspace::new();
    let store = FileStrategyStore::new(dir.0.clone());
    let mut doc = StrategyDocument::default();
    doc.revision = "1".into();
    assert!(store.save(&doc).is_ok());
    assert_eq!(store.load().unwrap(), doc);
    assert!(store.save(&doc).is_err());
    std::fs::write(dir.0.join("strategy.json.tmp"), b"{malformed}").unwrap();
    doc.revision = "2".into();
    assert!(store.save(&doc).is_err());
}
#[test]
fn document_requires_an_object_and_rejects_unknown_fields() {
    assert!(serde_json::from_str::<StrategyDocument>(r#"[1,"0",[]]"#).is_err());
    assert!(serde_json::from_str::<StrategyDocument>(
        r#"{"schemaVersion":1,"revision":"0","runs":[],"secret":"x"}"#
    )
    .is_err());
}
#[test]
fn conflicting_same_revision_and_oversized_candidates_never_fall_back() {
    let dir = Workspace::new();
    let store = FileStrategyStore::new(dir.0.clone());
    std::fs::write(
        dir.0.join("strategy.json"),
        br#"{"schemaVersion":1,"revision":"1","runs":[]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.0.join("strategy.json.tmp"),
        vec![b' '; 16 * 1024 * 1024 + 1],
    )
    .unwrap();
    assert!(store.load().is_err());
    std::fs::write(
        dir.0.join("strategy.json.tmp"),
        br#"{"schemaVersion":1,"revision":"01","runs":[]}"#,
    )
    .unwrap();
    assert!(store.load().is_err());
}

// Break: the strategy-specific all-candidates load accidentally bypasses the
// existing no-follow barrier at a candidate which used to be ignored.
#[cfg(windows)]
#[test]
fn strategy_candidates_refuse_windows_junctions_without_touching_the_target() {
    for name in [
        "strategy.json",
        "strategy.json.tmp",
        "strategy.json.bak",
        "strategy.json.pending",
        "strategy.json.bak.pending",
    ] {
        let dir = Workspace::new();
        let outside = Workspace::new();
        std::fs::write(outside.0.join("keep"), b"untouched").unwrap();
        let link = dir.0.join(name);
        let result = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside.0)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "junction fixture unavailable: {result:?}"
        );
        let store = FileStrategyStore::new(dir.0.clone());
        let rejected = store.load().is_err();
        let untouched = std::fs::read(outside.0.join("keep")).unwrap();
        std::fs::remove_dir(&link).unwrap();
        assert!(rejected, "candidate {name} was followed");
        assert_eq!(untouched, b"untouched");
    }
}
