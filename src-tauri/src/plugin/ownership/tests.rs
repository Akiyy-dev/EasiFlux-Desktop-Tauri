use crate::plugin::manifest::PluginManifestV1;
use crate::plugin::ownership::{
    FileIdentity, ManagedOwnershipEntryV1, ManagedOwnershipIndexV1, OwnershipFailure,
    OwnershipReceiptV1, PackageSlot, ReceiptId, RemovalSlot, VerifiedPackageReceipt,
    MAX_MANAGED_OWNERSHIP_BYTES, MAX_MANAGED_OWNERSHIP_ENTRIES, MAX_OWNERSHIP_RECEIPT_BYTES,
};
use crate::plugin::record::PluginRecord;

const MANIFEST_TEMPLATE: &str = r#"{
    "schemaVersion": 1,
    "id": "PLUGIN_ID",
    "publisherId": "com.example",
    "publisher": "Example",
    "name": "Notes",
    "description": "Metadata only",
    "version": "1.0.0",
    "contributions": [],
    "requestedCapabilities": []
}"#;

fn local_record(id: &str) -> PluginRecord {
    let manifest: PluginManifestV1 =
        serde_json::from_str(&MANIFEST_TEMPLATE.replace("PLUGIN_ID", id)).unwrap();
    PluginRecord::local_declarative(manifest).unwrap()
}

fn uuid_v4_simple(value: u64) -> String {
    format!("{value:012x}40008000{value:012x}")
}

fn receipt_id(value: u64) -> ReceiptId {
    ReceiptId::parse(&uuid_v4_simple(value)).unwrap()
}

fn package_slot(value: u64) -> PackageSlot {
    PackageSlot::parse(&format!("pkg-{}", uuid_v4_simple(value))).unwrap()
}

fn removal_slot(value: u64) -> RemovalSlot {
    RemovalSlot::parse(&format!("remove-{}", uuid_v4_simple(value))).unwrap()
}

fn identity(value: u128) -> FileIdentity {
    FileIdentity {
        volume: 0x2a,
        object: value,
    }
}

fn managed_entry(id: &str, value: u64) -> ManagedOwnershipEntryV1 {
    let record = local_record(id);
    let receipt = OwnershipReceiptV1::new(receipt_id(value), package_slot(value), &record).unwrap();
    let verified = VerifiedPackageReceipt {
        canonical_sha256: receipt.canonical_sha256(),
        model: receipt,
        file_identity: identity(u128::from(value) * 3 + 3),
    };
    ManagedOwnershipEntryV1::managed(
        verified,
        &record,
        identity(u128::from(value) * 3 + 1),
        identity(u128::from(value) * 3 + 2),
    )
    .unwrap()
}

fn managed_entry_with(
    id: &str,
    receipt_id: ReceiptId,
    package_slot: PackageSlot,
    directory_identity: FileIdentity,
    manifest_identity: FileIdentity,
    receipt_identity: FileIdentity,
) -> ManagedOwnershipEntryV1 {
    let record = local_record(id);
    let receipt = OwnershipReceiptV1::new(receipt_id, package_slot, &record).unwrap();
    let verified = VerifiedPackageReceipt {
        canonical_sha256: receipt.canonical_sha256(),
        model: receipt,
        file_identity: receipt_identity,
    };
    ManagedOwnershipEntryV1::managed(verified, &record, directory_identity, manifest_identity)
        .unwrap()
}

fn replace_once(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .replacen(from, to, 1)
        .into_bytes()
}

#[test]
fn canonical_receipt_binds_slot_and_manifest_identity() {
    let record = local_record("com.example.notes");
    let receipt = OwnershipReceiptV1::new(
        ReceiptId::parse("91a76dcf6dfb4d44a61b34b876ae486d").unwrap(),
        PackageSlot::parse("pkg-b99c92da6ef54d1f942c1ec706892a99").unwrap(),
        &record,
    )
    .unwrap();
    let bytes = receipt.canonical_bytes().unwrap();
    assert!(bytes.len() <= MAX_OWNERSHIP_RECEIPT_BYTES);
    assert_eq!(OwnershipReceiptV1::parse(&bytes).unwrap(), receipt);
    assert!(receipt.matches_record(&record));
    assert_eq!(hex::encode(receipt.canonical_sha256()).len(), 64);
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        format!(
            concat!(
                r#"{{"schemaVersion":1,"receiptId":"91a76dcf6dfb4d44a61b34b876ae486d","#,
                r#""packageSlot":"pkg-b99c92da6ef54d1f942c1ec706892a99","#,
                r#""source":"localDeclarative","pluginId":"com.example.notes","#,
                r#""publisherId":"com.example","approvalFingerprint":"{}"}}"#
            ),
            record.approval_fingerprint()
        )
    );
}

