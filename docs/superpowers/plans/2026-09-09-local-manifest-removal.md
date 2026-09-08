# Managed Local Manifest Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add provable managed ownership for newly imported metadata-only local packages and allow users to remove only disabled packages whose receipt, ownership index, slot, manifest identity, and filesystem identities all still match.

**Architecture:** Introduce a strict package receipt and an independent bounded ownership index, then extend safe discovery with opaque locators and reconcile those three sources into catalog transport v3. Refactor import/removal filesystem work behind one handle-relative package adapter; import registers ownership only after promotion, while removal persists disabled and `removing` state before an exclusive quarantine rename, builds an unpublished authoritative candidate, attempts one exact non-recursive cleanup, and publishes the final reconciled snapshot once.

**Tech Stack:** Rust, Tauri 2, Tokio, serde, SHA-256, UUID, rustix, windows-sys, libc, Vue 3, Pinia, TypeScript, Vitest/Vue Test Utils, existing native HTML dialog and three-platform GitHub Actions matrix.

**Spec:** `docs/superpowers/specs/2026-09-09-local-manifest-removal-design.md` — implement the complete approved revision at `dd2cdeb`; executors must read it in full before every task.

## Global Constraints

- `pkg-*` names, manifest fingerprints, plugin-state decisions, package shape, an isolated receipt, or an isolated index entry never authorize deletion by themselves.
- Managed status requires one strict package receipt, one exact index entry, the current package slot, the current manifest safety identity, and matching directory/manifest/receipt OS object identities.
- Historical Phase 1A/1B packages without both receipt and index remain `external`; do not auto-adopt, backfill, hide, or grant removal.
- Only `localDeclarative + managed + disabled + available plugin-state` may be removed. There is no combined “disable and remove” operation.
- Do not remove, move, or claim external, built-in, blocked, enabled, ownership-conflicted, or ownership-unavailable packages.
- Do not execute contributions, scripts, HTML, JavaScript, WASM, dynamic libraries, native programs, or resources; contributions and requested/granted capabilities remain empty.
- Do not implement download, update, overwrite, downgrade, package-version rollback, signing, publisher authentication, trust roots, remote indexes, capability grants, adoption, bulk removal, or cleanup UI. The required `removing -> managed` index recovery is not a package-version rollback.
- Removal request payloads never contain paths, package/removal slots, receipt IDs, fingerprints, object IDs, receipts, raw manifests, publishers, or source claims; they contain only plugin ID and expected catalog generation.
- Preserve plugin-state schema 2 and manifest schema 1. Upgrade catalog and mutation transport to schema 3, and import commit transport to schema 2; receipt/index schemas begin at 1.
- Keep `state.json` disabled identities after removal; do not clear orphan state.
- Ownership failure may disable managed import/removal, but must not block application startup, built-ins, or read-only local discovery.
- Import order is exact two-file stage -> persist disabled -> exclusive promotion -> reopen and verify target -> atomically register ownership -> authoritative scan/reconcile/publication.
- Import promotion remains its filesystem commit point. A later ownership-registration failure preserves the package; return `importedExternal` only when the final snapshot proves it external, otherwise `importedNotVisible`. Never delete the promoted package or auto-register it during reload/startup.
- Removal order is gate/preflight -> reopen and verify -> reserve exactly one removal slot -> persist disabled through the platform barrier -> persist `removing` through the platform barrier -> final source/target revalidation -> one exclusive quarantine rename -> authoritative candidate scan -> at most one exact cleanup attempt -> optional index deletion -> one final reconciliation/publication.
- Quarantine rename is the removal commit point. Any unconfirmed state after it is committed pending/unknown, never `notRemoved`, and never automatically replays rename or cleanup.
- Before the removal commit point, restore `managed` only when rename is known not to have happened; a failed restoration publishes removal-pending/degraded rather than guessing.
- Reload/startup may perform only no-delete convergence: restore managed when exact source exists and target does not, or delete a stale index entry when both locations are absent. They never unlink/rmdir quarantine.
- Cleanup is non-recursive, revalidates exact owned identities, unlinks manifest before receipt, accepts only the recorded monotonic known-file subset, and stops on any unknown/replaced object. Never use `remove_dir_all`.
- `local`, `import-staging`, and `removal-staging` are opened from one verified plugins parent and must remain on one volume. Never fall back to copying, overwriting, absolute-path rename, or an exists-then-destructive sequence.
- Linux uses held dirfds and `renameat2(RENAME_NOREPLACE)`; macOS uses held dirfds and `renameatx_np(RENAME_EXCL)`. Both Unix platforms rely on the supported single-writer/no-manual-edit boundary: adjacent checks are best effort and do not promise detection of every same-user name swap. Windows uses component-relative `NtCreateFile`, renames the held source-directory handle with `SetFileInformationByHandle(FileRenameInfo)` and `ReplaceIfExists=false`, and cleans each verified child through handle-bound disposition without closing and reopening it by path.
- Both `state.json` and `managed-ownership.json` use one secure fixed-name document protocol covering main, `.tmp`, `.bak`, `.pending`, and `.bak.pending`; pending names are write-only and never recovery authorities. Every persist reports `PersistOutcome::{NotCommitted, CommittedProcessCrashSafe, CommittedDurable}` and destructive package rename proceeds only after `save_before_destructive_rename` accepts the platform-specific outcome.
- Keep the single-process FIFO `operation_gate` for initial discovery, reload, state/ownership retry, toggle, import, reconcile, and remove. Accepted owned tasks survive caller cancellation; cross-process writers remain unsupported.
- A remove request revalidates current status while holding the gate. If a same-generation toggle advanced revision and made the item enabled, return `plugin_remove_requires_disabled` before any removal-storage access.
- An open removal confirmation becomes stale on a newer generation, and also when a same-generation higher-revision adoption changes the target status/identity/canRemove state.
- Fast get returns only a complete previously published registry snapshot, never an intermediate index or filesystem transition.
- Every ownership-derived published change (locator/object identity, receipt presence/digest, management, ownership eligibility/toggle block reason, or ownership summary) advances catalog generation, and registry swaps one complete candidate snapshot exactly once. A status-only toggle may change `canRemove` while advancing only state revision; mutation generation remains unchanged and all structural/ownership fields remain identical. Equal `(revision, catalogGeneration)` means semantically identical complete DTOs.
- `rollbackPendingCount`, `cleanupPendingCount`, and `conflictingEntryCount` are bounded, complete, and mutually exclusive per entry/object. Full, receipt-only, empty-directory, and both-absent confirmed committed shapes are cleanup-pending; manifest-only, extra, unmatched, orphaned, or commit-uncertain shapes are conflict.
- All source/receipt/index/OS errors are sanitized. Logs and IPC never reveal config paths, slots, receipt content/IDs, manifest bytes, fingerprints, or object identities.
- Exact budgets: ownership index 262,144 bytes with a 262,145-byte probe; 160 index entries; receipt 4,096 bytes with a 4,097-byte probe; local root 256 entries/128 packages; import staging 16 entries; removal staging 16 entries; manifest 16,384 bytes; local discovery has its own 2,097,152-byte aggregate including receipt bytes, and removal reconciliation has an independent 335,872-byte aggregate (328 KiB) including every manifest/receipt probe.
- Import may use at most four complete independently generated stage/receipt/package-slot attempts. Removal selects and records exactly one `remove-*` slot, attempts rename once, and on a proven collision rolls the index back and ends the request without selecting a replacement slot.
- receiptId is UUID v4 simple lower hex; package slots are `pkg-` plus 32 lower hex; removal slots are `remove-` plus 32 lower hex; serialized object volume/object identities are exactly 16/32 lower hex characters.
- Tests use injected temporary roots and controlled barriers only. Never test destructive behavior against the real user configuration directory.

---

## Workspace and execution rules

Run every command from `D:\EasiFlux\EasiFlux-Desktop-Tauri\target\worktrees\plugin-local-manifest-removal` on branch `plugin/local-manifest-removal`, baseline `dd2cdeb`. Before Task 1 record `git status --short --branch`; do not switch or rebase this worktree. Preserve unrelated edits and stage only each task's exact file list; never use `git add -A`.

Every RED command must run before production changes and fail for the asserted missing behavior, not for an unrelated compiler, dependency, or fixture error. Each GREEN boundary includes focused regressions before its commit. Use `pnpm exec vue-tsc --noEmit`; the repository has no `pnpm typecheck` script. Cargo checks use `--locked` except an explicit dependency-resolution command, and changed Rust leaf files receive scoped rustfmt checks without reformatting pre-existing repository debt.

## File responsibility map and dependency order

| Responsibility | Files | First owner |
| --- | --- | --- |
| Strict receipt, slots, object identity, index model | `plugin/ownership.rs`, `plugin/ownership/tests.rs`, `plugin/record.rs` | Task 1 |
| Secure five-name documents, explicit persist outcomes, ownership/state barriers | `storage/safe_plugin_document.rs`, its platform/tests, `storage/managed_plugin_ownership.rs`, `storage/plugin_state.rs`, tests | Task 2 |
| Opaque discovery locator, independent removal observation, reconciliation, atomic catalog v3 publication | `plugin/discovery.rs`, `discovery/safe_fs.rs`, `plugin/manifest.rs`, `plugin/registry.rs`, `plugin/runtime.rs`, `state.rs`, tests | Task 3 |
| Shared package filesystem and two-file import staging | `storage/local_plugin_package.rs`, platform/tests, `storage/local_plugin_import.rs`, tests | Task 4 |
| Promotion-after registration and import schema 2 | `plugin/runtime/import.rs`, `plugin/import.rs`, runtime/import tests | Task 5 |
| Exact remove result domain and one-slot quarantine/cleanup storage | `plugin/removal.rs`, shared package adapter and tests | Task 6 |
| Removal orchestration, one final publication, and startup/reload no-delete convergence | `plugin/runtime/removal.rs`, runtime/registry tests | Task 7 |
| Fixed command and local-main ACL | `commands/plugin.rs`, `lib.rs`, `build.rs`, capability and permission | Task 8 |
| Strict frontend catalog/import/remove protocols and cross-field validation | `types/plugin.ts`, `services/pluginService.ts`, service tests | Task 9 |
| Removal flight and common snapshot arbitration | `stores/plugin.ts`, store tests | Task 10 |
| Accessible removal UI, management labels, guidance | plugin components/CSS/tests, user guide | Task 11 |
| Crash matrix, platform CI, complete regression | Rust integration tests, workflow and workflow test | Task 12 |

### Task 1: Define strict receipts, slots, identities, and ownership-index transactions

**Files:**
- Modify: `src-tauri/src/plugin/mod.rs`
- Modify: `src-tauri/src/plugin/record.rs`
- Create: `src-tauri/src/plugin/ownership.rs`
- Create: `src-tauri/src/plugin/ownership/tests.rs`

**Interfaces:**
- Consumes: `PluginRecord`, `PluginId`, `PluginPublisherId`, `PluginSource`, SHA-256 and UUID.
- Produces: `ReceiptId::parse(&str)`, `PackageSlot::parse(&str)`, `RemovalSlot::parse(&str)`, each canonical and opaque.
- Produces: `FileIdentity { pub(crate) volume: u64, pub(crate) object: u128 }` with exact fixed-width lower-hex wire conversion but no transport serialization.
- Produces: `OwnershipReceiptV1::new(receipt_id: ReceiptId, package_slot: PackageSlot, record: &PluginRecord)`, `parse(bytes: &[u8])`, `canonical_bytes()`, `canonical_sha256()` and `matches_record(&PluginRecord)`.
- Produces: `VerifiedPackageReceipt { model: OwnershipReceiptV1, canonical_sha256: [u8; 32], file_identity: FileIdentity }`.
- Produces: `ManagedOwnershipEntryV1::{managed(...), begin_removal(RemovalSlot), restore_managed(), receipt_id(), package_slot(), removal_slot()}` and exact `managed`/`removing` wire branches.
- Produces: `ManagedOwnershipIndexV1::{empty(), parse(), canonical_bytes(), validate(), validate_for_persistence(), require_revision_headroom(steps), register(entry), begin_removal(receipt_id, slot), restore_managed(receipt_id), remove(receipt_id)}`. Mutators return a cloned validated candidate and checked-increment revision only for a real logical change.
- Produces: `OwnershipFailure::{Unavailable, PersistFailed, CapacityExceeded, RevisionExhausted, Conflict}` and `as_code() -> &'static str`.
- Makes `PluginRecord::approval_fingerprint(&self) -> &str` crate-visible in production; `PluginRecord` still carries no locator or filesystem ownership.

