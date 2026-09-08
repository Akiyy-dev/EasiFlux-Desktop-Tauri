use std::time::{Duration, Instant};

use serde_json::json;
use sha2::{Digest, Sha256};

use super::*;
use crate::error::{AppError, AppResult};
use crate::plugin::manifest::{LocalDiscoverySummary, PluginAvailability, PluginCatalogSnapshot};

fn assert_code<T>(result: AppResult<T>, expected: &str) {
    let error = result.err().expect("operation must fail");
    match &error {
        AppError::Plugin {
            code, diagnostic, ..
        } => {
            assert_eq!(*code, expected);
            assert!(diagnostic.is_none());
        }
        _ => panic!("expected a sanitized plugin error"),
    }
    let wire = serde_json::to_value(error).unwrap();
    assert_eq!(wire.as_object().unwrap().len(), 2);
}

fn ready(sessions: &std::sync::Arc<ImportSessions>, now: Instant) -> ImportPreview {
    let lease: PrepareLease = sessions.reserve_prepare(now).unwrap();
    lease
        .publish(
            PreparedManifest::parse(test_support::VALID).unwrap(),
            4,
            now,
        )
        .unwrap()
}

// Catches noncanonical installation bytes or a changed fingerprint domain/order.
#[test]
fn canonical_bytes_match_fingerprint_input() {
    let content = PreparedManifest::parse(test_support::VALID).unwrap();
    assert_eq!(content.bytes(), test_support::VALID);
    assert_eq!(
        content.record().canonical_manifest_bytes().unwrap(),
        test_support::VALID
    );
    let mut digest = Sha256::new();
    digest.update(b"EasiFlux.localDeclarative.manifest.v1\0");
    digest.update(test_support::VALID);
    assert_eq!(
        content.record().approval_fingerprint(),
        format!("v1:sha256:{}", hex::encode(digest.finalize()))
    );
    let reordered = br#"{ "requestedCapabilities": [], "contributions": [], "version": "1.0.0", "description": "Metadata only", "name": "Notes", "publisher": "Example", "publisherId": "com.example", "id": "com.example.notes", "schemaVersion": 1 }"#;
    let other = PreparedManifest::parse(reordered).unwrap();
    assert_eq!(other.bytes(), content.bytes());
    assert_eq!(other.record().identity(), content.record().identity());
    assert_eq!(content.clone().bytes(), content.bytes());
}

// Catches dropping any semantic identity component from the hash.
#[test]
fn semantic_change_changes_identity() {
    let content = PreparedManifest::parse(test_support::VALID).unwrap();
    let text = std::str::from_utf8(test_support::VALID).unwrap();
    for (old, new) in [
        ("com.example.notes", "com.example.other"),
        ("\"com.example\"", "\"org.example\""),
        ("Example", "Other"),
        ("Notes", "Other"),
        ("Metadata only", "Changed"),
        ("1.0.0", "1.0.1"),
    ] {
        let changed = PreparedManifest::parse(text.replace(old, new).as_bytes()).unwrap();
        assert_ne!(
            changed.record().approval_fingerprint(),
            content.record().approval_fingerprint()
        );
    }
}

// Catches lossy JSON parsing, unbounded direct parsing, and reserved capability acceptance.
#[test]
fn invalid_utf8_bom_duplicate_fields_and_nonempty_contributions_are_rejected() {
    let valid = std::str::from_utf8(test_support::VALID).unwrap();
    let mut invalid_utf8 = test_support::VALID.to_vec();
    // Lossy decoding would turn this into a valid name, so JSON syntax cannot mask the bug.
    invalid_utf8[valid.find("Notes").unwrap()] = 0xff;
    let mut bom = vec![0xef, 0xbb, 0xbf];
    bom.extend_from_slice(test_support::VALID);
    let mut oversized = test_support::VALID.to_vec();
    oversized.resize(16_385, b' ');
    let cases = vec![
        invalid_utf8,
        bom,
        oversized,
        valid
            .replace(
                "\"schemaVersion\":1",
                "\"schemaVersion\":1,\"schemaVersion\":1",
            )
            .into_bytes(),
        valid
            .replace("\"contributions\":[]", "\"contributions\":[{}]")
            .into_bytes(),
        valid
            .replace(
                "\"requestedCapabilities\":[]",
                "\"requestedCapabilities\":[\"network\"]",
            )
            .into_bytes(),
        valid
            .replace("\"schemaVersion\":1", "\"schemaVersion\":2")
            .into_bytes(),
        valid.replace("\"schemaVersion\":1,", "").into_bytes(),
        valid
            .replace("\"schemaVersion\":1", "\"schemaVersion\":1,\"extra\":true")
            .into_bytes(),
        valid.replace("\"1.0.0\"", "\"invalid\"").into_bytes(),
        valid.replace("\"Notes\"", "123").into_bytes(),
        br#"[1,"com.example.notes","com.example","Example","Notes","Metadata only","1.0.0",[],[]]"#
            .to_vec(),
        format!("{valid}{valid}").into_bytes(),
    ];
    for bytes in cases {
        assert_code(
            PreparedManifest::parse(&bytes),
            "plugin_import_manifest_invalid",
        );
    }
    let mut boundary = test_support::VALID.to_vec();
    boundary.resize(16_384, b' ');
    assert_eq!(
        PreparedManifest::parse(&boundary).unwrap().bytes(),
        test_support::VALID
    );
}

