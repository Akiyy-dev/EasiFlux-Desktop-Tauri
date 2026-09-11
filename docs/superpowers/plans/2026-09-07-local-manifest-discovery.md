# Local Manifest Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add bounded discovery and management of metadata-only local plugin manifests without introducing any third-party code execution surface.

**Architecture:** Rust discovers a single fixed config-root directory through platform-specific no-follow file handles, converts validated manifests into host-owned trust records, and atomically replaces a local catalog slice. A `PluginRuntime` serializes discovery and publishes a catalog generation distinct from the persisted enable-state revision; Vue strictly validates the v2 transport and reconciles responses by both counters.

**Tech Stack:** Rust 1.97, Serde/serde_json, sha2, rustix on Unix, windows-sys on Windows, Tokio, Tauri 2 ACL, Vue 3, Pinia 3, TypeScript 5.6, Vitest, Vue Test Utils, pnpm 9.

**Spec:** `docs/superpowers/specs/2026-09-07-local-manifest-discovery-design.md`

## Global Constraints

- Production discovery root is exactly `<dirs::config_dir()>/EasiFlux Desktop/plugins/local`; no IPC accepts a path.
- A package is exactly `local/pkg-<32 lowercase hex>/manifest.json`; slot is opaque and is never an identity field.
- Scan at most 256 direct root entries, accept at most 128 packages, read at most 16 KiB per manifest and 2 MiB per scan.
- Never recurse, follow symlinks/reparse points, accept extra package files, or use ordinary `File::open` after only a metadata check.
- Unix manifest opens include `NONBLOCK | NOCTTY` before `fstat`, so a FIFO with no writer cannot hold the reload gate.
- `PluginManifestV1` stays at schema 1 and requires empty contributions and requested capabilities.
- Catalog snapshot and mutation envelope are schema 2; state file writes are schema 2 with strict v1 migration.
- `PluginSource` wire values are exactly `builtIn` and `localDeclarative`.
- Built-in fingerprint is exactly `v1:none`; local fingerprint is exactly `v1:sha256:<64 lowercase hex>` over the validated canonical manifest and domain `EasiFlux.localDeclarative.manifest.v1\0`.
- `revision` changes only for logical persisted-state changes; `catalogGeneration` changes only for a newly published catalog/discovery state, with first discovery publishing generation 1.
- Root-level discovery failure clears only the local slice; built-ins, persisted decisions, and core startup remain available.
- No installer, downloader, updater, uninstaller, watcher, event channel, directory picker, dynamic command, JS/HTML/WASM/native code, resources, or contribution executor.
- Every task follows RED -> GREEN -> focused verification -> commit. Do not weaken a failing test to make it pass.
- Preserve unrelated primary-checkout changes; all work occurs in `target/worktrees/plugin-local-manifests` on `plugin/local-manifests`.

## File Responsibility Map

| File | Responsibility |
|---|---|
| `src-tauri/src/plugin/manifest.rs` | Strict manifest and catalog transport DTOs; no filesystem policy. |
| `src-tauri/src/plugin/record.rs` | Host-owned source, identity and canonical approval fingerprint. |
| `src-tauri/src/storage/plugin_state.rs` | Strict state v1/v2 recovery, migration signal and atomic v2 persistence. |
| `src-tauri/src/plugin/discovery/safe_fs.rs` | Platform-specific bounded, no-follow directory and file reads. |
| `src-tauri/src/plugin/discovery.rs` | Slot/package validation, manifest parsing, duplicate isolation and aggregate outcome. |
| `src-tauri/src/plugin/registry.rs` | Built-in/local merge, catalog generation and transactional enable decisions. |
| `src-tauri/src/plugin/runtime.rs` | Lazy first scan, explicit serialized reload and async/blocking boundary. |
| `src-tauri/src/commands/plugin.rs` | Three fixed Tauri command adapters only. |
| `src/services/pluginService.ts` | Exact v2 IPC validation and sanitized error mapping. |
| `src/stores/plugin.ts` | Generation/revision response arbitration and request ownership. |
| `src/components/plugins/pluginPresentation.ts` | Central source/status presentation copy. |
| `src/components/plugins/PluginCard.vue` | One plugin's accessible state and trust disclosure. |
| `src/components/plugins/PluginMarketplacePage.vue` | Installed/market/manage membership, reload and discovery health. |
| `docs/plugin-local-manifests.md` | End-user package layout and metadata-only contract. |

---

### Task 1: Host-owned trust record and transport v2

**Files:**

- Create: `src-tauri/src/plugin/record.rs`
- Modify: `src-tauri/src/plugin/mod.rs`
- Modify: `src-tauri/src/plugin/manifest.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Modify: `src-tauri/src/plugin/registry/tests.rs`
- Modify: `src-tauri/src/commands/plugin.rs`
- Modify: `src-tauri/src/state.rs`
- Test: `src-tauri/src/plugin/manifest.rs`
- Test: `src-tauri/src/plugin/record.rs`

**Interfaces:**

- Consumes: existing `PluginId`, `PluginPublisherId`, `PluginManifestV1`, `PluginCatalogItem`, `PluginAvailability`.
- Produces: `PluginRecord::{built_in, local_declarative, identity}`, `PluginIdentity`, `PluginSource::LocalDeclarative`, `LocalDiscoveryStatus`, `LocalDiscoverySummary::{available, degraded, unavailable}`, schema-2 `PluginCatalogSnapshot` and `PluginCatalogMutationResult`.

- [ ] **Step 1: Add RED tests for map-only manifest decoding and canonical local identity**

Add tests that use two JSON objects with different key order and one positional JSON array:

```rust
#[test]
fn local_fingerprint_ignores_json_layout_but_binds_semantics() {
    let first: PluginManifestV1 = serde_json::from_str(VALID_MANIFEST_JSON).unwrap();
    let reordered: PluginManifestV1 = serde_json::from_str(REORDERED_MANIFEST_JSON).unwrap();
    let first = PluginRecord::local_declarative(first).unwrap();
    let reordered = PluginRecord::local_declarative(reordered).unwrap();
    assert_eq!(first.approval_fingerprint(), reordered.approval_fingerprint());

    let mut changed = reordered.manifest().clone();
    changed.name = "Changed name".to_owned();
    let changed = PluginRecord::local_declarative(changed).unwrap();
    assert_ne!(first.approval_fingerprint(), changed.approval_fingerprint());
}