- [ ] **Step 1: Write RED strict-domain tests**

Create exact fixture builders and this first kernel in `plugin/ownership/tests.rs`:

```rust
#[test]
fn canonical_receipt_binds_slot_and_manifest_identity() {
    let record = local_record("com.example.notes");
    let receipt = OwnershipReceiptV1::new(
        ReceiptId::parse("91a76dcf6dfb4d44a61b34b876ae486d").unwrap(),
        PackageSlot::parse("pkg-b99c92da6ef54d1f942c1ec706892a99").unwrap(),
        &record,
    ).unwrap();
    let bytes = receipt.canonical_bytes().unwrap();
    assert!(bytes.len() <= MAX_OWNERSHIP_RECEIPT_BYTES);
    assert_eq!(OwnershipReceiptV1::parse(&bytes).unwrap(), receipt);
    assert!(receipt.matches_record(&record));
    assert_eq!(hex::encode(receipt.canonical_sha256()).len(), 64);
}
```

Add named table tests `receipt_rejects_duplicate_unknown_missing_and_positional_fields`, `receipt_rejects_4097_bytes_and_noncanonical_identifiers`, `slot_and_receipt_id_require_exact_lower_hex`, `object_identity_wire_is_fixed_width_lower_hex`, `index_rejects_duplicate_receipt_slot_plugin_and_object_identity`, `managed_and_removing_have_exact_distinct_shapes`, `index_accepts_160_rejects_161_and_oversized_serialization`, `index_revision_max_minus_two_allows_removing_then_one_terminal_transition`, `index_revision_max_minus_one_has_no_two_transition_headroom`, `index_revision_max_rejects_change`, `canonical_noop_does_not_increment_revision`, and `receipt_copy_with_different_objects_does_not_equal_entry`.

- [ ] **Step 2: Run focused RED**

Run: `cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::ownership::tests --lib`

Expected: FAIL to compile because `plugin::ownership`, `OwnershipReceiptV1`, and the index transaction API do not exist.

- [ ] **Step 3: Implement the strict models and canonical encodings**

Use map-only deserialization with `deny_unknown_fields`; do not parse through `serde_json::Value`. The fixed model kernel is:

```rust
pub(crate) const MAX_OWNERSHIP_RECEIPT_BYTES: usize = 4 * 1024;
pub(crate) const MAX_MANAGED_OWNERSHIP_BYTES: usize = 256 * 1024;
pub(crate) const MAX_MANAGED_OWNERSHIP_ENTRIES: usize = 160;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) struct FileIdentity {
    pub(crate) volume: u64,
    pub(crate) object: u128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OwnershipLifecycleV1 {
    Managed,
    Removing { removal_slot: RemovalSlot },
}
```

Serialize index revision as canonical decimal text and identities through private DTOs using `format!("{:016x}", volume)` and `format!("{:032x}", object)`. Reject upper case, signs, prefixes, short/long values, future schemas, duplicate fields, and any source except `LocalDeclarative`. Canonical receipt fields retain the exact order in spec §6. Hash those canonical bytes directly; do not hash source JSON or a receipt `Value`.

Implement index candidate construction as clone -> mutate -> uniqueness/budget validation -> checked revision assignment:

```rust
fn finish_changed(mut next: Self, previous: &Self) -> Result<Self, OwnershipFailure> {
    next.entries.sort_by(|a, b| a.receipt_id().cmp(b.receipt_id()));
    next.validate_for_persistence()?;
    if next.entries == previous.entries { return Ok(previous.clone()); }
    next.revision = previous.revision.checked_add(1)
        .ok_or(OwnershipFailure::RevisionExhausted)?;
    Ok(next)
}
```

Implement `require_revision_headroom(steps)` with `checked_add`; runtime removal passes `2`, while import registration passes `1`. Validate entry count before revision exhaustion so a 161st entry reports capacity. Treat a duplicate active `(pluginId, localDeclarative)`, any reused object identity, receiptId, packageSlot, or removalSlot as conflict. Do not derive any slot/ID from manifest content.

- [ ] **Step 4: Run GREEN and record-domain regressions**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::ownership::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::record::tests --lib
```

Expected: all strict receipt/index cases pass; the existing manifest fingerprint golden behavior is unchanged.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/plugin/mod.rs src-tauri/src/plugin/record.rs src-tauri/src/plugin/ownership.rs src-tauri/src/plugin/ownership/tests.rs
git commit -m "feat(plugin): define managed package ownership records"
```

### Task 2: Introduce the secure five-name document protocol and platform persistence barriers

**Files:**
- Modify: `src-tauri/src/storage/mod.rs`
- Create: `src-tauri/src/storage/safe_plugin_document.rs`
- Create: `src-tauri/src/storage/safe_plugin_document/platform.rs`
- Create: `src-tauri/src/storage/safe_plugin_document/tests.rs`
- Create: `src-tauri/src/storage/managed_plugin_ownership.rs`
- Create: `src-tauri/src/storage/managed_plugin_ownership/tests.rs`
- Modify: `src-tauri/src/storage/plugin_state.rs`
- Modify: `src-tauri/src/storage/plugin_state/tests.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Modify: `src-tauri/src/plugin/registry/tests.rs`
- Modify: `src-tauri/src/plugin/runtime/tests.rs`
- Modify: `src-tauri/src/plugin/runtime/import/tests.rs`
- Modify: `src-tauri/src/commands/plugin.rs`
- Modify: `src-tauri/src/state.rs`

Do not modify `storage/atomic_file.rs`: it remains available to unrelated stores, but neither managed ownership nor a destructive plugin-state save may call its path-based replacement code.

**Interfaces:**
- Consumes: Task 1 `ManagedOwnershipIndexV1`; existing state main/tmp/bak candidate priority and clone -> persist -> adopt rule.
- Produces: `SafeDocumentNames::plugin_state()` and `SafeDocumentNames::managed_ownership()`, each fixing exactly `main`, `tmp`, `bak`, `pending`, and `bak_pending` below one verified plugins parent.
- Produces: `PersistOutcome::{NotCommitted, CommittedProcessCrashSafe, CommittedDurable}`, `PersistFailure { outcome: PersistOutcome }`, and `type PersistResult = Result<PersistOutcome, PersistFailure>`.
- Produces: `SafePluginDocument::{new(root, names, max_bytes), load_candidates(), persist(previous, next)}`; every existing candidate/work object is opened parent-relative, exact-case, no-follow/no-reparse, regular, single-link, bounded, and kept handle-bound through mutation.
- Produces: `persist_outcome_committed(outcome)` and `outcome_satisfies_destructive_barrier(outcome)`: Linux/macOS require `CommittedDurable`; Windows accepts `CommittedProcessCrashSafe`; `NotCommitted` never adopts the candidate.
- Changes: `PluginStatePersistence::save(&PluginStateFileV2) -> PersistResult`; adds `save_before_destructive_rename(&PluginStateFileV2) -> PersistResult`. Ordinary callers adopt any committed outcome, while destructive callers additionally require the platform barrier.
- Produces: `ManagedOwnershipLoad { index, requires_rewrite }`, `ManagedOwnershipPersistence::{load(), save() -> PersistResult}`, and `ManagedOwnershipStore::{new(), with_plugins_root(PathBuf)}`. Missing all three authority candidates is empty/available; a non-main recovery cannot authorize deletion until an equivalent secure rewrite succeeds.
- Produces test-only `SafeDocumentStep::{CleanPending, CleanBackupPending, CreatePending, WritePending, FlushPending, CreateBackupPending, WriteBackupPending, FlushBackupPending, ReplaceBackup, DeleteLegacyTmp, SyncAfterLegacyTmp, ReplaceMain, VerifyMain, SyncParent}`.

- [ ] **Step 1: Write RED protocol, recovery, and outcome tests**

Create the common adapter tests first. The initial contract is:

```rust
#[cfg(unix)]
#[test]
fn main_commit_without_parent_sync_is_committed_but_not_destructive_barrier_safe() {
    let fixture = DocumentFixture::plugin_state();
    fixture.fail_at(SafeDocumentStep::SyncParent);
    let failure = fixture.persist(b"old", b"new").unwrap_err();
    assert_eq!(failure.outcome, PersistOutcome::CommittedProcessCrashSafe);
    assert!(persist_outcome_committed(failure.outcome));
    assert!(!outcome_satisfies_destructive_barrier(failure.outcome));
    assert_eq!(fixture.read_main(), b"new");
}
```

Add `both_documents_use_exactly_five_reserved_names`, `pending_and_backup_pending_are_never_authority_candidates`, `safe_leftovers_are_cleaned_before_exclusive_create`, `unsafe_pending_symlink_reparse_hardlink_or_case_variant_fails_closed`, `every_precommit_fault_reports_not_committed_and_preserves_old_main`, `backup_replace_precedes_legacy_tmp_delete_and_main_replace`, `postcommit_fault_reports_committed_outcome_and_adopts_new_bytes`, `replacement_reopens_to_the_same_source_identity`, `future_or_oversized_primary_is_terminal`, `corrupt_primary_uses_tmp_then_bak_and_requires_rewrite`, and `no_candidates_means_empty_without_creating_the_root`.

For both `state.json*` and `managed-ownership.json*`, table-test all five names against symlink/reparse, hardlink, non-regular, wrong-case, pre-existing unsafe work objects, and a controlled identity swap. On Windows assert work-object replacement uses the held source handle with `ReplaceIfExists=true`; on Unix assert the parent sync separates process-crash-safe from durable. Add a Windows power-loss-boundary test that asserts successful replace reports only `CommittedProcessCrashSafe`, never durable.

In `managed_plugin_ownership/tests.rs`, add `missing_index_is_available_empty`, `valid_main_wins`, `secondary_candidate_requires_rewrite_before_authorization`, `future_or_262145_byte_primary_never_falls_back`, `all_present_candidates_invalid_is_unavailable`, `safe_rewrite_preserves_revision`, and `committed_save_failure_requires_memory_adoption`. Extend `plugin_state/tests.rs` with the same five-name safety matrix and `destructive_state_save_exposes_platform_barrier`.

- [ ] **Step 2: Run focused RED and verify the failure reason**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::safe_plugin_document::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::managed_plugin_ownership::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml destructive_state_save_exposes_platform_barrier --lib
```

Expected: the first two commands fail to compile because the modules and `PersistOutcome` do not exist; the third fails because `PluginStatePersistence` has no destructive-save/outcome contract. Stop if failure comes from an unrelated fixture or dependency error.

- [ ] **Step 3: Implement the minimal handle-bound document transaction**

Open the injected or lazily resolved absolute plugins root component-by-component. `load_candidates` may read only main/tmp/bak and must distinguish “all missing” from “present but invalid”; pending/bak.pending never enter its recovery list. A save executes exactly:

```text
safe-read main/tmp/bak authorities
exact-clean bounded pending and bak.pending leftovers; sync parent
exclusive-create pending; write all; flush file
if previous exists: exclusive-create bak.pending; write previous; flush; object-bound replace bak
exact-delete the old tmp recovery object; sync parent
object-bound replace main from the held pending handle; re-open and compare identity
sync parent on Linux/macOS
```

Never create/truncate a pre-existing name, unlink main before replacement, reopen a verified work object by path, or promote pending/bak.pending to recovery authority. Windows replacement uses `SetFileInformationByHandle(FileRenameInfo)` with the held source and parent handles and validates any replaceable authority first. Unix/macOS use parent-relative operations inside the documented single-writer/no-manual-edit boundary; controlled pre/post identity checks are best effort and must not be described as complete same-user race prevention.

Return `Err(PersistFailure { outcome: NotCommitted })` before main replacement, `Err(...CommittedProcessCrashSafe)` for a Unix post-main/pre-parent-sync failure, `Ok(CommittedDurable)` only after Unix parent sync, and `Ok(CommittedProcessCrashSafe)` after the Windows handle-bound replacement/flush boundary. Registry transactions clone first and adopt `next` whenever the returned outcome is committed, even when the caller receives a sanitized persistence error; only `NotCommitted` retains old memory. `save_before_destructive_rename` preserves the same adoption rule and separately rejects an outcome that does not meet the current platform barrier.

Update every in-tree `PluginStatePersistence` fake listed in this task to return explicit outcomes, with fault fakes able to model both pre-commit and post-commit failures. Implement ownership parsing limits at read time (262,145-byte probe) and exact canonical rewrite without revision churn.