// Catches token replay, early expiry, inclusive-boundary mistakes, or cancelling accepted work.
#[test]
fn token_is_one_time_and_expires_at_exactly_300_seconds() {
    let start = Instant::now();
    let sessions = ImportSessions::new();
    let preview = ready(&sessions, start);
    assert_eq!(preview.token.len(), 32);
    assert!(preview
        .token
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert_code(sessions.reserve_prepare(start), "plugin_import_busy");
    let lease: CommitLease = sessions
        .claim_commit(&preview.token, start + Duration::from_secs(299))
        .unwrap();
    assert_eq!(lease.generation(), 4);
    assert_eq!(lease.content().bytes(), test_support::VALID);
    assert_code(
        sessions.claim_commit(&preview.token, start),
        "plugin_import_token_invalid",
    );
    assert_code(sessions.cancel(&preview.token, start), "plugin_import_busy");
    assert_code(
        sessions.reserve_prepare(start + Duration::from_secs(301)),
        "plugin_import_busy",
    );
    drop(lease);
    assert_code(
        sessions.claim_commit(&preview.token, start),
        "plugin_import_token_invalid",
    );
    let second = ready(&sessions, start);
    assert_code(
        sessions.claim_commit(&second.token, start + Duration::from_secs(300)),
        "plugin_import_token_invalid",
    );
    assert!(sessions
        .reserve_prepare(start + Duration::from_secs(300))
        .is_ok());
}

// Catches invalid-token requests consuming someone else's live preview.
#[test]
fn cancel_unknown_is_idempotent_without_cancelling_current_ready() {
    let start = Instant::now();
    let sessions = ImportSessions::new();
    let preview = ready(&sessions, start);
    for token in [
        "",
        "wrong",
        "ABCDEF0123456789ABCDEF0123456789",
        "00000000000000000000000000000000",
    ] {
        sessions.cancel(token, start).unwrap();
        assert_code(
            sessions.claim_commit(token, start),
            "plugin_import_token_invalid",
        );
        assert_code(sessions.reserve_prepare(start), "plugin_import_busy");
    }
    sessions.cancel(&preview.token, start).unwrap();
    sessions.cancel(&preview.token, start).unwrap();
    assert_code(
        sessions.claim_commit(&preview.token, start),
        "plugin_import_token_invalid",
    );
    let second = ready(&sessions, start);
    sessions
        .cancel(&second.token, start + Duration::from_secs(300))
        .unwrap();
    assert!(sessions
        .reserve_prepare(start + Duration::from_secs(300))
        .is_ok());
}

// Catches leaked admission when a picker/reader future is dropped before publishing.
#[test]
fn dropping_prepare_lease_releases_admission() {
    let start = Instant::now();
    let sessions = ImportSessions::new();
    let lease = sessions.reserve_prepare(start).unwrap();
    sessions.cancel("unknown", start).unwrap();
    assert_code(sessions.reserve_prepare(start), "plugin_import_busy");
    drop(lease);
    assert!(sessions.reserve_prepare(start).is_ok());
}

// Catches a stale destructor clearing an unrelated owner (test-only duplicate leases).
#[test]
fn dropping_old_owner_cannot_release_new_session() {
    let start = Instant::now();
    let sessions = ImportSessions::new();
    let first = sessions.reserve_prepare(start).unwrap();
    let old = session::test_support::duplicate_prepare(&first);
    let stale_publisher = session::test_support::duplicate_prepare(&first);
    drop(first);
    let next = sessions.reserve_prepare(start).unwrap();
    drop(old);
    assert_code(sessions.reserve_prepare(start), "plugin_import_busy");
    assert_code(
        stale_publisher.publish(
            PreparedManifest::parse(test_support::VALID).unwrap(),
            99,
            start,
        ),
        "plugin_import_busy",
    );
    let owner = session::test_support::prepare_owner(&next);
    let preview = next
        .publish(
            PreparedManifest::parse(test_support::VALID).unwrap(),
            4,
            start,
        )
        .unwrap();
    assert_ne!(owner.simple().to_string(), preview.token);
    let lease = sessions.claim_commit(&preview.token, start).unwrap();
    let stale = session::test_support::duplicate_commit(&lease);
    drop(lease);
    let second = ready(&sessions, start);
    let next = sessions.claim_commit(&second.token, start).unwrap();
    drop(stale);
    assert_code(sessions.reserve_prepare(start), "plugin_import_busy");
    drop(next);
}

// Catches expiry being applied only to commit, or starting the TTL at admission instead of publication.
#[test]
fn prepare_and_cancel_expire_ready_at_the_publication_deadline() {
    let start = Instant::now();
    for cancel_first in [false, true] {
        let sessions = ImportSessions::new();
        let preparing = sessions.reserve_prepare(start).unwrap();
        let published = start + Duration::from_secs(600);
        let preview = preparing
            .publish(
                PreparedManifest::parse(test_support::VALID).unwrap(),
                4,
                published,
            )
            .unwrap();
        assert_code(
            sessions.reserve_prepare(published + Duration::from_secs(299)),
            "plugin_import_busy",
        );
        let deadline = published + Duration::from_secs(300);
        if cancel_first {
            sessions.cancel("unknown", deadline).unwrap();
            assert!(session::test_support::is_idle(&sessions));
        }
        let next = sessions.reserve_prepare(deadline).unwrap();
        assert_code(
            sessions.claim_commit(&preview.token, deadline),
            "plugin_import_token_invalid",
        );
        drop(next);
    }
}

// Catches non-atomic token consumption under two callers racing the same ready session.
#[test]
fn concurrent_commit_claims_have_exactly_one_owner() {
    let now = Instant::now();
    let sessions = ImportSessions::new();
    let preview = ready(&sessions, now);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let callers: Vec<_> = (0..2)
        .map(|_| {
            let sessions = std::sync::Arc::clone(&sessions);
            let barrier = std::sync::Arc::clone(&barrier);
            let token = preview.token.clone();
            std::thread::spawn(move || {
                barrier.wait();
                sessions.claim_commit(&token, now)
            })
        })
        .collect();
    barrier.wait();
    let mut winners = Vec::new();
    let mut rejected = 0;
    for caller in callers {
        match caller.join().unwrap() {
            Ok(lease) => winners.push(lease),
            Err(error) => {
                assert_code::<()>(Err(error), "plugin_import_token_invalid");
                rejected += 1;
            }
        }
    }
    assert_eq!(winners.len(), 1);
    assert_eq!(rejected, 1);
    assert_code(sessions.reserve_prepare(now), "plugin_import_busy");
    drop(winners);
    assert!(sessions.reserve_prepare(now).is_ok());
}

// Catches changes to any closed result branch, fixed schema, or canonical generation encoding.
#[test]
fn import_result_wire_keys_match_spec() {
    let now = Instant::now();
    let sessions = ImportSessions::new();
    let preview = sessions
        .reserve_prepare(now)
        .unwrap()
        .publish(
            PreparedManifest::parse(test_support::VALID).unwrap(),
            u64::MAX,
            now,
        )
        .unwrap();
    let expected = json!({"schemaVersion":1,"status":"ready","token":preview.token,"expiresInSeconds":300,"catalogGeneration":"18446744073709551615","manifest":serde_json::from_slice::<serde_json::Value>(test_support::VALID).unwrap()});
    assert_eq!(serde_json::to_value(&preview).unwrap(), expected);
    assert_eq!(
        serde_json::to_value(PrepareImportResult::Ready(preview)).unwrap(),
        expected
    );
    assert_eq!(
        serde_json::to_value(PrepareImportResult::cancelled()).unwrap(),
        json!({"schemaVersion":1,"status":"cancelled"})
    );
    assert_eq!(
        serde_json::to_value(CancelImportResult::new()).unwrap(),
        json!({"schemaVersion":1,"status":"cancelled"})
    );
    let snapshot = PluginCatalogSnapshot::new(
        "9".into(),
        "4".into(),
        PluginAvailability::Available,
        None,
        LocalDiscoverySummary::available(),
        crate::plugin::manifest::ManagedOwnershipSummary::available(),
        vec![],
    );
    assert_eq!(
        serde_json::to_value(CommitImportResult::imported(
            "com.example.notes".into(),
            snapshot.clone()
        ))
        .unwrap(),
        json!({"schemaVersion":2,"status":"imported","pluginId":"com.example.notes","snapshot":snapshot})
    );
    assert_eq!(
        serde_json::to_value(CommitImportResult::imported_not_visible(
            "com.example.notes".into(),
            snapshot.clone()
        ))
        .unwrap(),
        json!({"schemaVersion":2,"status":"importedNotVisible","pluginId":"com.example.notes","reasonCode":"plugin_import_publication_unconfirmed","snapshot":snapshot})
    );
    assert_eq!(
        serde_json::to_value(CommitImportResult::imported_external(
            "com.example.notes".into(),
            snapshot.clone()
        ))
        .unwrap(),
        json!({"schemaVersion":2,"status":"importedExternal","pluginId":"com.example.notes","reasonCode":"plugin_import_ownership_not_registered","snapshot":snapshot})
    );
    for (reason, code) in [
        (
            ImportCommitFailure::OwnershipUnavailable,
            "plugin_ownership_unavailable",
        ),
        (
            ImportCommitFailure::OwnershipCapacityExceeded,
            "plugin_ownership_capacity_exceeded",
        ),
        (
            ImportCommitFailure::OwnershipRevisionExhausted,
            "plugin_ownership_revision_exhausted",
        ),
        (ImportCommitFailure::CatalogStale, "plugin_catalog_stale"),
        (
            ImportCommitFailure::CatalogInvalid,
            "plugin_catalog_invalid",
        ),
        (
            ImportCommitFailure::CatalogGenerationExhausted,
            "plugin_catalog_generation_exhausted",
        ),
        (
            ImportCommitFailure::StateUnavailable,
            "plugin_state_unavailable",
        ),
        (
            ImportCommitFailure::StatePersistFailed,
            "plugin_state_persist_failed",
        ),
        (
            ImportCommitFailure::StateCapacityExceeded,
            "plugin_state_capacity_exceeded",
        ),
        (
            ImportCommitFailure::RevisionExhausted,
            "plugin_revision_exhausted",
        ),
        (ImportCommitFailure::IdConflict, "plugin_import_id_conflict"),
        (
            ImportCommitFailure::DiscoveryUnavailable,
            "plugin_import_discovery_unavailable",
        ),
        (
            ImportCommitFailure::CapacityExceeded,
            "plugin_import_capacity_exceeded",
        ),
        (
            ImportCommitFailure::StagingCapacityExceeded,
            "plugin_import_staging_capacity_exceeded",
        ),
        (
            ImportCommitFailure::WriteFailed,
            "plugin_import_write_failed",
        ),
    ] {
        assert_eq!(reason.as_code(), code);
        let saved_values: &[bool] = if reason == ImportCommitFailure::WriteFailed {
            &[false, true]
        } else {
            &[false]
        };
        for &saved in saved_values {
            assert_eq!(
                serde_json::to_value(CommitImportResult::not_imported(
                    reason,
                    saved,
                    snapshot.clone()
                ))
                .unwrap(),
                json!({"schemaVersion":2,"status":"notImported","disabledDecisionSaved":saved,"reasonCode":code,"snapshot":snapshot})
            );
        }
    }
}

// Catches a selector contract that cannot be shared or awaited in an owned worker.
#[test]
fn selector_and_its_future_are_send_and_sync_as_required() {
    fn shared<T: Send + Sync + ?Sized>() {}
    fn send<T: Send>(_: T) {}
    shared::<dyn LocalManifestSelector>();
    struct CancelSelector;
    impl LocalManifestSelector for CancelSelector {
        fn select(
            &self,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = AppResult<SelectedManifestSource>> + Send + '_>,
        > {
            Box::pin(async { Ok(SelectedManifestSource::Cancelled) })
        }
    }
    send(CancelSelector.select());
    let selected = SelectedManifestSource::Selected(std::path::PathBuf::from("fixture.json"));
    assert!(
        matches!(selected, SelectedManifestSource::Selected(path) if path == std::path::Path::new("fixture.json"))
    );
}