#[test]
fn manifest_rejects_positional_struct_representation() {
    assert!(serde_json::from_str::<PluginManifestV1>(
        r#"[1,"com.example.alpha","com.example","Example","Alpha","Metadata","1.0.0",[],[]]"#,
    )
    .is_err());
}
```

- [ ] **Step 2: Run the focused tests and confirm RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin::manifest::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml plugin::record::tests --lib
```

Expected: compilation fails because `PluginRecord` and `LocalDeclarative` do not exist, or the positional array is accepted.

- [ ] **Step 3: Implement strict manifest decoding and the trust record**

Move derived wire decoding behind a `Visitor::visit_map` implementation so sequences cannot deserialize. Import `sha2::{Digest, Sha256}` and add the record with a frozen fingerprint DTO:

```rust
const LOCAL_FINGERPRINT_DOMAIN: &[u8] = b"EasiFlux.localDeclarative.manifest.v1\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginRecord {
    manifest: PluginManifestV1,
    source: PluginSource,
    approval_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginIdentity {
    pub(crate) id: PluginId,
    pub(crate) source: PluginSource,
    pub(crate) publisher_id: PluginPublisherId,
    pub(crate) approval_fingerprint: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalManifestV1<'a> {
    schema_version: u32,
    id: &'a PluginId,
    publisher_id: &'a PluginPublisherId,
    publisher: &'a str,
    name: &'a str,
    description: &'a str,
    version: &'a semver::Version,
    contributions: &'a [serde_json::Value],
    requested_capabilities: &'a [String],
}

impl<'a> From<&'a PluginManifestV1> for CanonicalManifestV1<'a> {
    fn from(manifest: &'a PluginManifestV1) -> Self {
        Self {
            schema_version: manifest.schema_version,
            id: &manifest.id,
            publisher_id: &manifest.publisher_id,
            publisher: &manifest.publisher,
            name: &manifest.name,
            description: &manifest.description,
            version: &manifest.version,
            contributions: &manifest.contributions,
            requested_capabilities: &manifest.requested_capabilities,
        }
    }
}

impl PluginRecord {
    pub(crate) fn built_in(manifest: PluginManifestV1) -> Result<Self, String> {
        manifest.validate()?;
        Ok(Self {
            manifest,
            source: PluginSource::BuiltIn,
            approval_fingerprint: APPROVAL_FINGERPRINT_NONE.to_owned(),
        })
    }

    pub(crate) fn local_declarative(manifest: PluginManifestV1) -> Result<Self, String> {
        manifest.validate()?;
        let canonical = CanonicalManifestV1::from(&manifest);
        let bytes = serde_json::to_vec(&canonical)
            .map_err(|_| "manifest fingerprint failed".to_owned())?;
        let mut digest = Sha256::new();
        digest.update(LOCAL_FINGERPRINT_DOMAIN);
        digest.update(bytes);
        Ok(Self {
            manifest,
            source: PluginSource::LocalDeclarative,
            approval_fingerprint: format!("v1:sha256:{}", hex::encode(digest.finalize())),
        })
    }

    pub(crate) fn manifest(&self) -> &PluginManifestV1 { &self.manifest }
    pub(crate) fn source(&self) -> PluginSource { self.source }
    pub(crate) fn approval_fingerprint(&self) -> &str { &self.approval_fingerprint }
    pub(crate) fn identity(&self) -> PluginIdentity {
        PluginIdentity {
            id: self.manifest.id.clone(),
            source: self.source,
            publisher_id: self.manifest.publisher_id.clone(),
            approval_fingerprint: self.approval_fingerprint.clone(),
        }
    }
}
```

Validate before hashing. Serialize a private struct containing exactly schemaVersion, id, publisherId, publisher, name, description, version, contributions and requestedCapabilities in that field order; hash domain then bytes with `Sha256` and format lowercase through `hex`.

- [ ] **Step 4: Add RED tests for exact schema-2 envelopes**

Assert full JSON equality, including the independent manifest schema:

```rust
#[test]
fn catalog_transport_v2_carries_generation_and_discovery() {
    let snapshot = PluginCatalogSnapshot::new(
        "9".to_owned(),
        "3".to_owned(),
        PluginAvailability::Available,
        None,
        LocalDiscoverySummary::available(),
        Vec::new(),
    );
    assert_eq!(
        serde_json::to_value(snapshot).unwrap(),
        serde_json::json!({
            "schemaVersion": 2,
            "revision": "9",
            "catalogGeneration": "3",
            "availability": "available",
            "availabilityReasonCode": null,
            "localDiscovery": {"status": "available", "rejectedPackageCount": 0},
            "plugins": []
        }),
    );
}
```

- [ ] **Step 5: Implement transport v2 and minimally adapt registry constructors**

Add:

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginSource { BuiltIn, LocalDeclarative }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalDiscoveryStatus { Available, Degraded, Unavailable }

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDiscoverySummary {
    pub status: LocalDiscoveryStatus,
    pub rejected_package_count: u32,
}

impl LocalDiscoverySummary {
    pub fn available() -> Self {
        Self { status: LocalDiscoveryStatus::Available, rejected_package_count: 0 }
    }

    pub fn degraded(rejected_package_count: u32) -> Result<Self, String> {
        if !(1..=256).contains(&rejected_package_count) {
            return Err("degraded discovery count must be between 1 and 256".to_owned());
        }
        Ok(Self { status: LocalDiscoveryStatus::Degraded, rejected_package_count })
    }

    pub fn unavailable() -> Self {
        Self { status: LocalDiscoveryStatus::Unavailable, rejected_package_count: 0 }
    }
}
```

Implement `LocalDiscoverySummary::available()` as available/0, `degraded(count)` as a checked constructor accepting only 1..=256, and `unavailable()` as unavailable/0. Use a dedicated `CATALOG_TRANSPORT_SCHEMA_VERSION: u8 = 2`; do not reuse the manifest schema constant. Add `schema_version`, `catalog_generation` and `local_discovery` to snapshot and add `schema_version` plus `catalog_generation` to mutation. Keep registry behavior built-in-only for this task, constructing built-in `PluginRecord`s and using generation `0` with an available empty local summary until Task 4. Update existing registry, command and startup JSON assertions to the exact v2 envelope so the full Rust suite remains green.

- [ ] **Step 6: Run focused and regression tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin::manifest::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml plugin::record::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml plugin::registry::tests --lib
```