- [ ] **Step 4: Run GREEN plus all persistence-transaction regressions**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::safe_plugin_document::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::managed_plugin_ownership::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::plugin_state::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::registry::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::runtime --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml commands::plugin::tests --lib
```

Expected: each fault checkpoint proves the correct disk/memory boundary; both documents reject unsafe reserved objects; Linux/macOS destructive barriers reject missing parent durability; Windows reports and accepts only process-crash-safe durability; unrelated plugin-state behavior remains green.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/storage/mod.rs src-tauri/src/storage/safe_plugin_document.rs src-tauri/src/storage/safe_plugin_document/platform.rs src-tauri/src/storage/safe_plugin_document/tests.rs src-tauri/src/storage/managed_plugin_ownership.rs src-tauri/src/storage/managed_plugin_ownership/tests.rs src-tauri/src/storage/plugin_state.rs src-tauri/src/storage/plugin_state/tests.rs src-tauri/src/plugin/registry.rs src-tauri/src/plugin/registry/tests.rs src-tauri/src/plugin/runtime/tests.rs src-tauri/src/plugin/runtime/import/tests.rs src-tauri/src/commands/plugin.rs src-tauri/src/state.rs
git commit -m "feat(plugin): secure lifecycle document persistence"
```

### Task 3: Discover opaque locators, reconcile both roots, and publish one complete catalog-v3 snapshot

**Files:**
- Modify: `src-tauri/src/plugin/discovery/safe_fs.rs`
- Modify: `src-tauri/src/plugin/discovery/safe_fs/tests.rs`
- Modify: `src-tauri/src/plugin/discovery.rs`
- Modify: `src-tauri/src/plugin/discovery/tests.rs`
- Modify: `src-tauri/src/plugin/ownership.rs`
- Modify: `src-tauri/src/plugin/ownership/tests.rs`
- Modify: `src-tauri/src/plugin/manifest.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Modify: `src-tauri/src/plugin/registry/tests.rs`
- Modify: `src-tauri/src/plugin/runtime.rs`
- Modify: `src-tauri/src/plugin/runtime/tests.rs`
- Modify: `src-tauri/src/state.rs`

**Interfaces:**
- Consumes: Task 1 receipt/index/identity values and Task 2 ownership persistence/rewrite state.
- Produces: non-serializable `LocalPackageLocator { package_slot, directory_identity, manifest_identity, receipt: Option<VerifiedPackageReceipt> }` with no arbitrary-string/path constructor.
- Produces: `DiscoveredLocalPlugin { record: PluginRecord, locator: LocalPackageLocator }` and `LocalDiscoveryOutcome { plugins, summary, usage, removals }`.
- Produces: `RemovalObservation { removal_slot, directory_identity, manifest: Option<(PluginRecord, FileIdentity)>, receipt: Option<VerifiedPackageReceipt>, shape: RemovalObservationShape }`, `RemovalObservationShape::{Full, ReceiptOnly, ManifestOnly, EmptyDirectory, Unknown}`, and `RemovalDiscoveryOutcome { observations, status, bytes_read }`. Its 328 KiB accounting is independent from local `ScanUsage`; a both-absent state is derived by reconciling an index entry against absent source and target observations.
- Produces: `PluginManagement::{BuiltIn, Managed, External, RemovalPending, OwnershipConflict, OwnershipUnavailable}`, `PluginToggleBlockReason::RemovalPending`, and `ManagedOwnershipSummary { status, conflicting_entry_count, rollback_pending_count, cleanup_pending_count }`.
- Changes: catalog snapshot and mutation schema constants from 2 to 3; items gain `management`, `can_remove`, and `toggle_block_reason_code`; snapshots gain `managed_ownership`.
- Produces: `OwnershipRuntime::{Available { index, requires_rewrite, persistence }, Unavailable { persistence }}`, `ownership_requires_retry()`, and reconciliation that may perform only no-delete convergence.
- Produces: an internal `CatalogPublicationCandidate` containing the entire next locals/locators/classifications/summaries/DTO. `PluginRegistry::publish_candidate` performs checked generation assignment and one atomic in-memory swap; no index transition or intermediate scan is separately visible.
- Changes initialization to `PluginRegistry::initialize(builtins, state_persistence: Box<dyn PluginStatePersistence>, ownership_persistence: Box<dyn ManagedOwnershipPersistence>)`; production `new()` supplies independently retryable system adapters, so either subsystem can fail without aborting `AppState`.

- [ ] **Step 1: Write RED discovery-shape, reconciliation, transport, and generation tests**

Start with a real two-file fixture:

```rust
#[test]
fn scanner_returns_external_and_receipted_candidates_with_internal_identity() {
    let fixture = Fixture::new();
    fixture.write_external("pkg-00000000000000000000000000000001", VALID);
    fixture.write_receipted("pkg-00000000000000000000000000000002", VALID, valid_receipt());
    let outcome = discover_from_plugins_root(fixture.plugins_root());
    assert_eq!(outcome.plugins.len(), 2);
    assert!(outcome.plugins[0].locator.receipt.is_none());
    assert!(outcome.plugins[1].locator.receipt.is_some());
    assert!(outcome.usage.unwrap().bytes_read > VALID.len() * 2);
}
```

Add `exact_receipt_and_index_and_three_objects_is_managed`, `receipt_without_index_is_external`, `index_without_matching_package_is_conflict`, `legacy_manifest_stays_external_when_index_is_unavailable`, `receipted_candidate_becomes_ownership_unavailable_when_index_is_unavailable`, `recovered_index_does_not_authorize_until_safe_rewrite`, `managed_disabled_only_is_removable`, `removing_exact_source_without_target_is_rollback_pending`, and `no_delete_reconcile_rolls_back_exact_source_or_deletes_both_absent_entry`.

Add one table test named `removal_shapes_are_complete_and_mutually_exclusive`: full target, receipt-only target, and empty target directory each increment cleanup only; both absent increments cleanup only when the stale-entry delete is injected to fail; exact source/target-absent increments rollback only when its safe index rollback is injected to fail. Manifest-only, extra child, both sides present, mismatched identity, orphan quarantine, and unconfirmed shape increment conflict only. Assert every entry/object contributes to at most one count, `available` means all zero, `degraded` means at least one nonzero, and `unavailable` carries all zeros.

Add `local_2mib_and_removal_328kib_budgets_are_independent`, proving a 335,873rd removal byte makes ownership unavailable without altering local discovery status, content, or its 2 MiB accounting. Include every 16,385-byte manifest and 4,097-byte receipt probe in the appropriate aggregate.

In `manifest.rs` tests, add `catalog_v3_enforces_management_toggle_and_summary_cross_fields` and `transport_never_contains_locator_receipt_slot_fingerprint_or_object_identity`. In registry tests add:

- `slot_receipt_or_object_change_advances_generation`;
- `management_eligibility_toggle_block_or_summary_change_advances_generation`;
- `status_only_toggle_changes_can_remove_with_revision_only`;
- `mutation_keeps_generation_and_every_structural_ownership_field`;
- `equal_revision_and_generation_imply_equal_complete_dto`;
- `candidate_generation_overflow_preserves_the_entire_old_snapshot`;
- `one_reconcile_swaps_once_without_observable_intermediate_summary`.

Safe-fs tests cover exact one-file and two-file sets, malformed/oversized receipt, wrong case, extra child, hardlink/symlink/FIFO/device, Windows junction/reparse/ADS, and a controlled post-read shape/identity replacement.

- [ ] **Step 2: Run each RED boundary and confirm a behavior failure**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml scanner_returns_external_and_receipted_candidates_with_internal_identity --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml removal_shapes_are_complete_and_mutually_exclusive --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml status_only_toggle_changes_can_remove_with_revision_only --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml catalog_v3_enforces_management_toggle_and_summary_cross_fields --lib
```

Expected: failures identify the absent locator/removal observation/v3 management fields and old schema-2 behavior. Do not proceed on an unrelated fixture or platform failure.

- [ ] **Step 3: Extend the two bounded scanners without crossing trust boundaries**

Change the local safe-reader result to retain only safe opaque evidence:

```rust
pub(super) struct PackageBytes {
    pub(super) package_slot: PackageSlot,
    pub(super) directory_identity: FileIdentity,
    pub(super) manifest_identity: FileIdentity,
    pub(super) manifest_bytes: Vec<u8>,
    pub(super) receipt: Option<(Vec<u8>, FileIdentity)>,
}
```

Enumerate at most three children and accept only `{manifest.json}` or `{manifest.json, ownership-receipt.json}` with exact case. Open relative to held directories; require regular, single-link, no-reparse files; hold handles through a second shape/identity check. Read manifest with a 16,385-byte probe and receipt with a 4,097-byte probe, charging all bytes—including rejected probes—to the local 2 MiB budget. A malformed receipt rejects the entire double-file package and never falls back to legacy external.

Resolve the system scanner from the plugins root so local and `removal-staging` share the verified ancestor but retain separate byte counters. Removal observation enumerates at most 16 direct items and records only bounded known-object shapes; it does not delete or repair. It exposes full, receipt-only, manifest-only, empty-directory, and unknown target observations; reconciliation derives both-absent from the index plus missing source/target. Manifest-only/extra/unmatched data remains conflict evidence. No observation contains a public path.

- [ ] **Step 4: Reconcile exact evidence and publish schema 3 atomically**

Keep ownership evidence beside—not inside—`PluginRecord`:

```rust
struct LocalRuntimeRecord {
    discovered: DiscoveredLocalPlugin,
    management: PluginManagement,
}

fn proves_managed(package: &DiscoveredLocalPlugin, entry: &ManagedOwnershipEntryV1) -> bool {
    let Some(receipt) = package.locator.receipt.as_ref() else { return false };
    entry.lifecycle().is_managed()
        && entry.matches_receipt(&receipt.model, receipt.canonical_sha256)
        && entry.matches_locator(&package.locator)
        && receipt.model.matches_record(&package.record)
}
```

Load ownership independently from plugin state. Failure makes only managed import/removal unavailable: built-ins and readable locals remain visible, and no-receipt legacy packages stay external/toggleable. A tmp/bak recovery first reconciles its entries and securely rewrites equivalent canonical bytes; until committed rewrite, it authorizes no deletion. Never construct a missing entry from a receipt.

During reload/startup reconciliation, the only writes allowed are `removing -> managed` when the exact source exists and target is absent, or entry deletion when both are absent. Never unlink/rmdir a quarantine object. A failed convergence save leaves `removalPending` or cleanup/conflict evidence visible and uses the corresponding mutually exclusive count.

Build the full next `CatalogPublicationCandidate`, including state-derived statuses, local summary, ownership summary, internal locator basis, and all transport items. Compare semantic publication basis before mutation. Locator/receipt/object, management/ownership eligibility, toggle-block, ownership summary, membership, local discovery, or another structural change consumes one checked generation. A status-only toggle consumes only a checked plugin-state revision and may change only status/canToggle/statusReason/canRemove; mutation generation and all structural/ownership fields remain identical. If another DTO change would otherwise reuse the same `(revision,generation)`, advance generation before publication. At `u64::MAX`, retain the previous candidate byte-for-byte. Swap all internal state once, then derive the snapshot; scan usage is never part of equality or IPC.

Upgrade item and summary validation to every invariant in spec §8.3. `removalPending` is disabled, not removable, non-toggleable, and has the only non-null toggle block reason. Ownership problems do not create `statusReasonCode`; only plugin-state availability creates blocked items.

- [ ] **Step 5: Run GREEN across discovery, ownership, registry, runtime, and startup isolation**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::discovery --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::ownership::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::manifest::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::registry::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::runtime::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml state::tests --lib
```

Expected: exact evidence alone grants managed/removal capability; removal shapes and counts are exhaustive; the two scan budgets cannot poison one another; a single reconciliation has one visible publication; ownership startup failure remains non-fatal.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/plugin/discovery/safe_fs.rs src-tauri/src/plugin/discovery/safe_fs/tests.rs src-tauri/src/plugin/discovery.rs src-tauri/src/plugin/discovery/tests.rs src-tauri/src/plugin/ownership.rs src-tauri/src/plugin/ownership/tests.rs src-tauri/src/plugin/manifest.rs src-tauri/src/plugin/registry.rs src-tauri/src/plugin/registry/tests.rs src-tauri/src/plugin/runtime.rs src-tauri/src/plugin/runtime/tests.rs src-tauri/src/state.rs
git commit -m "feat(plugin): reconcile managed ownership in catalog v3"
```

### Task 4: Refactor package filesystem primitives and stage exact two-file imports