#[test]
fn receipt_rejects_duplicate_unknown_missing_and_positional_fields() {
    let receipt = OwnershipReceiptV1::new(
        receipt_id(1),
        package_slot(1),
        &local_record("com.example.notes"),
    )
    .unwrap();
    let bytes = receipt.canonical_bytes().unwrap();

    let duplicate = replace_once(
        &bytes,
        r#"{"schemaVersion":1,"#,
        r#"{"schemaVersion":1,"schemaVersion":1,"#,
    );
    let unknown = replace_once(
        &bytes,
        r#"{"schemaVersion":1,"#,
        r#"{"unknown":true,"schemaVersion":1,"#,
    );
    let missing = replace_once(&bytes, r#""source":"localDeclarative","#, "");

    for invalid in [duplicate, unknown, missing, br#"[]"#.to_vec()] {
        assert!(OwnershipReceiptV1::parse(&invalid).is_err());
    }
}

#[test]
fn receipt_rejects_4097_bytes_and_noncanonical_identifiers() {
    assert!(OwnershipReceiptV1::parse(&vec![b' '; MAX_OWNERSHIP_RECEIPT_BYTES + 1]).is_err());

    let receipt = OwnershipReceiptV1::new(
        ReceiptId::parse("91a76dcf6dfb4d44a61b34b876ae486d").unwrap(),
        PackageSlot::parse("pkg-b99c92da6ef54d1f942c1ec706892a99").unwrap(),
        &local_record("com.example.notes"),
    )
    .unwrap();
    let bytes = receipt.canonical_bytes().unwrap();
    let canonical_id = "91a76dcf6dfb4d44a61b34b876ae486d";
    let uppercase_id = canonical_id.to_ascii_uppercase();
    let uppercase_receipt = replace_once(&bytes, canonical_id, &uppercase_id);
    let canonical_slot_id = "b99c92da6ef54d1f942c1ec706892a99";
    let uppercase_slot_id = canonical_slot_id.to_ascii_uppercase();
    let uppercase_slot = replace_once(
        &bytes,
        &format!("pkg-{canonical_slot_id}"),
        &format!("pkg-{uppercase_slot_id}"),
    );
    let future_schema = replace_once(&bytes, r#""schemaVersion":1"#, r#""schemaVersion":2"#);
    let built_in = replace_once(
        &bytes,
        r#""source":"localDeclarative""#,
        r#""source":"builtIn""#,
    );

    for invalid in [uppercase_receipt, uppercase_slot, future_schema, built_in] {
        assert!(OwnershipReceiptV1::parse(&invalid).is_err());
    }
}

#[test]
fn slot_and_receipt_id_require_exact_lower_hex() {
    let valid = "91a76dcf6dfb4d44a61b34b876ae486d";
    assert!(ReceiptId::parse(valid).is_ok());
    assert!(PackageSlot::parse(&format!("pkg-{valid}")).is_ok());
    assert!(RemovalSlot::parse(&format!("remove-{valid}")).is_ok());

    for invalid in [
        "91A76DCF6DFB4D44A61B34B876AE486D",
        "91a76dcf6dfb4d44a61b34b876ae486",
        "91a76dcf6dfb4d44a61b34b876ae486dd",
        "g1a76dcf6dfb4d44a61b34b876ae486d",
    ] {
        assert!(ReceiptId::parse(invalid).is_err());
        assert!(PackageSlot::parse(&format!("pkg-{invalid}")).is_err());
        assert!(RemovalSlot::parse(&format!("remove-{invalid}")).is_err());
    }
    assert!(PackageSlot::parse(&format!("remove-{valid}")).is_err());
    assert!(RemovalSlot::parse(&format!("pkg-{valid}")).is_err());
}

#[test]
fn slots_accept_non_uuid_lower_hex_while_receipts_require_uuid_v4() {
    const NON_UUID_HEX: &str = "00000000000000000000000000000000";

    assert_eq!(
        PackageSlot::parse("pkg-00000000000000000000000000000000")
            .unwrap()
            .as_str(),
        "pkg-00000000000000000000000000000000"
    );
    assert_eq!(
        RemovalSlot::parse("remove-00000000000000000000000000000000")
            .unwrap()
            .as_str(),
        "remove-00000000000000000000000000000000"
    );
    assert!(ReceiptId::parse(NON_UUID_HEX).is_err());
}

#[test]
fn object_identity_wire_is_fixed_width_lower_hex() {
    let entry = managed_entry("com.example.notes", 3);
    let index = ManagedOwnershipIndexV1::empty().register(entry).unwrap();
    let bytes = index.canonical_bytes().unwrap();
    let json = String::from_utf8(bytes.clone()).unwrap();
    assert!(json.contains(r#""volume":"000000000000002a""#));
    assert!(json.contains(r#""object":"0000000000000000000000000000000a""#));

    let uppercase = replace_once(&bytes, "000000000000002a", "000000000000002A");
    let short = replace_once(&bytes, "000000000000002a", "2a");
    let prefixed = replace_once(&bytes, "000000000000002a", "0x0000000000002a");
    for invalid in [uppercase, short, prefixed] {
        assert!(ManagedOwnershipIndexV1::parse(&invalid).is_err());
    }
}

#[test]
fn index_rejects_duplicate_receipt_slot_plugin_and_object_identity() {
    let first = managed_entry("com.example.first", 4);
    let index = ManagedOwnershipIndexV1::empty()
        .register(first.clone())
        .unwrap();

    let same_receipt = managed_entry_with(
        "com.example.second",
        first.receipt_id().clone(),
        package_slot(5),
        identity(98),
        identity(99),
        identity(100),
    );
    assert_eq!(
        index.clone().register(same_receipt),
        Err(OwnershipFailure::Conflict)
    );

    let same_slot = managed_entry_with(
        "com.example.second",
        receipt_id(5),
        first.package_slot().clone(),
        identity(101),
        identity(102),
        identity(103),
    );
    assert_eq!(index.register(same_slot), Err(OwnershipFailure::Conflict));

    let same_plugin = managed_entry("com.example.first", 6);
    assert_eq!(index.register(same_plugin), Err(OwnershipFailure::Conflict));

    let same_object = managed_entry_with(
        "com.example.third",
        receipt_id(7),
        package_slot(7),
        first.manifest_identity,
        identity(104),
        identity(105),
    );
    assert_eq!(index.register(same_object), Err(OwnershipFailure::Conflict));
}

#[test]
fn managed_and_removing_have_exact_distinct_shapes() {
    let managed = ManagedOwnershipIndexV1::empty()
        .register(managed_entry("com.example.notes", 8))
        .unwrap();
    let managed_bytes = managed.canonical_bytes().unwrap();
    let managed_json = String::from_utf8(managed_bytes.clone()).unwrap();
    assert!(managed_json.contains(r#""lifecycle":"managed""#));
    assert!(!managed_json.contains("removalSlot"));

    let removing = managed
        .begin_removal(&receipt_id(8), removal_slot(8))
        .unwrap();
    let removing_bytes = removing.canonical_bytes().unwrap();
    let removing_json = String::from_utf8(removing_bytes.clone()).unwrap();
    assert!(removing_json.contains(r#""lifecycle":"removing""#));
    assert!(removing_json.contains(&format!(r#""removalSlot":"remove-{}""#, uuid_v4_simple(8))));

    let managed_with_slot = replace_once(
        &managed_bytes,
        r#""lifecycle":"managed""#,
        &format!(
            r#""lifecycle":"managed","removalSlot":"remove-{}""#,
            uuid_v4_simple(8)
        ),
    );
    let removing_without_slot = replace_once(
        &removing_bytes,
        &format!(r#","removalSlot":"remove-{}""#, uuid_v4_simple(8)),
        "",
    );
    let managed_with_null_slot = replace_once(
        &managed_bytes,
        r#""lifecycle":"managed""#,
        r#""lifecycle":"managed","removalSlot":null"#,
    );
    assert!(ManagedOwnershipIndexV1::parse(&managed_with_slot).is_err());
    assert!(ManagedOwnershipIndexV1::parse(&removing_without_slot).is_err());
    assert!(ManagedOwnershipIndexV1::parse(&managed_with_null_slot).is_err());
}

#[test]
fn entry_lifecycle_helpers_preserve_identity_and_only_change_the_slot() {
    let managed = managed_entry("com.example.notes", 9);
    let removing = managed.begin_removal(removal_slot(9));
    assert_eq!(removing.removal_slot(), Some(&removal_slot(9)));
    assert_eq!(removing.restore_managed(), managed);
}

#[test]
fn index_accepts_160_rejects_161_and_oversized_serialization() {
    let mut index = ManagedOwnershipIndexV1::empty();
    for value in 1..=MAX_MANAGED_OWNERSHIP_ENTRIES as u64 {
        index = index
            .register(managed_entry(
                &format!("com.example.plugin{value}"),
                value + 100,
            ))
            .unwrap();
    }
    let bytes = index.canonical_bytes().unwrap();
    assert!(bytes.len() <= MAX_MANAGED_OWNERSHIP_BYTES);
    assert_eq!(ManagedOwnershipIndexV1::parse(&bytes).unwrap(), index);

    let next = managed_entry("com.example.overflow", 10_000);
    assert_eq!(
        index.register(next),
        Err(OwnershipFailure::CapacityExceeded)
    );

    let mut oversized = bytes;
    oversized.resize(MAX_MANAGED_OWNERSHIP_BYTES + 1, b' ');
    assert_eq!(
        ManagedOwnershipIndexV1::parse(&oversized),
        Err(OwnershipFailure::CapacityExceeded)
    );
}

#[test]
fn index_revision_max_minus_two_allows_removing_then_one_terminal_transition() {
    let index = ManagedOwnershipIndexV1::empty()
        .register(managed_entry("com.example.notes", 11))
        .unwrap();
    let bytes = replace_once(
        &index.canonical_bytes().unwrap(),
        r#""revision":"1""#,
        &format!(r#""revision":"{}""#, u64::MAX - 2),
    );
    let index = ManagedOwnershipIndexV1::parse(&bytes).unwrap();
    index.require_revision_headroom(2).unwrap();

    let removing = index
        .begin_removal(&receipt_id(11), removal_slot(11))
        .unwrap();
    let terminal = removing.remove(&receipt_id(11)).unwrap();
    assert!(terminal
        .canonical_bytes()
        .unwrap()
        .starts_with(format!(r#"{{"schemaVersion":1,"revision":"{}""#, u64::MAX).as_bytes()));
}

#[test]
fn index_revision_max_minus_one_has_no_two_transition_headroom() {
    let index = ManagedOwnershipIndexV1::empty()
        .register(managed_entry("com.example.notes", 12))
        .unwrap();
    let bytes = replace_once(
        &index.canonical_bytes().unwrap(),
        r#""revision":"1""#,
        &format!(r#""revision":"{}""#, u64::MAX - 1),
    );
    let index = ManagedOwnershipIndexV1::parse(&bytes).unwrap();
    assert_eq!(
        index.require_revision_headroom(2),
        Err(OwnershipFailure::RevisionExhausted)
    );
    assert_eq!(
        index.begin_removal(&receipt_id(12), removal_slot(12)),
        Err(OwnershipFailure::RevisionExhausted)
    );
}

#[test]
fn index_revision_max_rejects_change() {
    let index = ManagedOwnershipIndexV1::empty()
        .register(managed_entry("com.example.notes", 13))
        .unwrap();
    let bytes = replace_once(
        &index.canonical_bytes().unwrap(),
        r#""revision":"1""#,
        &format!(r#""revision":"{}""#, u64::MAX),
    );
    let index = ManagedOwnershipIndexV1::parse(&bytes).unwrap();
    assert_eq!(
        index.register(managed_entry("com.example.other", 14)),
        Err(OwnershipFailure::RevisionExhausted)
    );
    assert_eq!(
        index.remove(&receipt_id(13)),
        Err(OwnershipFailure::RevisionExhausted)
    );
}

#[test]
fn canonical_noop_does_not_increment_revision() {
    let entry = managed_entry("com.example.notes", 15);
    let target_receipt_id = entry.receipt_id().clone();
    let managed = ManagedOwnershipIndexV1::empty()
        .register(entry.clone())
        .unwrap();
    assert_eq!(managed.register(entry).unwrap(), managed);
    assert_eq!(
        managed.restore_managed(&target_receipt_id).unwrap(),
        managed
    );
    assert_eq!(managed.remove(&receipt_id(99)).unwrap(), managed);

    let removing = managed
        .begin_removal(&target_receipt_id, removal_slot(15))
        .unwrap();
    assert_eq!(
        removing
            .begin_removal(&target_receipt_id, removal_slot(15))
            .unwrap(),
        removing
    );
}

#[test]
fn receipt_copy_with_different_objects_does_not_equal_entry() {
    let first = managed_entry("com.example.notes", 16);
    let mut copied = first.clone();
    copied.directory_identity = identity(999);
    copied.manifest_identity = identity(1_000);
    copied.receipt_identity = identity(1_001);
    assert_ne!(first, copied);
    assert_eq!(
        ManagedOwnershipIndexV1::empty()
            .register(first)
            .unwrap()
            .register(copied),
        Err(OwnershipFailure::Conflict)
    );
}

#[test]
fn ownership_failure_codes_are_stable() {
    assert_eq!(
        OwnershipFailure::Unavailable.as_code(),
        "plugin_ownership_unavailable"
    );
    assert_eq!(
        OwnershipFailure::PersistFailed.as_code(),
        "plugin_ownership_persist_failed"
    );
    assert_eq!(
        OwnershipFailure::CapacityExceeded.as_code(),
        "plugin_ownership_capacity_exceeded"
    );
    assert_eq!(
        OwnershipFailure::RevisionExhausted.as_code(),
        "plugin_ownership_revision_exhausted"
    );
    assert_eq!(
        OwnershipFailure::Conflict.as_code(),
        "plugin_ownership_conflict"
    );
}