Expected: all selected tests pass; no manifest or record test accepts nonempty capabilities/contributions.

- [ ] **Step 7: Commit Task 1**

```powershell
git add src-tauri/src/plugin/manifest.rs src-tauri/src/plugin/mod.rs src-tauri/src/plugin/record.rs src-tauri/src/plugin/registry.rs src-tauri/src/plugin/registry/tests.rs src-tauri/src/commands/plugin.rs src-tauri/src/state.rs
git commit -m "feat(plugin): add trust records and catalog transport v2"
```

### Task 2: Strict state v2 migration and persistence

**Files:**

- Modify: `src-tauri/src/storage/plugin_state.rs`
- Modify: `src-tauri/src/storage/plugin_state/tests.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Modify: `src-tauri/src/plugin/registry/tests.rs`
- Modify: `src-tauri/src/commands/plugin.rs`
- Modify: `src-tauri/src/state.rs`

**Interfaces:**

- Consumes: `PluginIdentity`, `PluginSource::{BuiltIn, LocalDeclarative}` and the Phase 0 copy-on-write registry transaction.
- Produces: `PluginStateEntryV2`, `PluginStateFileV2`, `PluginStateLoad { state, requires_rewrite }` and updated `PluginStatePersistence::{load, save}`. Task 4 consumes these types for local identity cleanup.

- [ ] **Step 1: Add RED migration and future-schema tests**

Use real state candidate files in temporary directories:

```rust
#[test]
fn v1_load_is_lossless_and_marks_v2_rewrite() {
    let fixture = state_store_fixture();
    fixture.write_primary(br#"{"schemaVersion":1,"revision":"7","entries":[{"id":"com.easiflux.alpha","source":"builtIn","publisherId":"com.easiflux","approvalFingerprint":"v1:none","enabled":true}]}"#);
    let loaded = fixture.store.load().unwrap();
    assert!(loaded.requires_rewrite);
    assert_eq!(loaded.state.schema_version, 2);
    assert_eq!(loaded.state.revision, 7);
    assert!(loaded.state.entries[0].enabled);
}

#[test]
fn future_primary_schema_never_falls_back_to_older_backup() {
    let fixture = state_store_fixture();
    fixture.write_primary(br#"{"schemaVersion":3,"revision":"8","entries":[]}"#);
    fixture.write_backup(valid_v2_document("7"));
    assert_plugin_code(fixture.store.load(), "plugin_state_unsupported_schema");
}
```

- [ ] **Step 2: Run state tests and confirm RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml storage::plugin_state::tests --lib
```

Expected: tests fail because only `PluginStateFileV1` exists.

- [ ] **Step 3: Implement separate strict v1 wire decode and canonical v2 storage**

Introduce:

```rust
pub(crate) struct PluginStateLoad {
    pub(crate) state: PluginStateFileV2,
    pub(crate) requires_rewrite: bool,
}

pub(crate) trait PluginStatePersistence: Send + Sync {
    fn load(&self) -> AppResult<PluginStateLoad>;
    fn save(&self, state: &PluginStateFileV2) -> AppResult<()>;
}
```

Keep a private `PluginStateFileV1Wire` whose source type only permits `builtIn`. Make both entry versions map-only and `deny_unknown_fields`; validate canonical decimal revision, 512 entries, composite-key uniqueness and source/fingerprint pairing. Save only schema 2 and preserve the existing bounded atomic replacement/recovery order.

- [ ] **Step 4: Add RED registry tests for deferred migration writes**

Use only the existing built-in catalog and Phase 0 two-argument mutator in this task:

```rust
#[test]
fn same_value_decision_rewrites_loaded_v1_as_v2_without_revision_change() {
    let persistence = MemoryPersistence::loaded_from_v1(enabled_builtin_state("7"));
    let mut registry = persistence.registry_with_builtin();

    let result = registry.set_enabled("com.easiflux.alpha", true).unwrap();

    assert_eq!(result.revision, "7");
    let saved = persistence.last_save().unwrap();
    assert_eq!(saved.schema_version, 2);
    assert_eq!(saved.revision, 7);
}

#[test]
fn failed_migration_rewrite_keeps_state_and_rewrite_marker() {
    let persistence = MemoryPersistence::failing_loaded_from_v1(enabled_builtin_state("7"));
    let mut registry = persistence.registry_with_builtin();
    assert!(registry.set_enabled("com.easiflux.alpha", true).is_err());
    assert_eq!(registry.catalog_snapshot().revision, "7");
    persistence.allow_saves();
    registry.set_enabled("com.easiflux.alpha", true).unwrap();
    assert_eq!(persistence.last_save().unwrap().schema_version, 2);
}
```

- [ ] **Step 5: Adapt the built-in registry to the migration result**

Update all persistence fakes in registry, command and state modules to return `PluginStateLoad`. Keep the existing two-argument `set_enabled` signature until Task 4. If an entry decision changes, increment revision once with checked arithmetic; if the decision is unchanged but `requires_rewrite` is true, save the canonical v2 document without changing revision. Enforce count and serialized-byte limits before save. Commit memory and clear `requires_rewrite` only after save succeeds.

- [ ] **Step 6: Run state and registry regression tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml storage::plugin_state::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml plugin::registry::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml commands::plugin::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml state::tests --lib
```

Expected: all selected tests pass; v1 files are readable, all new saves are v2, and save failure remains copy-on-write.

- [ ] **Step 7: Commit Task 2**

```powershell
git add src-tauri/src/storage/plugin_state.rs src-tauri/src/storage/plugin_state/tests.rs src-tauri/src/plugin/registry.rs src-tauri/src/plugin/registry/tests.rs src-tauri/src/commands/plugin.rs src-tauri/src/state.rs
git commit -m "feat(plugin): migrate enable state to schema v2"
```

### Task 3: Bounded cross-platform no-follow filesystem reader

**Files:**

- Create: `src-tauri/src/plugin/discovery/safe_fs.rs`
- Create: `src-tauri/src/plugin/discovery/safe_fs/tests.rs`
- Create: `src-tauri/src/plugin/discovery.rs`
- Modify: `src-tauri/src/plugin/mod.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**

- Consumes: only absolute fixed root `Path` and explicit `ScanLimits`; it does not parse JSON.
- Produces: `safe_fs::read_package_candidates(root, limits) -> Result<PackageScan, RootReadError>` with bounded same-handle bytes and aggregate package rejections.

- [ ] **Step 1: Add target-specific dependencies**

Add exactly:

```toml
[target.'cfg(unix)'.dependencies]
rustix = { version = "1.1", features = ["fs"] }

[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61", features = ["Win32_Storage_FileSystem"] }
```

Run `cargo check --manifest-path src-tauri/Cargo.toml` once so Cargo updates only the lockfile dependency roots already present transitively.

- [ ] **Step 2: Add RED portable limit, slot and shape tests**

Define the production limits in test expectations:

```rust
#[test]
fn slot_parser_accepts_only_pkg_prefix_and_32_lower_hex() {
    assert!(parse_slot_name(OsStr::new("pkg-550e8400e29b41d4a716446655440000")).is_some());
    for invalid in [
        "con.plugin",
        "com1.foo",
        "nul.anything",
        "pkg-550E8400e29b41d4a716446655440000",
        "550e8400-e29b-41d4-a716-446655440000",
        "pkg-550e8400e29b41d4a71644665544000",
    ] {
        assert!(parse_slot_name(OsStr::new(invalid)).is_none(), "{invalid}");
    }
}

#[test]
fn package_requires_exactly_one_case_exact_manifest_file() {
    let fixture = scan_fixture();
    fixture.package("pkg-550e8400e29b41d4a716446655440000")
        .file("Manifest.json", b"{}");
    let scan = read_package_candidates(fixture.root(), ScanLimits::production()).unwrap();
    assert!(scan.packages.is_empty());
    assert_eq!(scan.rejected_package_count, 1);
}
```

Add separate tests for missing root, 16 KiB/16 KiB+1, 256/257 direct entries, 128/129 structurally valid packages, 2 MiB total input, extra file, nested directory, manifest-as-directory and deterministic slot ordering.

- [ ] **Step 3: Run safe filesystem tests and confirm RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin::discovery::safe_fs::tests --lib
```

Expected: compilation fails because the scanner types and functions do not exist.

- [ ] **Step 4: Implement shared bounded policy and Unix handle-relative opening**

Create these shared types:

```rust
pub(super) struct ScanLimits {
    pub(super) max_root_entries: usize,
    pub(super) max_packages: usize,
    pub(super) max_manifest_bytes: usize,
    pub(super) max_total_bytes: usize,
}

pub(super) struct PackageBytes {
    pub(super) slot: LocalPackageSlot,
    pub(super) manifest_bytes: Vec<u8>,
}

pub(super) struct PackageScan {
    pub(super) packages: Vec<PackageBytes>,
    pub(super) rejected_package_count: u32,
}
```

On Unix, walk every absolute-root component with `rustix::fs::openat` and `OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC`. Enumerate from held directory descriptors and open slots by one component relative to their parent descriptor. Open the exact manifest with `OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOCTTY`, immediately `fstat`, reject every non-regular type before reading, then read `max + 1` bytes from the same descriptor. Do not replace this with a pre-open type check, which would reintroduce a swap race.

- [ ] **Step 5: Implement Windows reparse-aware held-handle opening**

Use `std::os::windows::fs::{MetadataExt, OpenOptionsExt}` and constants from `windows-sys::Win32::Storage::FileSystem`. Open each local-drive absolute-path prefix with `FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT`, `FILE_READ_ATTRIBUTES`, and a share mode without `FILE_SHARE_DELETE`; keep ancestor handles alive through the scan. Reject every opened handle with `FILE_ATTRIBUTE_REPARSE_POINT`.

Open manifest handles with read access, `FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_SEQUENTIAL_SCAN`, and `FILE_SHARE_READ` only. Verify non-directory, non-reparse regular metadata from that handle, then perform the bounded read from it. Enumerate package contents before and after reading and reject the package if the exact one-entry shape changed. Fail closed for relative roots, UNC roots and sharing violations.

- [ ] **Step 6: Add platform security tests**

Under `#[cfg(unix)]`, add real symlink tests for a root intermediate component, slot and manifest. Add a FIFO named `manifest.json` with no writer; run the scan under a 500 ms receive timeout, assert it returns a package rejection, then replace the FIFO with a regular manifest and assert the next scan succeeds. Under `#[cfg(windows)]`, test the attribute predicate, real directory junction, file symlink and “opened manifest cannot be renamed while held”; only `ERROR_PRIVILEGE_NOT_HELD` may explicitly skip symlink creation.

```rust
#[cfg(windows)]
#[test]
fn reparse_attribute_is_always_rejected() {
    assert!(is_reparse_point(FILE_ATTRIBUTE_REPARSE_POINT));
    assert!(is_reparse_point(FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT));
}
```

- [ ] **Step 7: Add a Windows discovery security CI job**

Extend `.github/workflows/ci.yml` with a `windows-latest` Rust job using the same stable toolchain and cache strategy as the existing Rust job, then run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin::discovery::safe_fs::tests --lib
```

Do not reduce the existing Ubuntu full Rust job.

- [ ] **Step 8: Run focused tests on the current platform**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin::discovery::safe_fs::tests --lib
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
```

Expected on this Windows worktree: portable and Windows security tests pass; no scan returns raw path or OS error data.

- [ ] **Step 9: Commit Task 3**

```powershell
git add .github/workflows/ci.yml src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/plugin/mod.rs src-tauri/src/plugin/discovery.rs src-tauri/src/plugin/discovery/safe_fs.rs src-tauri/src/plugin/discovery/safe_fs/tests.rs
git commit -m "feat(plugin): add bounded local package reader"
```

### Task 4: Local discovery policy, registry merge and catalog generation

**Files:**

- Modify: `src-tauri/src/plugin/discovery.rs`
- Create: `src-tauri/src/plugin/discovery/tests.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Modify: `src-tauri/src/plugin/registry/tests.rs`
- Modify: `src-tauri/src/plugin/manifest.rs`
- Modify: `src-tauri/src/commands/plugin.rs`

**Interfaces:**

- Consumes: `PackageScan`, strict `PluginManifestV1`, `PluginRecord::local_declarative`, `PluginStateFileV2`.
- Produces: `LocalPluginDiscovery`, `SystemLocalPluginDiscovery`, `LocalDiscoveryOutcome`, `PluginRegistry::apply_local_discovery`, `PluginRegistry::catalog_generation`, and `PluginRegistry::set_enabled(&mut self, id: &str, enabled: bool, expected_catalog_generation: &str)`.

- [ ] **Step 1: Add RED discovery parsing and duplicate-isolation tests**

```rust
#[test]
fn invalid_package_is_isolated_and_duplicate_ids_have_no_winner() {
    let scan = package_scan([
        package("pkg-00000000000000000000000000000001", valid_manifest("com.example.dup")),
        package("pkg-00000000000000000000000000000002", valid_manifest("com.example.dup")),
        package("pkg-00000000000000000000000000000003", valid_manifest("com.example.good")),
        package("pkg-00000000000000000000000000000004", br#"{"schemaVersion":1}"#),
    ]);
    let outcome = parse_package_scan(scan);
    assert_eq!(ids(&outcome.records), ["com.example.good"]);
    assert_eq!(outcome.summary, LocalDiscoverySummary::degraded(3).unwrap());
}
```

Add tests for top-level manifest array, duplicate object fields, unknown fields, BOM, invalid UTF-8, nonempty contribution/capability arrays and canonical ID ordering.

- [ ] **Step 2: Run discovery tests and confirm RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin::discovery::tests --lib
```

Expected: tests fail because parse/discovery policy is not implemented.

- [ ] **Step 3: Implement the discovery outcome and system resolver**

Use:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalDiscoveryOutcome {
    pub(crate) records: Vec<PluginRecord>,
    pub(crate) summary: LocalDiscoverySummary,
}

impl LocalDiscoveryOutcome {
    pub(crate) fn available(records: Vec<PluginRecord>) -> Self {
        Self { records, summary: LocalDiscoverySummary::available() }
    }

    pub(crate) fn degraded(records: Vec<PluginRecord>, rejected: u32) -> Result<Self, String> {
        Ok(Self { records, summary: LocalDiscoverySummary::degraded(rejected)? })
    }

    pub(crate) fn unavailable() -> Self {
        Self { records: Vec::new(), summary: LocalDiscoverySummary::unavailable() }
    }
}

pub(crate) trait LocalPluginDiscovery: Send + Sync {
    fn discover(&self) -> LocalDiscoveryOutcome;
}
```

`SystemLocalPluginDiscovery` resolves `dirs::config_dir()/APP_NAME/plugins/local` on every explicit scan and calls the safe reader. Missing root becomes available empty; root resolver/read errors become unavailable with empty records. Parse each package independently, reject all candidates in every duplicate-ID group, and expose only records plus aggregate summary.

- [ ] **Step 4: Add RED registry merge/generation tests**

```rust
#[test]
fn first_publish_is_generation_one_and_identical_reload_is_stable() {
    let mut registry = registry_with_builtins(Vec::new());
    let empty = LocalDiscoveryOutcome::available(Vec::new());
    assert!(registry.apply_local_discovery(empty.clone()).unwrap());
    assert_eq!(registry.catalog_generation(), 1);
    assert!(!registry.apply_local_discovery(empty).unwrap());
    assert_eq!(registry.catalog_generation(), 1);
}

#[test]
fn local_failure_clears_only_local_slice() {
    let mut registry = registry_with_builtin_and_local();
    registry.apply_local_discovery(LocalDiscoveryOutcome::unavailable()).unwrap();
    assert_eq!(catalog_ids(&registry), ["com.easiflux.builtin"]);
    assert_eq!(registry.catalog_snapshot().availability, PluginAvailability::Available);
}

#[test]
fn stale_catalog_generation_never_saves_state() {
    let mut fixture = registry_fixture_at_generation(2);
    assert_plugin_code(
        fixture.registry.set_enabled("com.example.alpha", true, "1"),
        "plugin_catalog_stale",
    );
    assert_eq!(fixture.persistence.save_count(), 0);
}

#[test]
fn explicit_disabled_for_new_local_identity_prunes_old_enabled_identity() {
    let old = local_record_named("Old");
    let new = local_record_named("New");
    let persistence = MemoryPersistence::with_entry(state_entry(&old, true));
    let mut registry = registry_with_local(new.clone(), persistence.clone());

    registry.set_enabled(new.manifest().id.as_str(), false, "1").unwrap();

    let saved = persistence.last_save().unwrap();
    assert_eq!(saved.entries.len(), 1);
    assert_eq!(saved.entries[0].approval_fingerprint, new.approval_fingerprint());
    assert!(!saved.entries[0].enabled);
}

#[test]
fn failed_identity_cleanup_does_not_mutate_memory_or_revision() {
    let mut fixture = failing_save_registry_with_replaced_local_identity();
    let before = fixture.registry.catalog_snapshot();
    assert!(fixture.registry.set_enabled("com.example.alpha", false, "1").is_err());
    assert_eq!(fixture.registry.catalog_snapshot(), before);
}
```

Cover built-in/local collision, local content replacement default-disabled, exact-identity recovery, degraded records, unavailable clearing, generation overflow and revision/generation independence.

- [ ] **Step 5: Implement atomic local merge and expected-generation mutations**

Store built-ins and locals in separate `BTreeMap<PluginId, PluginRecord>`. During apply, remove local records whose IDs collide with built-ins and add their count to the bounded summary. Compare normalized records plus summary with the currently published local state. On first apply or change, checked-increment generation and commit all fields together; on overflow return `plugin_catalog_generation_exhausted` without mutation.

Change the mutator signature to:

```rust
pub(crate) fn set_enabled(
    &mut self,
    id: &str,
    enabled: bool,
    expected_catalog_generation: &str,
) -> AppResult<PluginCatalogMutationResult>;
```

Parse expected generation as canonical u64 and reject any mismatch before state lookup/save. For an accepted decision, clone state, remove every entry with the current record's `(id, source)`, insert exactly the current full identity with the requested boolean, sort, validate bounds, persist, then commit. This cleanup also runs for an explicit disabled decision whose new identity is already effectively disabled. Build snapshot and mutation envelopes with current generation and current state revision.

Update `set_plugin_enabled_from`, the public Tauri mutator and all command tests to require/pass `expected_catalog_generation` immediately. Do not add a compatibility default that reads the current generation; Task 5 only moves these already-strict calls behind `PluginRuntime`.

- [ ] **Step 6: Run discovery, registry and state tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin::discovery::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml plugin::registry::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml storage::plugin_state::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml commands::plugin::tests --lib
```

Expected: all selected tests pass; a lower-trust collision never replaces a built-in record.

- [ ] **Step 7: Commit Task 4**

```powershell
git add src-tauri/src/plugin/discovery.rs src-tauri/src/plugin/discovery/tests.rs src-tauri/src/plugin/manifest.rs src-tauri/src/plugin/registry.rs src-tauri/src/plugin/registry/tests.rs src-tauri/src/commands/plugin.rs
git commit -m "feat(plugin): merge local manifests with catalog generations"
```

### Task 5: Plugin runtime, explicit reload command and ACL

**Files:**

- Create: `src-tauri/src/plugin/runtime.rs`
- Create: `src-tauri/src/plugin/runtime/tests.rs`
- Modify: `src-tauri/src/plugin/mod.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/commands/plugin.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/capabilities/plugin-runtime.json`
- Create: `src-tauri/permissions/autogenerated/reload_plugin_catalog.toml`

**Interfaces:**

- Consumes: `PluginRegistry`, `Arc<dyn LocalPluginDiscovery>`, schema-2 snapshot/mutation.
- Produces: `PluginRuntime::{get_catalog, reload_catalog, set_enabled}` and fixed Tauri command `reload_plugin_catalog`.

- [ ] **Step 1: Add RED runtime scheduling tests with a controlled discovery fake**

```rust
#[tokio::test]
async fn concurrent_first_gets_scan_once_and_later_get_does_not_rescan() {
    let discovery = Arc::new(CountingDiscovery::available_empty());
    let runtime = runtime_with(discovery.clone());
    let (left, right) = tokio::join!(runtime.get_catalog(), runtime.get_catalog());
    left.unwrap();
    right.unwrap();
    runtime.get_catalog().await.unwrap();
    assert_eq!(discovery.call_count(), 1);
}

#[tokio::test]
async fn scan_does_not_hold_the_registry_lock() {
    let discovery = Arc::new(BarrierDiscovery::new());
    let runtime = runtime_with(discovery.clone());
    let reload = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.reload_catalog().await }
    });
    discovery.wait_until_scanning().await;
    assert!(runtime.registry_snapshot_for_test().await.is_some());
    discovery.release();
    reload.await.unwrap().unwrap();
}
```

Also test serialized reload order, failed first discovery counted as attempted, explicit reload retry, and a toggle made before/after publication.

- [ ] **Step 2: Run runtime tests and confirm RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin::runtime::tests --lib
```

Expected: compilation fails because `PluginRuntime` does not exist.

- [ ] **Step 3: Implement `PluginRuntime` and blocking boundary**

Use this ownership shape:

```rust
pub(crate) struct PluginRuntime {
    registry: RwLock<PluginRegistry>,
    discovery: Arc<dyn LocalPluginDiscovery>,
    reload_gate: Mutex<()>,
    initial_discovery_attempted: AtomicBool,
}
```

`get_catalog` double-checks the atomic inside `reload_gate`, invokes one `spawn_blocking` discovery, publishes, marks attempted, retries state recovery if required, then snapshots. `reload_catalog` always takes the gate and scans. No blocking scan may hold a registry guard. `set_enabled` takes the registry write guard and delegates the expected-generation transaction.

- [ ] **Step 4: Add RED command and ACL tests**

Assert exact command JSON and arguments:

```rust
#[tokio::test]
async fn mutation_command_requires_expected_catalog_generation() {
    let runtime = runtime_with_one_local();
    let result = set_plugin_enabled_from(&runtime, "com.example.alpha", true, "1")
        .await
        .unwrap();
    let value = serde_json::to_value(result).unwrap();
    assert_eq!(value["schemaVersion"], 2);
    assert_eq!(value["catalogGeneration"], "1");
}
```

Extend the existing capability regression list to exactly `get_plugin_catalog`, `reload_plugin_catalog`, and `set_plugin_enabled`, each granted only to local `main` WebView.

- [ ] **Step 5: Wire state, commands, build manifest and capability**

Change `AppState.plugins` to `Arc<PluginRuntime>`. Add:

```rust
#[tauri::command]
pub async fn reload_plugin_catalog(
    state: State<'_, AppState>,
) -> AppResult<PluginCatalogSnapshot> {
    state.plugins.reload_catalog().await
}
```

Preserve Task 4's required `expected_catalog_generation: String` argument while moving `set_plugin_enabled` behind `PluginRuntime`. Add the reload command to `generate_handler!`, `build.rs` command allowlist and `plugin-runtime.json`; generate and commit its allow/deny permission file. Do not add a wildcard or path scope.

- [ ] **Step 6: Run runtime, command, startup and ACL tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin::runtime::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml commands::plugin::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml state::tests --lib
cargo test --manifest-path src-tauri/Cargo.toml capability_tests --lib
```

Expected: all selected tests pass; plugin discovery failure cannot escape `AppState` initialization.

- [ ] **Step 7: Commit Task 5**

```powershell
git add src-tauri/src/plugin/runtime.rs src-tauri/src/plugin/runtime/tests.rs src-tauri/src/plugin/mod.rs src-tauri/src/state.rs src-tauri/src/commands/plugin.rs src-tauri/src/lib.rs src-tauri/build.rs src-tauri/capabilities/plugin-runtime.json src-tauri/permissions/autogenerated/reload_plugin_catalog.toml src-tauri/permissions/autogenerated/get_plugin_catalog.toml src-tauri/permissions/autogenerated/set_plugin_enabled.toml
git commit -m "feat(plugin): add serialized local catalog reload"
```

### Task 6: Strict frontend v2 service and generation-safe store

**Files:**

- Modify: `src/types/plugin.ts`
- Modify: `src/services/pluginService.ts`
- Modify: `src/stores/plugin.ts`
- Modify: `tests/frontend/pluginService.test.ts`
- Modify: `tests/frontend/pluginStore.test.ts`
- Modify: `tests/frontend/pluginMarketplacePage.test.ts`

**Interfaces:**

- Consumes: exact backend v2 snapshot/mutation and the three fixed command names.
- Produces: `reloadPluginCatalog()`, `setPluginEnabled(id, enabled, expectedCatalogGeneration)`, store refs `catalogGeneration`, `localDiscovery`, `reloadStatus`, `reloadError`, and action `reload()`.

- [ ] **Step 1: Replace service fixtures with v2 and add RED parser tests**

Use an exact valid snapshot fixture:

```ts
const snapshotV2 = {
  schemaVersion: 2,
  revision: '4',
  catalogGeneration: '2',
  availability: 'available',
  availabilityReasonCode: null,
  localDiscovery: { status: 'degraded', rejectedPackageCount: 1 },
  plugins: [localPlugin],
}
```

Assert v1 is rejected; `localDeclarative` is accepted; unknown source/status/key, negative/fractional/count-over-256, and noncanonical generation are rejected. Assert `reloadPluginCatalog()` invokes `reload_plugin_catalog`, while mutation invokes:

```ts
expect(tauriInvoke).toHaveBeenCalledWith('set_plugin_enabled', {
  id: 'com.example.alpha',
  enabled: true,
  expectedCatalogGeneration: '2',
})
```

- [ ] **Step 2: Run service tests and confirm RED**

Run:

```powershell
corepack pnpm@9.15.9 test -- tests/frontend/pluginService.test.ts
```

Expected: tests fail because parser and service still require schema 1/builtIn only.

- [ ] **Step 3: Implement exact v2 types, parser and error closure**

Add:

```ts
export type PluginSource = 'builtIn' | 'localDeclarative'
export type LocalDiscoveryStatus = 'available' | 'degraded' | 'unavailable'

export interface PluginLocalDiscoverySummary {
  status: LocalDiscoveryStatus
  rejectedPackageCount: number
}
```

Require exact snapshot keys `schemaVersion`, `revision`, `catalogGeneration`, `availability`, `availabilityReasonCode`, `localDiscovery`, `plugins`. Require exact mutation keys `schemaVersion`, `revision`, `catalogGeneration`, `plugin`. Enforce summary correlations from the spec and count `<= 256`. Add fixed mappings for `plugin_catalog_stale`, `plugin_catalog_generation_exhausted`, and `plugin_state_capacity_exceeded`; never display backend `message`. Upgrade the page test's typed catalog fixtures to the exact v2 envelope without changing UI assertions in this task, keeping the complete frontend suite type-safe before Task 7.

- [ ] **Step 4: Add RED store race tests**

```ts
it('new generation replaces membership and late old mutation cannot revive it', async () => {
  const oldMutation = deferred<PluginCatalogMutationResult>()
  vi.mocked(setPluginEnabled).mockReturnValueOnce(oldMutation.promise)
  const store = usePluginStore()
  await store.load()
  const toggle = store.setEnabled('com.example.alpha', true)

  vi.mocked(reloadPluginCatalog).mockResolvedValueOnce(snapshotWithoutAlphaAtGeneration('2'))
  await store.reload()
  oldMutation.resolve(enabledAlphaMutationAtGeneration('1', '5'))
  await toggle

  expect(store.catalog.map(item => item.manifest.id)).not.toContain('com.example.alpha')
})
```

Add tests that an older snapshot is ignored, a newer generation fully replaces metadata/discovery state, same-generation higher per-item revision wins, load and reload flights coalesce separately, and stale errors preserve confirmed catalog.

- [ ] **Step 5: Run store tests and confirm RED**

Run:

```powershell
corepack pnpm@9.15.9 test -- tests/frontend/pluginStore.test.ts
```

Expected: tests fail because the store only arbitrates by revision.

- [ ] **Step 6: Implement two-dimensional response arbitration**

Add `catalogGeneration`, `localDiscovery`, `reloadStatus`, `reloadError`, separate `loadFlight`/`reloadFlight`, and `confirmedItemRevisions` scoped to the current generation.

Implement snapshot adoption in this order:

```ts
if (BigInt(snapshot.catalogGeneration) < BigInt(catalogGeneration.value)) return
if (BigInt(snapshot.catalogGeneration) > BigInt(catalogGeneration.value)) {
  replaceEntireGeneration(snapshot)
  return
}
mergeSameGenerationByRevision(snapshot)
```

Capture `requestGeneration` before calling `setPluginEnabled`. Apply a mutation only when its parsed generation equals both `requestGeneration` and the store's current generation, its request owner is current, the ID still exists, and its revision is not older than that item's confirmed revision. Do not automatically replay stale operations.

- [ ] **Step 7: Run frontend contract/store verification**

Run:

```powershell
corepack pnpm@9.15.9 test -- tests/frontend/pluginService.test.ts
corepack pnpm@9.15.9 test -- tests/frontend/pluginStore.test.ts
corepack pnpm@9.15.9 build
```

Expected: focused tests and TypeScript/Vite build pass.

- [ ] **Step 8: Commit Task 6**

```powershell
git add src/types/plugin.ts src/services/pluginService.ts src/stores/plugin.ts tests/frontend/pluginService.test.ts tests/frontend/pluginStore.test.ts tests/frontend/pluginMarketplacePage.test.ts
git commit -m "feat(plugin): reconcile catalog generations in frontend"
```

### Task 7: Source-aware UI, reload health and local-package guide

**Files:**

- Create: `src/components/plugins/pluginPresentation.ts`
- Modify: `src/components/plugins/PluginCard.vue`
- Modify: `src/components/plugins/PluginMarketplacePage.vue`
- Modify: `src/components/plugins/PluginMarketplacePage.css`
- Modify: `tests/frontend/pluginCard.test.ts`
- Modify: `tests/frontend/pluginMarketplacePage.test.ts`
- Create: `docs/plugin-local-manifests.md`

**Interfaces:**

- Consumes: store `catalog`, `localDiscovery`, `reloadStatus`, `reloadError`, `reload()` and `PluginSource`.
- Produces: consistent source disclosure, installed/market/manage source rules, accessible discovery warnings and an exact package authoring guide.

- [ ] **Step 1: Add RED presentation and card tests**

```ts
it('labels a local declarative package without implying execution or verified publishing', () => {
  const wrapper = mount(PluginCard, { props: { plugin: localPlugin, pending: false, error: null } })
  expect(wrapper.text()).toContain('本地声明式包 · 已发现，未执行')
  expect(wrapper.text()).toContain('启用仅记录宿主偏好，不会运行插件代码')
  expect(wrapper.text()).not.toContain('已认证')
})
```

Assert built-in still renders `内置 · 随应用提供` and existing keyboard switch/status associations remain exact.

- [ ] **Step 2: Add RED page membership, reload and accessibility tests**

```ts
it('keeps local packages out of market while showing them in installed and manage', async () => {
  const installed = mountPage('installed', [builtInPlugin, localPlugin])
  expect(installed.text()).toContain(localPlugin.manifest.name)

  const market = mountPage('market', [builtInPlugin, localPlugin])
  expect(market.text()).not.toContain(localPlugin.manifest.name)

  const manage = mountPage('manage', [builtInPlugin, localPlugin])
  expect(manage.text()).toContain(localPlugin.manifest.name)
})
```

Add assertions that reload calls `store.reload`, disables while initial load/reload is active, degraded uses `role="status"` with only aggregate count, unavailable uses `role="alert"`, and no path/raw OS error/install/download/update/uninstall control appears.

- [ ] **Step 3: Run component tests and confirm RED**

Run:

```powershell
corepack pnpm@9.15.9 test -- tests/frontend/pluginCard.test.ts
corepack pnpm@9.15.9 test -- tests/frontend/pluginMarketplacePage.test.ts
```

Expected: tests fail because current UI hard-codes the built-in source and has no healthy reload action.

- [ ] **Step 4: Implement centralized copy and the three-view behavior**

Create:

```ts
export const pluginSourceLabels: Record<PluginSource, string> = {
  builtIn: '内置 · 随应用提供',
  localDeclarative: '本地声明式包 · 已发现，未执行',
}

export function pluginSourceLabel(source: PluginSource): string {
  return pluginSourceLabels[source]
}
```

Use the helper in card, market and management rows. Installed/manage render both sources; market filters `builtIn`. Put “重新扫描本地插件” in the common page header, disabled during initial load or reload. Render degraded/unavailable summaries from validated store data and preserve confirmed catalog on transport failures.

- [ ] **Step 5: Write the exact local package guide**

Document the platform config-root convention, opaque slot rule, exact one-file shape, manifest v1 JSON example, all byte/count limits, explicit reload action, duplicate/collision behavior and this warning:

```text
本地声明式插件只会被读取为元数据。当前版本不会执行插件代码、贡献、脚本或资源；“启用”只记录与该清单内容身份绑定的宿主偏好。
```

Do not document an install command, executable entrypoint or arbitrary path override.

- [ ] **Step 6: Run UI, lint and build verification**

Run:

```powershell
corepack pnpm@9.15.9 test -- tests/frontend/pluginCard.test.ts
corepack pnpm@9.15.9 test -- tests/frontend/pluginMarketplacePage.test.ts
corepack pnpm@9.15.9 lint
corepack pnpm@9.15.9 build
```

Expected: component tests and build pass; lint exits 0 with no new warning category in changed files.

- [ ] **Step 7: Commit Task 7**

```powershell
git add src/components/plugins/pluginPresentation.ts src/components/plugins/PluginCard.vue src/components/plugins/PluginMarketplacePage.vue src/components/plugins/PluginMarketplacePage.css tests/frontend/pluginCard.test.ts tests/frontend/pluginMarketplacePage.test.ts docs/plugin-local-manifests.md
git commit -m "feat(plugin): expose local discovery in marketplace UI"
```

## Final Branch Verification

- [ ] Run every Rust target:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
```

- [ ] Run normal Clippy; record existing baseline warnings but require exit 0 and no new warnings in changed plugin files:

```powershell
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
```

- [ ] Run scoped rustfmt on every changed Rust source, then run full fmt to confirm any remaining failures are only the six unchanged baseline files recorded on Phase 0:

```powershell
rustfmt --edition 2021 --check src-tauri/src/plugin/manifest.rs src-tauri/src/plugin/record.rs src-tauri/src/plugin/discovery.rs src-tauri/src/plugin/discovery/safe_fs.rs src-tauri/src/plugin/registry.rs src-tauri/src/plugin/runtime.rs src-tauri/src/commands/plugin.rs src-tauri/src/storage/plugin_state.rs src-tauri/src/state.rs
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
```

- [ ] Run all frontend tests, type/build validation and lint:

```powershell
corepack pnpm@9.15.9 test
corepack pnpm@9.15.9 build
corepack pnpm@9.15.9 lint
```

- [ ] Verify diff hygiene, generated ACL and non-scope exclusions:

```powershell
git diff --check plugin/runtime-foundation...HEAD
git diff --stat plugin/runtime-foundation...HEAD
git status --short --branch
rg -n "reload_plugin_catalog|allow-reload-plugin-catalog" src-tauri/build.rs src-tauri/src/lib.rs src-tauri/capabilities/plugin-runtime.json src-tauri/permissions/autogenerated
rg -n "install_plugin|download_plugin|uninstall_plugin|watch_plugin|eval\(|WebAssembly|Library::new" src-tauri/src/plugin src/services/pluginService.ts src/stores/plugin.ts src/components/plugins
```

Expected: all required tests/builds exit 0; scoped rustfmt passes; the broad fmt command reports only unchanged pre-existing formatting debt if it still exists; status is clean; ACL includes exactly the three fixed plugin commands; non-scope search finds no new execution/install surface.

- [ ] Request an independent spec-compliance review and a separate code-quality/security review. Resolve every Critical or Important finding with a new focused RED test before changing implementation.

- [ ] Commit only any review-driven fixes, rerun the affected focused suites, then rerun all commands above before claiming completion.