**Files:**
- Modify: `src-tauri/src/storage/mod.rs`
- Create: `src-tauri/src/storage/local_plugin_package.rs`
- Create: `src-tauri/src/storage/local_plugin_package/platform.rs`
- Create: `src-tauri/src/storage/local_plugin_package/tests.rs`
- Modify: `src-tauri/src/storage/local_plugin_import.rs`
- Delete: `src-tauri/src/storage/local_plugin_import/platform.rs`
- Modify: `src-tauri/src/storage/local_plugin_import/tests.rs`

**Interfaces:**
- Consumes: canonical manifest bytes, `PluginRecord`, receipt/index domain types and current exclusive import behavior.
- Produces shared `SystemLocalPluginPackageStorage::{new(), with_plugins_root(PathBuf)}` and fixed held handles for plugins/local/import-staging/removal-staging, with one-volume verification.
- Produces updated `LocalManifestImportStorage::prepare_stage(&self, record: &PluginRecord, manifest_bytes: &[u8]) -> Result<Box<dyn OwnedImportStage>, ImportCommitFailure>`.
- Produces `OwnedImportStage::promote(&mut self) -> Result<PromotedManagedPackage, ImportCommitFailure>` and `cleanup(&mut self)`; `PromotedManagedPackage { entry: ManagedOwnershipEntryV1, locator: LocalPackageLocator }` contains no path.
- Produces shared handle-relative open/identity/exclusive-rename/exact-nonrecursive-cleanup primitives used by Task 6. Windows import promotion is migrated from `MoveFileExW` to source-directory-handle `SetFileInformationByHandle(FileRenameInfo)`; verified child cleanup is handle-bound too.

- [ ] **Step 1: Write RED exact-two-file and native-rename tests**

Start with a real disk contract:

```rust
#[test]
fn promoted_import_contains_exact_receipt_and_manifest_bound_to_entry() {
    let fixture = Fixture::new();
    let storage = fixture.storage();
    let record = local_record("com.example.notes");
    let bytes = record.canonical_manifest_bytes().unwrap();
    let stage = storage.prepare_stage(&record, &bytes).unwrap();
    assert!(fixture.local_entries().is_empty());
    let promoted = stage.promote().unwrap();
    fixture.assert_exact_two_file_package(&promoted);
    assert!(promoted.entry.matches_locator(&promoted.locator));
}
```

Add `stage_writes_and_syncs_manifest_then_receipt_then_directories`, `receipt_and_target_slot_are_host_generated`, `target_collision_rebuilds_entire_stage_and_receipt`, `four_complete_attempts_stop_without_reusing_or_mutating_receipt`, `stage_name_collision_is_bounded_by_the_same_four_attempts`, `partial_stage_cleanup_requires_every_surviving_recorded_identity`, `extra_file_or_replaced_object_blocks_cleanup`, `same_volume_is_required`, `exclusive_rename_never_replaces_target`, `postpromotion_reopen_compares_all_three_identities`, and platform-specific nonzero tests for Linux/macOS/Windows native rename. Each attempt must have a fresh stage name, receiptId, packageSlot, and immutable receipt; a target collision can advance only after exact cleanup proves the prior promotion did not happen.

- [ ] **Step 2: Run focused RED**

Run: `cargo test --locked --manifest-path src-tauri/Cargo.toml storage::local_plugin_package::tests --lib`

Expected: FAIL because the shared adapter and two-file promotion result do not exist.

- [ ] **Step 3: Extract the shared fixed-root adapter and implement two-file stage ownership**

Move—not copy—the existing platform handle, bounded enumeration, sync, and exclusive rename logic under `local_plugin_package/platform.rs`; use Task 1's single `FileIdentity` domain type. Open all three child roots from the same verified plugins handle and fail on a volume mismatch. The stage kernel becomes:

```text
generate receiptId + packageSlot
create exclusive stage directory
create/write/sync manifest.json
create/write/sync ownership-receipt.json
verify exact two-file shape + three identities
sync stage and import-staging parent on Unix
```

The prepared object retains canonical manifest/receipt bytes and exact identities. A stage-name or package-slot collision consumes the current bounded generation cycle. A package-slot collision may continue only after the prior rename is known not to have happened and exact non-recursive cleanup of the owned stage succeeds; generate a new receiptId, stage name, and packageSlot and rebuild both files. Stop after four complete cycles. Unknown cleanup state ends the operation and preserves evidence.

On Windows, open the source directory with DELETE access and rename that held handle relative to the held local parent:

```rust
FILE_RENAME_INFO {
    ReplaceIfExists: false.into(),
    RootDirectory: local_parent_handle,
    FileNameLength: target_utf16_bytes,
    FileName: validated_single_component,
}
```

Use `SetFileInformationByHandle(FileRenameInfo)`; remove the `MoveFileExW` path and do not add a fallback. Delete Windows stage children through disposition on their verified handles rather than closing and reopening by absolute name. Linux/macOS keep `renameat2(RENAME_NOREPLACE)` / `renameatx_np(RENAME_EXCL)` and `unlinkat` under the documented single-writer/no-manual-edit boundary; pre/post checks detect injected swaps best effort but do not claim complete same-user race prevention. Reopen the promoted target from the held local parent, reparse both canonical files, and compare directory/manifest/receipt identities before constructing `PromotedManagedPackage`.

- [ ] **Step 4: Run GREEN and preserved import-storage regression**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::local_plugin_package::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::local_plugin_import::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::discovery::safe_fs::tests --lib
```

Expected: all current-platform real filesystem tests select at least one test and pass; staging remains invisible to discovery and no target is replaced.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/storage/mod.rs src-tauri/src/storage/local_plugin_package.rs src-tauri/src/storage/local_plugin_package/platform.rs src-tauri/src/storage/local_plugin_package/tests.rs src-tauri/src/storage/local_plugin_import.rs src-tauri/src/storage/local_plugin_import/tests.rs
git rm src-tauri/src/storage/local_plugin_import/platform.rs
git commit -m "refactor(plugin): share verified package filesystem operations"
```

### Task 5: Register new imports only after verified promotion and expose import schema 2

**Files:**
- Modify: `src-tauri/src/plugin/import.rs`
- Modify: `src-tauri/src/plugin/import/tests.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Modify: `src-tauri/src/plugin/registry/tests.rs`
- Modify: `src-tauri/src/plugin/runtime/import.rs`
- Modify: `src-tauri/src/plugin/runtime/import/tests.rs`
- Modify: `src-tauri/src/plugin/runtime.rs`

**Interfaces:**
- Consumes: Task 3 ownership-aware registry/catalog and Task 4 `PromotedManagedPackage`.
- Produces: `PluginRegistry::validate_managed_import(record, usage, expected_generation) -> Result<(), ImportCommitFailure>` including canonical generation equality, one final-publication generation increment, ownership availability, 160-entry/index-byte/revision headroom, state revision/capacity, and actual manifest+receipt local-scan budget before any stage storage access.
- Produces: `OwnershipMutationFailure { failure: OwnershipFailure, persist_outcome: PersistOutcome }` and `PluginRegistry::register_managed_import(entry: ManagedOwnershipEntryV1) -> Result<PersistOutcome, OwnershipMutationFailure>` with clone -> persist -> committed-outcome memory adoption; it never fabricates catalog membership.
- Produces: `ImportPostCommitReason::{OwnershipNotRegistered, PublicationUnconfirmed}` mapped only to their fixed wire literals.
- Produces: import commit schema 2 with `Imported`, `ImportedExternal`, `ImportedNotVisible`, and `NotImported` branches.
- Extends pre-promotion failures with ownership unavailable/capacity/revision; maps a post-promotion failure to `ImportedExternal { reason: OwnershipNotRegistered }` only when the final snapshot confirms the exact package as external, otherwise to `ImportedNotVisible`. Neither branch deletes or auto-registers the promoted package.

- [ ] **Step 1: Write RED import ordering and result tests**

Extend the real-root event fixture. The first kernel is:

```rust
#[tokio::test]
async fn import_registers_ownership_only_after_verified_promotion() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    fixture.events.lock().unwrap().clear();
    let result = fixture.runtime.commit_import(&preview.token, &preview.catalog_generation)
        .await.unwrap();
    assert_eq!(fixture.events(), ["scan", "stage-two-files", "disable", "promote", "verify-target", "save-index", "scan", "publish-once"]);
    let wire = serde_json::to_value(result).unwrap();
    assert_eq!(wire["schemaVersion"], 2);
    assert_eq!(wire["status"], "imported");
    assert_eq!(wire["snapshot"]["plugins"][0]["management"], "managed");
    assert_eq!(wire["snapshot"]["plugins"][0]["status"], "disabled");
}
```

Add `ownership_unavailable_rejects_before_stage`, `ownership_160_capacity_and_max_revision_reject_before_stage`, `generation_max_rejects_before_stage`, `index_save_failure_after_promotion_returns_imported_external_only_when_snapshot_confirms_external`, `postcommit_index_or_scan_uncertainty_returns_imported_not_visible`, `target_identity_change_never_registers_index`, `promoted_external_is_never_deleted_or_auto_registered`, `one_complete_candidate_is_published_after_registration`, `caller_loss_still_registers_once`, `commit_result_schema_two_has_exact_branches`, and `successful_restart_reconciles_managed_from_disk`.

- [ ] **Step 2: Run focused RED**

Run: `cargo test --locked --manifest-path src-tauri/Cargo.toml import_registers_ownership_only_after_verified_promotion --lib`

Expected: FAIL because current staging writes one file, current runtime has no index save, and result schema is 1.

- [ ] **Step 3: Implement preflight, promotion, registration, and exact classification**

Keep the existing token/FIFO/state ordering, replacing only the package lifecycle tail:

```text
operation.enter
pre-scan + reconcile + canonical generation check
validate final generation + state + ownership/index + local scan headroom
prepare exact two-file stage
persist exact disabled decision
promote and verify target
register managed index entry atomically
post-scan + reconcile into one complete unpublished candidate
publish candidate exactly once
classify exact managed/external/not-visible result
```

Never hold a registry guard across blocking filesystem work. If target verification fails after promotion, or ownership persistence is `NotCommitted`, retain the promoted package and old index; return `ImportedExternal` only if the one final authoritative snapshot confirms the expected item as external/non-removable. If a committed index outcome, worker failure, scan failure, or publication failure prevents that proof, return `ImportedNotVisible` with the last complete snapshot. `Imported` requires exact manifest fields, source localDeclarative, management managed, status disabled, `canRemove=true`, and the registered locator basis. No branch publishes an intermediate external/managed snapshot.

Serialize exact result branches:

```rust
pub(crate) enum CommitImportResult {
    Imported { plugin_id: String, snapshot: PluginCatalogSnapshot },
    ImportedExternal {
        plugin_id: String,
        reason: ImportPostCommitReason::OwnershipNotRegistered,
        snapshot: PluginCatalogSnapshot,
    },
    ImportedNotVisible {
        plugin_id: String,
        reason: ImportPostCommitReason::PublicationUnconfirmed,
        snapshot: PluginCatalogSnapshot,
    },
    NotImported { disabled_decision_saved: bool, reason: ImportCommitFailure, snapshot: PluginCatalogSnapshot },
}
```

Only pre-promotion ownership unavailable/capacity/revision errors join `ImportCommitFailure`. `plugin_ownership_persist_failed` after promotion is never serialized inside `notImported`; an externally confirmed package uses `plugin_import_ownership_not_registered`, while an unconfirmed publication uses `plugin_import_publication_unconfirmed`. No reload reconstructs the absent entry from its receipt.

- [ ] **Step 4: Run GREEN and backend import regressions**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::runtime::import::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::import::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::registry::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::local_plugin_import --lib
```

Expected: registration ordering and schema 2 pass; all one-time token, fail-closed state, caller cancellation, and old import security cases remain green.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/plugin/import.rs src-tauri/src/plugin/import/tests.rs src-tauri/src/plugin/registry.rs src-tauri/src/plugin/registry/tests.rs src-tauri/src/plugin/runtime.rs src-tauri/src/plugin/runtime/import.rs src-tauri/src/plugin/runtime/import/tests.rs
git commit -m "feat(plugin): register ownership for new manifest imports"
```

### Task 6: Define the closed removal result and add one-slot quarantine/cleanup storage

**Files:**
- Modify: `src-tauri/src/plugin/mod.rs`
- Create: `src-tauri/src/plugin/removal.rs`
- Create: `src-tauri/src/plugin/removal/tests.rs`
- Modify: `src-tauri/src/storage/local_plugin_package.rs`
- Modify: `src-tauri/src/storage/local_plugin_package/platform.rs`
- Modify: `src-tauri/src/storage/local_plugin_package/tests.rs`

**Interfaces:**
- Produces the exact `RemoveManagedLocalPluginFailure` literals from spec §10.5 and `RemoveManagedLocalPluginResult::{Removed, RemovedCleanupPending, RemovedCatalogUnconfirmed, NotRemoved}` with schema 1.
- Separates `BeforeDisabledFailure` from `AfterDisabledFailure` so `disabledDecisionSaved` is derived, not caller-supplied. `BeforeDisabledFailure` covers catalog stale/invalid/generation exhausted, state unavailable/persist/capacity/revision, ownership unavailable/capacity/revision/conflict, discovery unavailable, not managed, requires disabled, storage unavailable, staging capacity, and identity changed. `AfterDisabledFailure` contains only ownership persist failed, remove write failed, and identity changed. `NotRemoved` has no `plugin_id` field.
- The public failure union is exactly `plugin_catalog_stale | plugin_catalog_invalid | plugin_catalog_generation_exhausted | plugin_state_unavailable | plugin_state_persist_failed | plugin_state_capacity_exceeded | plugin_revision_exhausted | plugin_ownership_unavailable | plugin_ownership_persist_failed | plugin_ownership_capacity_exceeded | plugin_ownership_revision_exhausted | plugin_ownership_conflict | plugin_remove_discovery_unavailable | plugin_remove_not_managed | plugin_remove_requires_disabled | plugin_remove_storage_unavailable | plugin_remove_identity_changed | plugin_remove_staging_capacity_exceeded | plugin_remove_write_failed`. `plugin_remove_publication_unconfirmed` is not a member and exists only on `RemovedCatalogUnconfirmed`.
- Produces: `ManagedLocalPluginRemovalStorage::prepare(&LocalPackageLocator, &ManagedOwnershipEntryV1) -> Result<Box<dyn OwnedRemoval>, RemovalStorageFailure>`.
- Produces: `RemovalStorageFailure::{Unavailable, IdentityChanged, StagingCapacityExceeded, ProvenWriteFailure}`; only `quarantine_once` can additionally yield commit uncertainty.
- Produces: `OwnedRemoval::{removal_slot(), reverify_before_rename(), quarantine_once(), verify_quarantine(), cleanup_once()}`. The object owns the one generated slot and all fixed ancestors/source identity needed for the request; there is no API to replace its slot or call rename twice.
- Produces: `QuarantineRenameOutcome::{Committed(VerifiedQuarantine), ProvenNotCommitted(RemovalStorageFailure), CommitUnconfirmed}` and `CleanupOutcome::{Removed, Pending(KnownCleanupShape), Conflict}`.
- Produces: `KnownCleanupShape::{Full, ReceiptOnly, EmptyDirectory, BothAbsent}`. Manifest-only is deliberately absent and therefore maps to conflict.

- [ ] **Step 1: Write RED result-union and native storage tests**

In `plugin/removal/tests.rs`, serialize every branch and every failure literal. Start with:

```rust
#[test]
fn not_removed_has_no_plugin_id_and_derives_disabled_decision_phase() {
    let before = RemoveManagedLocalPluginResult::not_removed(
        BeforeDisabledFailure::RequiresDisabled,
        snapshot(),
    );
    let after = RemoveManagedLocalPluginResult::not_removed_after_disabled(
        AfterDisabledFailure::RemoveWriteFailed,
        snapshot(),
    );
    assert_eq!(json(before)["disabledDecisionSaved"], false);
    assert!(json(before).get("pluginId").is_none());
    assert_eq!(json(after)["disabledDecisionSaved"], true);
}
```

Add `remove_result_has_exact_schema_one_branch_keys`, `removed_branches_carry_the_requested_plugin_id`, `publication_unconfirmed_code_exists_only_on_unconfirmed`, `false_only_failure_codes_cannot_construct_true`, `true_only_failure_codes_cannot_construct_false`, and `identity_changed_is_constructible_before_or_after_disabled`. Assert `AppResult::Err` is not used to encode any expected business failure.

In storage tests add:

- `prepare_requires_exact_receipt_manifest_and_three_entry_identities`;
- `prepare_counts_all_17_removal_entries_and_rejects_capacity`;
- `prepare_allocates_one_absent_remove_slot`;
- `target_collision_returns_proven_not_committed_without_second_uuid`;
- `unknown_rename_result_is_commit_unconfirmed`;
- `postrename_reopen_reparses_both_files_and_three_identities`;
- `cleanup_unlinks_manifest_then_receipt_then_empty_directory`;
- `cleanup_interruption_reports_full_receipt_only_empty_and_both_absent`;
- `manifest_only_extra_or_replaced_child_is_conflict`;
- `cleanup_is_never_recursive_and_reload_has_no_cleanup_entrypoint`.

On Windows add `source_name_swap_does_not_redirect_handle_bound_rename` and `child_name_swap_does_not_redirect_handle_bound_disposition`; assert the held source directory is renamed with `SetFileInformationByHandle`, and each verified manifest/receipt handle is deleted with disposition before the directory handle is removed. On Linux/macOS add controlled hooks immediately before/after `renameat2`/`renameatx_np` and `unlinkat`; assert a detected mismatch preserves quarantine/entry and prevents further cleanup. Name these tests “best effort” and do not assert that every concurrent same-user exchange is detectable.

- [ ] **Step 2: Run focused RED and check that missing APIs—not platform setup—fail**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::removal::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml prepare_allocates_one_absent_remove_slot --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml cleanup_unlinks_manifest_then_receipt_then_empty_directory --lib
```

Expected: compile failures name the absent result/storage interfaces. If a native fixture cannot create its root or probe its supported primitive, fix the fixture before production code.

- [ ] **Step 3: Implement the minimal one-shot quarantine object**

`prepare` opens plugins/local/removal-staging from held verified ancestors, checks same volume, counts at most 17 direct removal entries, generates exactly one canonical `remove-*` name, proves it absent, and reopens the source through its opaque locator. It validates exact two-file shape, canonical receipt/manifest, fingerprint, and directory/manifest/receipt identities before returning the owned operation. It never trusts a caller path, slot, receipt, or identity.

`reverify_before_rename` repeats source evidence and target absence immediately before the commit point. `quarantine_once` consumes an internal state transition so it cannot run twice. Linux uses `renameat2(RENAME_NOREPLACE)`, macOS `renameatx_np(RENAME_EXCL)`, and Windows renames the still-held source directory handle relative to the still-held removal parent with `ReplaceIfExists=false`. No platform falls back to absolute-path rename, overwrite, or copy.

Use a private state machine with no reset transition:

```rust
enum OwnedRemovalState {
    Prepared,
    RenameAttempted,
    Committed(VerifiedQuarantine),
    CleanupAttempted,
}

fn quarantine_once(&mut self) -> QuarantineRenameOutcome {
    if !matches!(self.state, OwnedRemovalState::Prepared) {
        return QuarantineRenameOutcome::CommitUnconfirmed;
    }
    self.state = OwnedRemovalState::RenameAttempted;
    self.rename_held_source_to_registered_target()
}
```

An `AlreadyExists` result is `ProvenNotCommitted` only after the original source still matches all three identities and the target is a different existing object. Any other ambiguous return is `CommitUnconfirmed`. After a committed rename, sync both parents where supported, reopen the target relative to removal-staging, parse both files again, and compare all identities.

`cleanup_once` runs at most once for the accepted owned request. Reverify the current monotonic known subset, remove manifest first, receipt last, prove the directory empty, then remove the directory and sync removal-staging. Windows mutation stays bound to verified child/source handles. Unix name-based unlink/rmdir is supported only under the single-writer/no-manual-edit boundary; adjacent identity checks are best effort. Unknown children, manifest-only, identity drift, or detected replacement stops immediately and preserves evidence. Never call `remove_dir_all`.

Encode result invariants in constructors and private enums so an invalid boolean/code or pluginId shape cannot be serialized by ordinary production code. Sanitize storage errors to the closed public codes; retain OS error kind only in non-sensitive tracing fields.

- [ ] **Step 4: Run GREEN and the shared import-platform regression**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::removal::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::local_plugin_package::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::local_plugin_import::tests --lib
$forbidden = rg -n "remove_dir_all|MoveFileExW|ReplaceIfExists:\s*true" src-tauri/src/plugin/removal.rs src-tauri/src/storage/local_plugin_package.rs src-tauri/src/storage/local_plugin_package
if ($LASTEXITCODE -eq 0) { $forbidden; throw 'forbidden lifecycle filesystem primitive' }
if ($LASTEXITCODE -ne 1) { exit $LASTEXITCODE }
```

Expected: domain and current-platform filesystem tests pass. The static assertion finds no lifecycle `remove_dir_all`, `MoveFileExW`, or replace-existing package rename.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/plugin/mod.rs src-tauri/src/plugin/removal.rs src-tauri/src/plugin/removal/tests.rs src-tauri/src/storage/local_plugin_package.rs src-tauri/src/storage/local_plugin_package/platform.rs src-tauri/src/storage/local_plugin_package/tests.rs
git commit -m "feat(plugin): quarantine managed packages safely"
```

### Task 7: Orchestrate disabled -> removing -> quarantine -> cleanup with one final publication

**Files:**
- Modify: `src-tauri/src/plugin/runtime.rs`
- Create: `src-tauri/src/plugin/runtime/removal.rs`
- Create: `src-tauri/src/plugin/runtime/removal/tests.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Modify: `src-tauri/src/plugin/registry/tests.rs`
- Modify: `src-tauri/src/plugin/ownership.rs`
- Modify: `src-tauri/src/storage/plugin_state.rs`
- Modify: `src-tauri/src/storage/managed_plugin_ownership.rs`

**Interfaces:**
- Produces: `PluginRuntime::remove_managed_local_plugin(id, expected_catalog_generation) -> AppResult<RemoveManagedLocalPluginResult>` using the existing synchronous operation reservation and owned spawned task pattern.
- Produces: `PluginRuntime::with_lifecycle_services(registry, discovery, reader, import_storage, removal_storage)` for tests; production constructs one `Arc<SystemLocalPluginPackageStorage>` and supplies clones behind the import and removal trait objects.
- Produces: `RemovalPreflight { record, locator, entry }`, obtainable only from the current complete registry snapshot for one exact managed, localDeclarative, disabled, state/ownership-available item.
- Produces registry transactions `preflight_removal`, `persist_disabled_before_removal`, `persist_removing`, `rollback_removing`, `delete_removed_entry`, `build_reconciled_candidate`, and `publish_candidate_once`.
- Extends the injected state persistence contract so tests can return every `PersistOutcome` from `save_before_destructive_rename`; ownership transitions already return explicit outcomes from Task 2.
- Preserves the complete last published snapshot separately from the unpublished post-rename candidate, allowing committed-unconfirmed responses without partial registry mutation.

- [ ] **Step 1: Write RED ordering, gate, crash-boundary, and result-classification tests**

Build an event-recording runtime fixture with independent discovery, state persistence, ownership persistence, and package-storage spies. The core happy-path test is:

```rust
#[tokio::test]
async fn remove_has_one_commit_point_one_cleanup_attempt_and_one_publication() {
    let fixture = RemovalFixture::managed_disabled().await;
    let result = fixture.remove().await;
    assert!(matches!(result, RemoveManagedLocalPluginResult::Removed { .. }));
    assert_eq!(fixture.events(), [
        "gate", "preflight", "open-source", "reserve-one-slot",
        "save-disabled-barrier", "save-removing-barrier", "final-reverify",
        "rename-once", "verify-quarantine", "scan-candidate", "cleanup-once",
        "delete-entry", "final-reconcile", "publish-once",
    ]);
}
```

Add pre-storage tests for noncanonical/stale generation, generation `MAX`, state revision/capacity, ownership capacity, ownership revision `MAX-1/MAX`, discovery unavailable, state/ownership unavailable, built-in, external, ownership conflict, removal pending, missing/duplicate, and enabled. Each asserts zero calls to removal storage, state save, and index save.

Add the required status race test `same_generation_higher_revision_reenable_is_rejected_before_removal_storage`: open/capture while disabled, run a successful toggle to enabled that keeps generation unchanged, then submit the captured generation; assert `plugin_remove_requires_disabled`, `disabledDecisionSaved=false`, and no package/root/staging access.

Add `state_barrier_failure_never_saves_removing_or_renames`, `ownership_barrier_failure_never_renames_and_adopts_committed_document`, `initial_identity_change_is_false_phase`, `final_identity_change_is_true_phase_and_rolls_back`, `one_slot_collision_rolls_back_and_ends_without_retry`, `rollback_failure_publishes_removal_pending_once`, `ambiguous_rename_never_returns_not_removed`, `postrename_scan_failure_still_attempts_cleanup_once_then_returns_unconfirmed`, `cleanup_failure_keeps_removing_and_returns_cleanup_pending`, `cleanup_success_then_index_failure_returns_cleanup_pending`, `fully_clean_result_returns_removed`, `same_id_external_arriving_after_commit_is_not_the_removed_identity`, `caller_cancellation_does_not_cancel_owned_remove`, and `remove_import_toggle_reload_are_fifo`.

Finally add `startup_and_reload_only_observe_quarantine`, covering full, receipt-only, empty directory, both absent, manifest-only, and orphan/mismatch shapes. Assert reload may write only the safe rollback/stale-entry index transitions, never invoke rename, child unlink, or rmdir.

- [ ] **Step 2: Run focused RED and confirm orchestration is absent**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml same_generation_higher_revision_reenable_is_rejected_before_removal_storage --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml remove_has_one_commit_point_one_cleanup_attempt_and_one_publication --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml ambiguous_rename_never_returns_not_removed --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml startup_and_reload_only_observe_quarantine --lib
```

Expected: tests fail because the runtime removal entry and registry transactions do not exist. A RED caused only by an incorrect existing fixture is not acceptable.

- [ ] **Step 3: Implement exact gate/preflight and the two document barriers**

Reserve the FIFO operation synchronously, spawn the owned task without a suspension gap, then acquire the gate. Parse ID and canonical generation. From the current fully reconciled registry state, preflight—in this order before state/index/package mutation storage—one final generation increment, one state revision rewrite/capacity slot, and two ownership revisions (`managed -> removing`, then exactly one of rollback or delete). Capacity errors precede revision exhaustion. Validate unique localDeclarative, disabled, managed, state/ownership available, and capture its internal record/locator/entry.

Keep the storage boundary visible in the coordinator:

```rust
let preflight = runtime.registry.blocking_write()
    .preflight_removal(&id, &expected_generation)?;
// No package-storage method is called above this line.
let mut owned = runtime.removal_storage
    .prepare(&preflight.locator, &preflight.entry)?;
runtime.registry.blocking_write()
    .persist_disabled_before_removal(&preflight.record)?;
runtime.registry.blocking_write()
    .persist_removing(preflight.entry.receipt_id(), owned.removal_slot())?;
```

Call package `prepare` only after those checks. Persist the exact current disabled identity through `save_before_destructive_rename`, removing historical enabled fingerprints even when the visible item is already disabled. Adopt a committed document result, but continue only when the platform barrier is satisfied. Before that barrier, every rejection has `disabledDecisionSaved=false`; after it, subsequent proven-precommit failures use true.

Persist the same entry as `removing` with the one `removal_slot`, again requiring the platform barrier and adopting committed outcomes. On barrier failure or any later proven pre-rename error, attempt exactly one atomic `removing -> managed` rollback. If rollback fails, reconcile/publish removal-pending/degraded once; never allocate another slot.

- [ ] **Step 4: Implement the one-shot commit, cleanup, and result matrix**

Immediately before rename, call `reverify_before_rename`. Initial open failure maps identity changed/false; this final failure maps identity changed/true. A proven target collision or proven-not-committed write failure rolls back, returns `notRemoved`, and ends. `Committed` and `CommitUnconfirmed` permanently cross the result boundary: no later path may construct `NotRemoved` or call quarantine again.

After committed/uncertain rename, verify/sync and obtain one authoritative scan into an unpublished candidate. Even if scanning fails, the same accepted owned task may attempt at most one exact cleanup, but its response is `RemovedCatalogUnconfirmed`. Cleanup success attempts one index deletion; cleanup failure retains `removing`. Reconcile the actual final disk/index state, compare against the previously published complete candidate, consume the single preflighted generation if required, and swap once. Do not expose the temporary removal-pending state between index save and rename.

Classify only after final reconciliation:

```text
removed                 target owned identity absent; quarantine and index clean
removedCleanupPending   catalog confirms commit; cleanup/index remains and cleanupPendingCount > 0
removedCatalogUnconfirmed rename committed or cannot be excluded; final publication unconfirmed
notRemoved              backend proved rename did not happen
```

All ID-bearing branches use the normalized request ID; `NotRemoved` contains no pluginId. A same-ID external package may remain in `Removed.snapshot`, but the removed managed/removalPending identity may not. Unknown worker/join/invariant or transport failures remain `AppResult::Err`; clients treat them as unknown and never replay.

- [ ] **Step 5: Run GREEN plus import/toggle/reload concurrency regressions**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::runtime::removal::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::runtime::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::runtime::import::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::registry::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::plugin_state::tests --lib
```

Expected: all preflight failures occur before removal storage; same-generation re-enable is rejected; every commit-point failure is classified conservatively; one request uses one slot/rename/cleanup/publication; reload never performs destructive cleanup.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/plugin/runtime.rs src-tauri/src/plugin/runtime/removal.rs src-tauri/src/plugin/runtime/removal/tests.rs src-tauri/src/plugin/registry.rs src-tauri/src/plugin/registry/tests.rs src-tauri/src/plugin/ownership.rs src-tauri/src/storage/plugin_state.rs src-tauri/src/storage/managed_plugin_ownership.rs
git commit -m "feat(plugin): remove managed local packages transactionally"
```

### Task 8: Expose one fixed removal command and preserve the local-main ACL

**Files:**
- Modify: `src-tauri/src/commands/plugin.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/capabilities/plugin-runtime.json`
- Create (generated by `tauri-build`): `src-tauri/permissions/autogenerated/remove_managed_local_plugin.toml`

**Interfaces:**
- Produces helper `remove_managed_local_plugin_from(runtime, id, expected_catalog_generation)` and Tauri command `remove_managed_local_plugin(state, id: String, expected_catalog_generation: String)`.
- The only frontend-controlled fields are canonical plugin ID and catalog generation; helper forwards both unchanged to Task 7.
- Registers exactly one new command and one unscoped autogenerated allow/deny permission; capability remains `webviews: ["main"]` with no windows/remote/scope field.

- [ ] **Step 1: Write RED command-forwarding and ACL tests**

In the existing command test module add `remove_helper_forwards_only_id_and_expected_generation`, `remove_command_serializes_every_result_branch`, and `remove_command_keeps_business_rejections_in_ok`. Use a spy runtime/storage to prove no path, slot, receipt, fingerprint, publisher, object identity, or raw manifest can be accepted.

In `lib.rs` tests, extend the local/remote/other-window authority loop and change the exact capability assertion from six to seven commands. Add `remove_permission_has_no_scope_and_union_has_no_fs_shell_dialog_or_remote_grant`. First assert the generated permission file and capability entry are absent.

- [ ] **Step 2: Run focused RED**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml remove_helper_forwards_only_id_and_expected_generation --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin_commands_are_available_only_to_the_local_main_webview --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin_capability_grants_exactly_the_seven_fixed_commands_without_scope --lib
```

Expected: the helper test fails to compile and the ACL expectations fail because the seventh command/permission is not registered.

- [ ] **Step 3: Add the minimal command surface and regenerate permission metadata**

Implement only:

```rust
#[tauri::command]
pub async fn remove_managed_local_plugin(
    state: State<'_, AppState>,
    id: String,
    expected_catalog_generation: String,
) -> AppResult<RemoveManagedLocalPluginResult> {
    remove_managed_local_plugin_from(
        &state.plugins,
        &id,
        &expected_catalog_generation,
    ).await
}
```

Add the command to `generate_handler!`, `AppManifest::commands`, and the ordered `plugin-runtime` permissions array. Run `cargo test --locked --manifest-path src-tauri/Cargo.toml remove_helper_forwards_only_id_and_expected_generation --lib` once so `tauri-build` creates the exact autogenerated allow/deny TOML, then inspect and stage that one file. Do not hand-edit generated permission metadata and do not grant filesystem/dialog/shell/opener or a wildcard command.

- [ ] **Step 4: Run GREEN and the complete capability union tests**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml commands::plugin::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml capability_tests --lib
git diff --check
```

Expected: every result remains a successful structured response, local `main` resolves all seven permissions, remote/other webviews resolve none, and the union exposes no frontend file/dialog/shell access.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/commands/plugin.rs src-tauri/src/lib.rs src-tauri/build.rs src-tauri/capabilities/plugin-runtime.json src-tauri/permissions/autogenerated/remove_managed_local_plugin.toml
git commit -m "feat(plugin): expose managed package removal command"
```

### Task 9: Upgrade strict frontend protocols for catalog v3, import v2, and removal v1

**Files:**
- Modify: `src/types/plugin.ts`
- Modify: `src/services/pluginService.ts`
- Modify: `tests/frontend/pluginService.test.ts`
- Modify (fixture migration only): `tests/frontend/pluginStore.test.ts`
- Modify (fixture migration only): `tests/frontend/pluginCard.test.ts`
- Modify (fixture migration only): `tests/frontend/pluginMarketplacePage.test.ts`

**Interfaces:**
- Changes the existing unversioned public names `PluginCatalogItem`, `PluginCatalogSnapshot`, and `PluginCatalogMutationResult` in place to schema 3.
- Produces `PluginManagement`, `PluginToggleBlockReason`, and `ManagedOwnershipSummary`, plus item `management/canRemove/toggleBlockReasonCode` and snapshot `managedOwnership`.
- Changes `CommitLocalManifestImportResult` to schema 2 with exact `imported`, `importedExternal`, `importedNotVisible`, and `notImported` branches.
- Produces exact `RemoveManagedLocalPluginFailure` and `RemoveManagedLocalPluginResult` TypeScript unions matching spec §10.5; `notRemoved` has no `pluginId`.
- Produces `removeManagedLocalPlugin(id, expectedCatalogGeneration)` invoking `remove_managed_local_plugin` with exactly `{ id, expectedCatalogGeneration }`.
- Extends `PluginErrorCode` and fixed local Chinese mappings for ownership/removal codes; backend `message` remains untrusted and never displayed.

- [ ] **Step 1: Write RED exact-parser and command-payload tests**

First mechanically upgrade only the fixture builders in store/card/page test files to contain valid schema-3 fields and a valid zero-count available `managedOwnership`; do not add store or UI behavior in this task. This prevents required type fields from turning the parser RED into unrelated whole-suite TypeScript failures.

In `pluginService.test.ts`, update `validItem`, `validSnapshot`, `validMutation`, `importSnapshot`, and result helpers to v3/v2. Add table tests for exact keys, schemas, enums, canonical u64 strings, item/source/management/status/reason/toggle/canRemove invariants, summary integer/count/status invariants, sorted unique IDs, and global plugin-state availability. Bound rollback/cleanup counts to index capacity, conflict to index plus removal-staging capacity, and reject a combined count above the bounded population.

Add mutation tests proving request/result/current generation equality is required and the returned manifest/source/management/toggle-block/granted-capability structure is identical; status-only `canRemove` may change with a higher revision at the same generation.

Add import-v2 tests proving `imported` contains the exact expected managed+disabled+removable item, `importedExternal` contains the exact expected external+disabled+non-removable item and only `plugin_import_ownership_not_registered`, `importedNotVisible` uses only publication-unconfirmed, and `notImported` retains its exact pre-promotion boolean/code matrix. Reject schema 1.

Add an exhaustive removal test matrix:

```ts
it('rejects every illegal remove code and disabledDecisionSaved pairing', async () => {
  for (const reasonCode of FALSE_ONLY_REMOVE_CODES) {
    await expectRemoveRejected(notRemoved(reasonCode, true))
  }
  for (const reasonCode of TRUE_ONLY_REMOVE_CODES) {
    await expectRemoveRejected(notRemoved(reasonCode, false))
  }
  await expectRemoveAccepted(notRemoved('plugin_remove_identity_changed', false))
  await expectRemoveAccepted(notRemoved('plugin_remove_identity_changed', true))
})
```

Also reject extra/missing keys; pluginId on `notRemoved`; missing/mismatched pluginId on the three ID-bearing branches; same-ID managed/removalPending in `removed`; zero cleanup count in `removedCleanupPending`; any unconfirmed reason except `plugin_remove_publication_unconfirmed`; and any invalid nested v3 snapshot. Permit a same-ID external item in `removed`. Assert parser failure rejects the entire result and does not expose/adopt its snapshot. Assert the invoke mock sees exactly the command name and two fields.

- [ ] **Step 2: Run the focused RED tests**

Run: `pnpm test -- tests/frontend/pluginService.test.ts`

Expected: failures identify schema 2/1 assumptions and the missing remove adapter/parser. No fixture should fail merely because a required v3 field is absent.

- [ ] **Step 3: Implement strict v3/v2/v1 parsers before returning any value**

Extend exact-key arrays and parse helpers; do not cast an unchecked response. Parse `managedOwnership` with exact keys and require: available => all counts zero; degraded => at least one nonzero; unavailable => all zero. Enforce every item cross-field rule from spec §8.3 in snapshot context, including managed/local-only, managed+disabled+state-available as the sole removable combination, and `removalPending` as the sole toggle-block reason.

For mutation parsing, require schema 3 and a non-blocked item, then in `setPluginEnabled` require returned generation equals the sent generation. Leave store-level comparison to Task 10, but return no structurally invalid mutation.

Parse import and removal by status-discriminated exact branch keys. The removal parser receives the normalized requested ID so every ID-bearing result must match it; `notRemoved` must omit it. Encode the exact false-only/true-only/identity-either matrix as constant sets and reject before returning the nested snapshot. Every snapshot goes through the same complete v3 parser.

Invoke removal exactly as:

```ts
const value = await tauriInvoke<unknown>('remove_managed_local_plugin', {
  id,
  expectedCatalogGeneration,
})
return parseRemoveResult(value, id)
```

Add fixed local error text for every code in spec §14. `pluginErrorCode` accepts only exact `{ code, message }`; `pluginErrorMessage` never returns `message`.

- [ ] **Step 4: Run GREEN, typecheck all migrated fixtures, and lint touched TypeScript**

```powershell
pnpm test -- tests/frontend/pluginService.test.ts
pnpm exec vue-tsc --noEmit
pnpm lint
```

Expected: strict protocol tests pass, all store/card/page fixtures typecheck under v3, and no unchecked response or backend message reaches the application.

- [ ] **Step 5: Commit**

```powershell
git add src/types/plugin.ts src/services/pluginService.ts tests/frontend/pluginService.test.ts tests/frontend/pluginStore.test.ts tests/frontend/pluginCard.test.ts tests/frontend/pluginMarketplacePage.test.ts
git commit -m "feat(plugin): validate ownership and removal protocols"
```

### Task 10: Add an independent removal flight and reuse complete snapshot arbitration

**Files:**
- Modify: `src/stores/plugin.ts`
- Modify: `tests/frontend/pluginStore.test.ts`

**Interfaces:**
- Adds `managedOwnership` to adopted catalog state and expands immutable mutation comparison to manifest/source/management/toggle-block/granted-capability structure.
- Produces `PluginRemovalStatus = 'idle' | 'confirming' | 'removing' | 'result'`.
- Produces state `removalStatus`, `removalTarget`, `removalConfirmationStale`, `removalError`, `removalResult`, and `removalOutcomeUnknown`.
- Produces actions `beginRemoval(id)`, `cancelRemoval()`, `confirmRemoval()`, `releaseRemovalView()`, and `clearRemovalResult()`.
- Adds private, independent `removalFlight`, `removalOwnerSequence`, and `activeRemovalOwner`; do not reuse import ownership, toggle mutation owners, or `pendingIds`.

- [ ] **Step 1: Write RED removal-owner, invalidation, and arbitration tests**

Mock `removeManagedLocalPlugin` beside the existing service mocks. Add `managedOwnership` to store assertions and begin with:

```ts
it('captures only a current managed disabled removable item and generation', async () => {
  const store = await loadedStoreWith(managedDisabledItem(), '7', '11')
  expect(store.beginRemoval('com.example.notes')).toBe(true)
  expect(store.removalStatus).toBe('confirming')
  expect(store.removalTarget?.plugin.manifest.id).toBe('com.example.notes')
  expect(store.removalTarget?.catalogGeneration).toBe('7')
  expect(store.removalTarget?.revision).toBe('11')
})
```

Add `built_in_external_enabled_conflict_unavailable_and_pending_cannot_open_removal`, `double_confirm_coalesces_to_one_service_call`, `new_generation_permanently_invalidates_unsubmitted_confirmation`, `same_generation_higher_revision_target_reenable_invalidates_confirmation`, `same_generation_unrelated_revision_does_not_invalidate_unchanged_target`, `confirm_rechecks_current_target_before_invoke`, and `store_never_optimistically_removes_the_card`.

Add arbitration tests `every_valid_remove_branch_routes_snapshot_through_common_bigint_arbitration`, `late_lower_generation_remove_snapshot_cannot_delete_new_same_id_item`, `same_generation_lower_revision_cannot_overwrite_newer_target_status`, `same_generation_structural_or_ownership_change_is_rejected`, `remove_and_import_have_independent_flight_owners`, `transport_or_parser_failure_becomes_unknown_without_retry`, and `release_view_clears_only_unsubmitted_confirmation_but_keeps_accepted_owned_flight`. For the last test, unmount/release while the removal promise is pending, resolve it later, and assert its snapshot is still arbitrated without any focus action.

- [ ] **Step 2: Run focused RED**

Run: `pnpm test -- tests/frontend/pluginStore.test.ts`

Expected: failures name missing removal state/actions/service wiring; existing load/import/toggle arbitration remains green within the same file.

- [ ] **Step 3: Implement capture, permanent staleness, and independent ownership**

Represent the confirmation as a parsed item plus captured revision/generation:

```ts
interface RemovalTarget {
  plugin: PluginCatalogItem
  revision: string
  catalogGeneration: string
}
```

`beginRemoval` succeeds only for `management === 'managed'`, `status === 'disabled'`, `canRemove === true`, and no active removal. Never derive identity from DOM attributes. `cancelRemoval` works only before submission.

After every accepted full snapshot and every accepted mutation, recompute staleness against the current store item. Mark it permanently stale when generation differs, the item disappears, or the same-generation newer state changes target status/management/canRemove/toggle-block/immutable identity. Do not stale an unchanged target merely because an unrelated item advanced revision. Once stale, confirmation cannot become valid again.

Immediately before service invocation, re-read the current store target and apply the same eligibility/identity check; a mismatch marks stale and sends nothing. Submit the captured ID/generation once through an independent owner. Do not modify `catalog` optimistically.

- [ ] **Step 4: Feed every result through existing BigInt arbitration**

On a parsed result, call the existing authoritative snapshot adoption path with a new request order before storing the result. This applies equally to removed, cleanup-pending, unconfirmed, and not-removed. The common adoption function must include `managedOwnership`; on same generation it preserves per-item higher revisions, while a higher generation atomically replaces the structural/ownership basis.

```ts
const requestOrder = ++snapshotSequence
const adopted = adoptSnapshot(result.snapshot, requestOrder)
if (adopted) markOpenConfirmationsStaleAfterAdoption()
removalResult.value = result
```

At the same generation, reject any full snapshot or mutation that changes membership, manifest, source, management, toggle-block reason, granted capabilities, or ownership summary. Higher revision may change state/status and its status-derived `canRemove` only.

On service rejection—including parser rejection—set only fixed `removalOutcomeUnknown`/`removalError` guidance. Never synthesize a result, delete a card, or automatically call remove/reload. `releaseRemovalView` may discard an unsubmitted confirmation; after submission it leaves the flight/owner intact so a late snapshot can arbitrate, but store code never restores or steals focus. `clearRemovalResult` returns a settled view to idle.

- [ ] **Step 5: Run GREEN plus service/type regressions**

```powershell
pnpm test -- tests/frontend/pluginStore.test.ts
pnpm test -- tests/frontend/pluginService.test.ts
pnpm exec vue-tsc --noEmit
```

Expected: removal calls coalesce, confirmation invalidation observes both generation and status/revision, no optimistic deletion/retry occurs, and stale late snapshots cannot overwrite newer catalog state.

- [ ] **Step 6: Commit**

```powershell
git add src/stores/plugin.ts tests/frontend/pluginStore.test.ts
git commit -m "feat(plugin): coordinate managed package removal state"
```

### Task 11: Add the accessible confirmation dialog, management UX, and operator guidance

**Files:**
- Create: `src/components/plugins/PluginRemovalDialog.vue`
- Create: `tests/frontend/pluginRemovalDialog.test.ts`
- Modify: `src/components/plugins/PluginCard.vue`
- Modify: `src/components/plugins/pluginPresentation.ts`
- Modify: `src/components/plugins/PluginMarketplacePage.vue`
- Modify: `src/components/plugins/PluginMarketplacePage.css`
- Modify: `tests/frontend/pluginCard.test.ts`
- Modify: `tests/frontend/pluginMarketplacePage.test.ts`
- Modify: `docs/plugin-local-manifests.md`

**Interfaces:**
- `PluginRemovalDialog` props: parsed `plugin: PluginCatalogItem`, `submitting: boolean`, `stale: boolean`, optional connected `opener`; emits `confirm` and `cancel` only.
- `PluginCard` adds a removal event carrying plugin ID and the connected trigger; only managed items render a removal action state.
- `pluginPresentation.ts` adds management-based fixed labels and explanations; do not infer management from `source` alone.
- `PluginMarketplacePage` opens removal only in `installed` and `manage`, wires the Task 10 state, shows three ownership counts/warnings, and offers reload/maintenance guidance without automatic command replay.

- [ ] **Step 1: Write RED dialog, card, page, and copy tests**

Create `pluginRemovalDialog.test.ts` using the deterministic `HTMLDialogElement.prototype.showModal/close` shim already used by `pluginImportDialog.test.ts`. Test:

- exact `role="dialog"`, `aria-modal`, linked title/description, and modal open;
- initial focus on Cancel, not the destructive button;
- forward/backward Tab cycles within the dialog's enabled controls;
- Escape before submit prevents native close, emits cancel once, and restores only a still-connected opener;
- double click emits confirm once;
- stale disables/guards confirm and displays an alert;
- submitting sets `aria-busy`, disables both actions, exposes a polite `role=status`, and blocks Escape/cancel;
- hostile `<img ...>` text creates no element; every manifest value is plain interpolation inside `<bdi>` and long IDs remain visible.

In `pluginCard.test.ts`, add exact cases for built-in, external, managed enabled, managed disabled, managed blocked, ownership conflict/unavailable, and removal pending. Only managed renders the removal area; enabled says `请先停用`, exact disabled+canRemove has the active `移除本地包` button, and no other management state emits remove. Prove `toggleBlockReasonCode=removalPending` disables the switch independently of ownership warning text.

In `pluginMarketplacePage.test.ts`, add `removal_entry_exists_in_installed_and_manage_but_never_market`, `page_passes_current_parsed_item_and_connected_opener_to_dialog`, `rollback_cleanup_and_conflict_counts_are_distinct`, `ownership_unavailable_warns_without_hiding_external_browsing`, `removed_success_waits_for_result_snapshot`, `cleanup_pending_unconfirmed_and_unknown_offer_reload_only`, `not_removed_uses_fixed_local_copy_and_disabled_note`, `page_never_replays_remove`, and `unmount_during_submit_does_not_cancel_or_restore_focus_from_late_result`.

Update import page tests for schema-2 `importedExternal`: display that the package remains external/non-removable, advise against re-importing the same ID, and offer no destructive recovery.

- [ ] **Step 2: Run focused RED**

```powershell
pnpm test -- tests/frontend/pluginRemovalDialog.test.ts
pnpm test -- tests/frontend/pluginCard.test.ts tests/frontend/pluginMarketplacePage.test.ts
```

Expected: the first command cannot resolve the new component; the second reports missing management/removal controls and summary/result copy, while unrelated import/load tests remain green.

- [ ] **Step 3: Implement the modal and card semantics**

Use a native `<dialog>` opened with `showModal()`. Capture the opener before opening, focus Cancel after the next tick, and explicitly cycle Tab/Shift+Tab among connected enabled dialog controls so tests and non-native fallback behavior remain deterministic. On pre-submit cancel/Escape, prevent default, emit once, close on unmount, and restore only a connected opener. Once `submitting` or an action is requested, Escape and both controls cannot simulate cancellation. Keep title and irreversible-effect description linked via `aria-labelledby`/`aria-describedby`.

Render name, ID, version, publisher, and source/management using text interpolation and `<bdi>`. Explain: the managed copy in EasiFlux is deleted, the originally selected source is untouched, the operation is irreversible, and the disabled preference remains. Add `overflow-wrap: anywhere` for identifiers; never use `v-html`, auto-linking, or manifest-provided URLs.

Make presentation labels exact:

```text
builtIn: 内置 · 随应用提供
managed: 本地声明式包 · EasiFlux 管理
external: 本地声明式包 · 外部放置，应用不会删除
ownershipConflict: 管理记录不一致，需要退出应用后人工检查
ownershipUnavailable: 所有权状态暂不可用，应用不会删除
removalPending: 移除准备待回退，当前不可启停或移除
```

Only a managed card renders the removal area. Its enabled state is an instruction to disable first; its exact disabled/removable state emits one removal request with the current button as opener. External/built-in/conflict/unavailable/removal-pending cards have no hidden or disabled-but-callable delete control.

- [ ] **Step 4: Wire page results and document the maintenance boundary**

Render the dialog from `store.removalTarget`, never reconstructing fields from the DOM. Open it from installed-card and manage-list triggers only. On unmount call `releaseRemovalView`; that clears only an unsubmitted view. No late result requests focus.

Display ownership summary independently from plugin-state and local-discovery warnings. Show rollback-pending, cleanup-pending, and conflict counts separately with their meanings; ownership unavailable must not hide the catalog or prevent external toggles.

Result surfaces are fixed and sanitized: removed is a polite success; cleanup pending says the package left discovery but maintenance remains; catalog/transport unknown says the result is unconfirmed. Those latter states offer only the existing reload command and maintenance guidance—never another removal call. A `notRemoved` reason uses a total local map; when `disabledDecisionSaved=true`, add that the disabled preference was durably/process-crash-safely confirmed and retained. Do not remove a card until an adopted snapshot does so.

Update `docs/plugin-local-manifests.md` with the exact legacy one-file external shape, new two-file managed shape, three-way proof, disabled-first removal, retained disabled state, import-registration failure as external, quarantine/pending meanings, single-writer/no-manual-edit Unix boundary, Windows handle-bound operations, and “exit before manual maintenance.” Explicitly state reload/startup never deletes quarantine and no real user config directory should be used for destructive tests.

- [ ] **Step 5: Run GREEN and all plugin UI regressions**

```powershell
pnpm test -- tests/frontend/pluginRemovalDialog.test.ts tests/frontend/pluginCard.test.ts tests/frontend/pluginMarketplacePage.test.ts
pnpm test -- tests/frontend/pluginImportDialog.test.ts tests/frontend/pluginStore.test.ts tests/frontend/pluginService.test.ts
pnpm exec vue-tsc --noEmit
pnpm lint
```

Expected: dialog keyboard/focus semantics, exact removal availability, distinct ownership notices, result-only deletion, and reload-only unknown handling pass without regressing import UX.

- [ ] **Step 6: Commit**

```powershell
git add src/components/plugins/PluginRemovalDialog.vue src/components/plugins/PluginCard.vue src/components/plugins/pluginPresentation.ts src/components/plugins/PluginMarketplacePage.vue src/components/plugins/PluginMarketplacePage.css tests/frontend/pluginRemovalDialog.test.ts tests/frontend/pluginCard.test.ts tests/frontend/pluginMarketplacePage.test.ts docs/plugin-local-manifests.md
git commit -m "feat(plugin): add managed package removal experience"
```

### Task 12: Prove crash recovery, static safety, nonzero three-platform CI filters, and full regression

**Files:**
- Modify (test-only hooks): `src-tauri/src/storage/safe_plugin_document.rs`
- Modify: `src-tauri/src/storage/safe_plugin_document/tests.rs`
- Modify: `src-tauri/src/storage/managed_plugin_ownership/tests.rs`
- Modify (test-only hooks): `src-tauri/src/storage/local_plugin_package.rs`
- Modify: `src-tauri/src/storage/local_plugin_package/tests.rs`
- Modify: `src-tauri/src/plugin/runtime/import/tests.rs`
- Modify (test-only hooks): `src-tauri/src/plugin/runtime/removal.rs`
- Modify: `src-tauri/src/plugin/runtime/removal/tests.rs`
- Modify: `.github/workflows/ci.yml`
- Modify: `tests/frontend/pluginSecurityWorkflow.test.ts`
- Create: `tests/frontend/pluginLifecycleSecurity.test.ts`

**Interfaces:**
- Adds named `current_exe` child checkpoints for every import/removal commit boundary. Parents observe a temporary injected plugins root and a watchdog may terminate only the child process.
- Adds compile-time/test-only fault hooks; production constructors and behavior remain unchanged.
- Extends `plugin-security` on Ubuntu, Windows, and macOS with the four required new filters and a nonzero-test assertion for every old/new filter.
- Adds a static source guard for the fixed frontend payload, forbidden lifecycle deletion/rename fallbacks, execution/download/dynamic-command expansion, and capability union.

- [ ] **Step 1: Write RED crash-matrix, workflow, and static-contract tests**

Extend the existing import `current_exe` pattern instead of inventing an in-process “crash.” Import checkpoints are exact two-file stage complete, disabled saved, promotion returned, target verified, ownership main committed, candidate built, and published. Add one ignored child entry selected only by a private environment variable and one parent matrix test.

In `runtime/removal/tests.rs`, add a separate ignored child entry and parent test covering disabled saved, removing main committed, rename returned before parent sync, quarantine verified, candidate scanned, manifest removed, receipt removed, directory removed, index deleted, and final publication. The child exits at the requested checkpoint; the parent's ten-second watchdog polls and kills only that child on timeout. It never terminates a shell, test runner, or unrelated process.

After each import child exit, reopen state/index and scan both roots to assert: pre-promotion is not imported; promoted without entry is external; exact committed entry reconciles managed; no receipt reconstructs an entry. After each removal child exit, assert only actual safe shapes: pre-rename source remains without automatic rename; exact source may converge by index rollback; post-rename full/receipt-only/empty/both-absent are cleanup pending or safely stale-entry converged; manifest-only/mismatch is conflict; restart never unlinks/rmdirs quarantine. On Windows assert only process-crash recovery, not power-loss durability; on Unix require parent-sync durability before a later package rename checkpoint can occur.

Update `pluginSecurityWorkflow.test.ts` to require all three OS values and these exact filters:

```text
plugin::discovery::safe_fs::tests
plugin::import
storage::local_plugin_import
storage::plugin_state::tests
plugin::runtime::import::tests
storage::managed_plugin_ownership::tests
storage::local_plugin_package::tests
plugin::ownership::tests
plugin::runtime::removal::tests
```

Also require the job to inspect `--list` output and reject zero selected tests before running each filter. In `pluginLifecycleSecurity.test.ts`, scope source reads to production modules and assert: the removal invoke object has only `id` and `expectedCatalogGeneration`; lifecycle Rust has no `remove_dir_all`, `MoveFileExW`, absolute delete/overwrite fallback, downloader, process execution, dynamic library loading, or dynamic command dispatch; package rename always uses no-replace; only secure-document replacement may use replace-existing; capability files grant no dialog/fs/shell/remote scope.

- [ ] **Step 2: Run RED and verify each failure is intentional**

```powershell
pnpm test -- tests/frontend/pluginSecurityWorkflow.test.ts tests/frontend/pluginLifecycleSecurity.test.ts
cargo test --locked --manifest-path src-tauri/Cargo.toml process_crash_checkpoints_reconstruct_only_committed_disk_state --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml removal_process_crash_checkpoints_converge_without_automatic_delete --lib
```

Expected: workflow test reports missing new/nonzero filters, and crash tests report missing checkpoints or incorrect recovery classification. The static test should either pass immediately or identify a concrete forbidden production fallback; it must not fail on test-fixture cleanup.

- [ ] **Step 3: Add deterministic child checkpoints and complete the fault matrices**

Expose checkpoints only behind `cfg(test)` controls already introduced in earlier tasks. Child processes use a canonical disposable root supplied by the parent; all receipt/index/state bytes are real, and restart assertions use production loaders/scanners. Preserve abandoned/unknown evidence rather than cleaning the fixture until the parent has classified it.

```rust
fn run_crash_child(root: &Path, checkpoint: &str, exact_test: &str) -> ExitStatus {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", exact_test, "--nocapture"])
        .env_clear()
        .env("EASIFLUX_PLUGIN_TEST_ROOT", root)
        .env("EASIFLUX_PLUGIN_TEST_CHECKPOINT", checkpoint)
        .spawn()
        .unwrap();
    wait_with_ten_second_child_only_watchdog(&mut child, checkpoint)
}
```

Cover every row from spec §11 plus:

- every Task 2 document write/replace/sync fault and its exact `PersistOutcome`;
- import index failure preserving an external package;
- removal preflight/storage/identity failures before rename;
- post-rename sync, scan, physical cleanup, index cleanup, publication, worker, response/caller-loss failures;
- FIFO import/remove/reload/toggle ordering;
- the same-generation re-enable race before removal storage;
- a late removal snapshot losing to a newer generation with a replacement external package.

Use injected barriers/channels, not timing-only sleeps, for concurrency ordering. Keep only the watchdog's short poll for child liveness. Unix swap tests assert only that controlled observable deviations fail closed and preserve evidence; they must not claim all out-of-bound same-user races are detectable. Windows tests must prove both source rename and child cleanup act on held object handles.

- [ ] **Step 4: Make every CI filter prove it selected at least one test**

Keep `plugin-security` as the existing three-OS matrix. Use a cross-platform PowerShell step that lists then runs every suite:

```powershell
$suites = @(
  'plugin::discovery::safe_fs::tests',
  'plugin::import',
  'storage::local_plugin_import',
  'storage::plugin_state::tests',
  'plugin::runtime::import::tests',
  'storage::managed_plugin_ownership::tests',
  'storage::local_plugin_package::tests',
  'plugin::ownership::tests',
  'plugin::runtime::removal::tests'
)
foreach ($suite in $suites) {
  $listed = & cargo test --locked --manifest-path src-tauri/Cargo.toml $suite --lib -- --list
  if ($LASTEXITCODE -ne 0 -or -not ($listed | Select-String '^[1-9][0-9]* tests?, 0 benchmarks$')) {
    throw "plugin-security filter selected zero tests: $suite"
  }
  & cargo test --locked --manifest-path src-tauri/Cargo.toml $suite --lib
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
```

Set this step's shell explicitly to `pwsh`. Do not remove the ordinary full Rust job. Keep workflow permissions at `contents: read` and add no secrets, caches with write authority, or platform skips.

- [ ] **Step 5: Run focused GREEN, then the complete required verification**

```powershell
pnpm test -- tests/frontend/pluginSecurityWorkflow.test.ts tests/frontend/pluginLifecycleSecurity.test.ts
cargo test --locked --manifest-path src-tauri/Cargo.toml process_crash_checkpoints_reconstruct_only_committed_disk_state --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml removal_process_crash_checkpoints_converge_without_automatic_delete --lib
pnpm test
pnpm exec vue-tsc --noEmit
pnpm lint
pnpm build
cargo test --locked --all-targets --manifest-path src-tauri/Cargo.toml
cargo clippy --locked --all-targets --manifest-path src-tauri/Cargo.toml
$changedRust = git diff --name-only dd2cdeb...HEAD -- 'src-tauri/**/*.rs'
foreach ($path in $changedRust) { rustfmt --edition 2021 --check $path }
git diff --check dd2cdeb...HEAD
git status --short --branch
```

Expected: all commands exit zero; each platform-filter test is nonzero in CI; the only pending changes before the commit are this task's exact files. If a disposable VM/profile is available, additionally smoke-test new import -> managed after restart -> disable -> remove -> absent from discovery, historical one-file -> external, and the unaffected main business pages. Without an isolated profile, record native smoke as not executed; never point destructive testing at the real user configuration directory.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/storage/safe_plugin_document.rs src-tauri/src/storage/safe_plugin_document/tests.rs src-tauri/src/storage/managed_plugin_ownership/tests.rs src-tauri/src/storage/local_plugin_package.rs src-tauri/src/storage/local_plugin_package/tests.rs src-tauri/src/plugin/runtime/import/tests.rs src-tauri/src/plugin/runtime/removal.rs src-tauri/src/plugin/runtime/removal/tests.rs .github/workflows/ci.yml tests/frontend/pluginSecurityWorkflow.test.ts tests/frontend/pluginLifecycleSecurity.test.ts
git commit -m "test(plugin): prove managed package lifecycle safety"
```

## Plan completion audit

Before opening the implementation PR, compare every completed task against the full `dd2cdeb` spec, inspect `git diff --stat dd2cdeb...HEAD`, and confirm all twelve task commits are present in order. Verify specifically that the final tree contains no ownership inference from `pkg-*`, no path/receipt/object identity in IPC, no startup/reload quarantine deletion, no second removal slot, no illegal `disabledDecisionSaved` combination, no reuse of `(revision,generation)` for a different complete DTO, and no platform claim stronger than the implemented filesystem primitive.
