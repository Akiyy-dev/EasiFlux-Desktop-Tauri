# Plugin Runtime Foundation Implementation Plan

> **Execution:** Use `superpowers:subagent-driven-development` in this session. Every production behavior follows strict RED → GREEN → REFACTOR; each task is committed and reviewed before the next task starts.

**Goal:** Deliver Phase 0 of the EasiFlux application-plugin system: a strict built-in catalog, crash-isolated enable-state persistence, narrowly authorized Tauri commands, and a complete read-only-marketplace/enable-management UI. Production may contain zero plugins and must present that state honestly.

**Architecture:** Rust owns identity validation, catalog trust, activation eligibility, revisioned state and persistence. Vue treats every IPC response as untrusted, keeps request ownership in one Pinia store, and renders only host-produced data. No manifest or UI path executes plugin code. The existing `Plugin` trait lifecycle is removed.

**Tech stack:** Rust, Serde/serde_json, semver, Tokio `RwLock`, Tauri 2 ACL, Vue 3, Pinia 3, TypeScript, Vitest, Vue Test Utils.

**Spec:** `docs/superpowers/specs/2026-09-07-plugin-system-design.md`

## Global Constraints

- Phase 0 accepts only compile-time built-in manifests; `builtin_manifests()` may and initially will return an empty vector.
- Delete the existing executable `Plugin`/`PluginContext`/`Box<dyn Plugin>`/`on_init` skeleton. Enabling a plugin never invokes user-supplied code or a Rust trait lifecycle.
- Do not add local directory scanning, installation, downloading, network calls, dynamic libraries, WASM, JavaScript execution, arbitrary HTML, file paths, URLs or dynamic Tauri command names.
- Manifest schema version is exactly `1`. IDs are lowercase ASCII reverse-domain names, maximum 128 bytes, at least two segments, each maximum 63 bytes and bounded by an alphanumeric character.
- Phase 0 manifest `contributions` and `requestedCapabilities` must be empty. `grantedCapabilities` is also empty and comes from the host, never from the manifest.
- Plugin state is stored separately at the application config directory's `plugins/state.json`; maximum file size is 256 KiB and maximum entry count is 512.
- A saved decision is bound to `id + source + publisherId + approvalFingerprint`. Phase 0 uses source `builtIn` and fingerprint `v1:none`. Duplicate identities are rejected.
- State writes use same-directory temp/backup files, file and parent-directory sync, backup restoration on promotion failure, and commit memory only after disk succeeds.
- Corrupt/unreadable state or invalid built-in catalog never prevents `AppState` startup. The plugin snapshot becomes `unavailable`, items become non-toggleable/blocked, and UI-visible errors contain stable codes/messages but no paths, JSON, OS details or stack traces.
- Snapshot and mutation revisions are decimal strings across IPC. Revision changes only after a successful non-idempotent state write. `set_plugin_enabled` returns both the item and the new revision.
- The two application commands are fixed and ACL-restricted to the local `main` WebView. Do not grant them to remote URLs, wildcard windows or new WebViews.
- Frontend parsers reject unknown object keys, wrong schema versions, non-string revisions, non-empty Phase 0 contribution/capability arrays and unknown enum values.
- A marketplace section may browse the trusted built-in catalog only. It must not expose install/download/update controls or imply that external packages already work.
- Preserve the existing Settings → Plugins placeholder; the operable plugin surface is the top-level Plugins page, so there is one state owner.
- Use existing CSS variables/components and Chinese user-facing copy. Status must be expressed in text, not color alone.
- No task may modify the dirty primary checkout. All implementation occurs in the isolated `plugin/runtime-foundation` worktree branch.

## Task 1: Strict plugin manifest and transport model

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify if Cargo resolver requires it: `src-tauri/Cargo.lock`
- Replace: `src-tauri/src/plugin/mod.rs`
- Create: `src-tauri/src/plugin/manifest.rs`
- Create: `src-tauri/src/plugin/builtin.rs`
- Modify: `src-tauri/src/error.rs`

### Step 1: Write manifest and error-contract tests

In `plugin/manifest.rs`, write tests first for:

- valid IDs: `com.easiflux.analytics` and a legal 63-byte segment;
- invalid IDs: uppercase, one segment, empty/consecutive segments, leading/trailing hyphen, non-ASCII, over-63 segment and over-128 total;
- `PluginPublisherId` follows the same lowercase reverse-domain and 128/63-byte boundaries;
- a complete v1 manifest with `publisherId`, display publisher, valid SemVer and empty reserved arrays;
- wrong schema, blank/over-limit display text, invalid SemVer, non-empty contributions, non-empty requested capabilities and unknown JSON fields;
- exact camelCase serialization of a catalog snapshot and mutation result, including string revision, availability, source, status and reason codes;
- `builtin_manifests()` is valid, unique and deterministically ordered even when empty.

In `error.rs`, test a new structured plugin error shape `{ "code": "plugin_not_found", "message": "插件不存在" }` and prove private diagnostic text is absent from `Display`, `user_message` and serialized output.

Before writing production types, run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin::manifest --locked
cargo test --manifest-path src-tauri/Cargo.toml error::tests --locked
```

Record the expected compile/test failures in the task report.

### Step 2: Implement the smallest strict model

Add direct dependency `semver = "1"`. Implement:

- private-inner `PluginId` and `PluginPublisherId` with `parse`, `as_str`, `Display`, validated `Deserialize`, stable `Ord`/`Hash`;
- `PluginManifestV1` with `#[serde(rename_all = "camelCase", deny_unknown_fields)]`;
- reserved `contributions: Vec<serde_json::Value>` and `requested_capabilities: Vec<String>` that validation requires to be empty;
- `PluginSource::BuiltIn`, `PluginStatus::{Enabled, Disabled, Blocked}`, `PluginAvailability::{Available, Unavailable}`, and `PluginAvailabilityReason::{StateUnavailable, CatalogInvalid}`;
- `PluginCatalogItem`, `PluginCatalogSnapshot` and `PluginCatalogMutationResult`, all strict camelCase DTOs; constructors stringify `u64` revisions;
- `APPROVAL_FINGERPRINT_NONE = "v1:none"`;
- `builtin_manifests() -> Vec<PluginManifestV1>` returning `vec![]`;
- `AppError::Plugin { code: &'static str, message: &'static str, diagnostic: Option<String> }`, with only code/message serialized or displayed. Internal diagnostic may be logged but never serialized.

Use explicit length constants: name 80, description 500, publisher display 80, version 64. Reject strings whose trimmed value is empty, but do not silently rewrite signed/serialized input.

### Step 3: Verify and commit

Run the two focused test commands again, then:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml plugin::manifest --locked
cargo test --manifest-path src-tauri/Cargo.toml error::tests --locked
```

Commit: `feat(plugin): define strict manifest contract`

## Task 2: Bounded atomic plugin-state persistence

**Files:**

- Create: `src-tauri/src/storage/atomic_file.rs`
- Create: `src-tauri/src/storage/plugin_state.rs`
- Create: `src-tauri/src/storage/plugin_state/tests.rs`
- Modify: `src-tauri/src/storage/mod.rs`

### Step 1: Write storage behavior tests

Write tests for these observable behaviors before implementation:

- missing main/temp/backup yields an empty version-1 state;
- complete state round-trips with revision serialized as a string and a bound trust identity;
- main, then temp, then backup is selected only if the preceding candidate is missing or invalid;
- an unknown future-schema main file is terminal unavailable: it cannot fall back to or be overwritten by an older temp/backup;
- schema mismatch, unknown fields, duplicate entries, invalid IDs, wrong source/fingerprint, non-string/overflow revision, non-boolean enabled, more than 512 entries and input over 256 KiB are rejected;
- save creates parents, writes same-directory sidecars, syncs, preserves a backup and can read the new main;
- injected temp-write, backup-rotation, promotion and restore failures return a sanitized plugin error and never make an uncommitted state observable as current;
- persistence is `Send + Sync` and concurrent saves are serialized.

Run and capture RED:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml storage::plugin_state --locked
```

### Step 2: Implement state structures and atomic file primitive

Implement strict DTOs:

```rust
PluginStateFileV1 { schema_version: u32, revision: u64, entries: Vec<PluginStateEntryV1> }
PluginStateEntryV1 { id, source, publisher_id, approval_fingerprint, enabled }
```

Use a custom decimal-string serde adapter for revision. Validate size before UTF-8/JSON parsing and validate every entry after parsing. A vector plus a `BTreeSet` uniqueness pass must reject duplicate composite identities rather than applying last-wins behavior.

`atomic_file.rs` contains only model-independent path/candidate/write primitives. `PluginStateStore` owns a `std::sync::Mutex<()>` shared by the full load and save transactions and implements:

```rust
pub(crate) trait PluginStatePersistence: Send + Sync {
    fn load(&self) -> AppResult<PluginStateFileV1>;
    fn save(&self, state: &PluginStateFileV1) -> AppResult<()>;
}
```

`PluginStateStore::try_new()` resolves `dirs::config_dir()/APP_NAME/plugins/state.json` and returns a sanitized error when no config directory is available; tests use an explicit path constructor. Phase 0 state schema accepts only source `builtIn` and fingerprint `v1:none`; later sources require a new schema and migration. Log raw I/O detail with `tracing`, but return only structured sanitized plugin errors.

Wire `#[cfg(test)] mod tests;` from `plugin_state.rs`. A future-schema main candidate must stop recovery and saving; malformed current-schema data may fall back to a valid temp or backup. Platform parent-directory sync follows the existing notification-store pattern (real directory sync on Unix, documented no-op where Rust cannot open a directory for syncing).

### Step 3: Verify and commit

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml storage::plugin_state --locked
```

Commit: `feat(plugin): persist bounded enable state`

## Task 3: Registry state machine and failure isolation

**Files:**

- Create: `src-tauri/src/plugin/registry.rs`
- Create: `src-tauri/src/plugin/registry/tests.rs`
- Modify: `src-tauri/src/plugin/mod.rs`

### Step 1: Write registry behavior tests

Use an in-memory persistence fake with recorded saves. Tests must fail first for:

- an empty built-in catalog is available and returns revision `0`;
- items are deterministically ordered by ID and default disabled;
- invalid or duplicate built-ins produce an unavailable `catalogInvalid` snapshot without panicking;
- corrupt/unavailable state retains valid catalog metadata but reports `stateUnavailable` and blocked/non-toggleable items;
- a later successful reload recovers an unavailable state;
- only an exact `id + builtIn + publisherId + v1:none` state identity restores enabled; mismatched or unknown bounded entries remain stored but do not activate an item;
- invalid/unknown IDs and unavailable/non-toggleable items return the correct structured error;
- idempotent updates do not save and do not increment revision;
- successful updates save revision +1 and return `{ plugin, revision }`;
- failed saves preserve item status, revision and the previous persisted value;
- sequential changes to two IDs retain both values.

Run and capture RED:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml plugin::registry --locked
```

### Step 2: Implement the registry

`PluginRegistry` owns a `BTreeMap<PluginId, PluginManifestV1>` and a runtime enum:

```rust
Available { state, persistence }
Unavailable { reason, persistence }
```

`initialize(builtins, persistence)` never returns an error to application startup. It validates all built-ins as one trusted catalog, rejects all catalog contents on any invalid/duplicate item, then attempts state load. `retry_state_load()` only retries when the catalog itself is valid and state is unavailable.

`catalog_snapshot()` derives items from manifest plus exact trust-identity matches. `set_enabled()` performs parse → lookup → availability/eligibility → idempotence → clone next state → persist → memory commit. It returns `PluginCatalogMutationResult` and never holds an async suspension point.

The command layer must call this synchronous method while holding one `state.plugins.write().await` guard for the complete clone → persist → commit transaction. `retry_state_load()` is also called only under that write guard; never mutate the registry through a read guard.

Do not restore the old executable trait or add lifecycle hooks.

### Step 3: Verify and commit

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml plugin::registry --locked
```

Commit: `feat(plugin): add isolated registry runtime`

## Task 4: Tauri commands, startup wiring and command ACL

**Files:**

- Create: `src-tauri/src/commands/plugin.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/capabilities/default.json`

### Step 1: Write wiring and authority tests

Before production wiring, add focused tests that prove:

- catalog command retries an unavailable state then returns a snapshot;
- mutation command delegates to the write-locked registry and returns revision plus item;
- startup helper returns a registry even when persistence construction or load fails; absence/inaccessibility of the config directory must not panic or escape `AppState::new`, and a later retry reconstructs the store before reloading;
- local `main` resolves access for `get_plugin_catalog` and `set_plugin_enabled`;
- an unrelated window and a remote origin do not resolve access to either plugin command.

Prefer testable inner functions accepting `&RwLock<PluginRegistry>`; keep `tauri::State` wrappers trivial. Run and capture RED:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml commands::plugin --locked
cargo test --manifest-path src-tauri/Cargo.toml capability_tests --locked
```

### Step 2: Wire only the fixed surface

- Add `get_plugin_catalog` and `set_plugin_enabled` to `generate_handler!`.
- Initialize `PluginRegistry` from `builtin_manifests()` and `PluginStateStore::try_new()` without propagating plugin errors from `AppState::new`. Preserve enough unavailable state for the catalog command to reconstruct the store and retry path resolution later.
- Replace the simple build call with `tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&["get_plugin_catalog", "set_plugin_enabled"])))` and a build-only failure message. The locked Tauri builder auto-generates `allow-get-plugin-catalog` and `allow-set-plugin-enabled`; do not create a redundant custom permission file.
- Grant the generated identifiers `allow-get-plugin-catalog` and `allow-set-plugin-enabled` only to the existing capability whose exact window is `main`, and keep remote URLs absent.
- Do not change framework plugin dependencies or grant filesystem/network/shell permissions.

### Step 3: Verify generated authority and commit

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml commands::plugin --locked
cargo test --manifest-path src-tauri/Cargo.toml capability_tests --locked
cargo test --manifest-path src-tauri/Cargo.toml state::tests --locked
```

Commit: `feat(plugin): expose constrained host commands`

## Task 5: Strict frontend service and concurrency-safe store

**Files:**

- Create: `src/types/plugin.ts`
- Create: `src/services/pluginService.ts`
- Create: `src/stores/plugin.ts`
- Create: `tests/frontend/pluginService.test.ts`
- Create: `tests/frontend/pluginStore.test.ts`

### Step 1: Write service RED tests

Test the exact calls:

```ts
getPluginCatalog() -> tauriInvoke('get_plugin_catalog')
setPluginEnabled(id, enabled) -> tauriInvoke('set_plugin_enabled', { id, enabled })
```

Accept one complete valid response. Reject unknown keys at snapshot/item/manifest/mutation levels, wrong schema/source/status/availability/reason, missing nullable fields, numeric or non-canonical revision, non-empty Phase 0 arrays and malformed nested values. Canonical revision is `0` or a non-zero decimal without sign/leading zero and must not exceed `u64::MAX`; validate with string/`BigInt`, never `Number`. Validate plugin and publisher IDs with the same reverse-domain contract. Enforce cross-field consistency: available has null reason, unavailable has a reason, blocked is non-toggleable with a reason, and enabled/disabled are toggleable with null reason.

Decode only exact structured errors `{ code, message }`. Known plugin codes map to frontend-controlled Chinese copy. Extra fields, unknown codes, wrong field types and raw Error/path/JSON/stack details all fall back to a stable generic message without reflecting private input.

Run and capture RED:

```powershell
.\node_modules\.bin\vitest.cmd run tests/frontend/pluginService.test.ts
```

### Step 2: Implement strict types and service parser

Define the DTOs from the spec and runtime validators that compare exact key sets. Invoke Tauri with `unknown`, validate, then return typed values. Use stable local fallback messages for malformed responses and unrecognized errors.

### Step 3: Write store RED tests

With real Pinia and a mocked external service boundary, test:

- concurrent `load()` calls share one flight;
- after a ready result, a later ordinary `load()` is a no-op; only `retry()` forces a new request;
- initial success/failure, retry recovery and refresh failure preserving confirmed data;
- case-insensitive query across name/id/publisher/description and intersection with all/enabled/disabled/blocked filter without mutating catalog order;
- pending state and errors are per ID;
- successful mutation replaces only the target item and adopts returned revision;
- failure preserves confirmed state;
- a second request for the same ID owns the result so late first success/failure cannot overwrite data, error or pending state;
- requests for different IDs remain independent;
- unknown ID leaves state unchanged.

Run and capture RED:

```powershell
.\node_modules\.bin\vitest.cmd run tests/frontend/pluginStore.test.ts
```

### Step 4: Implement the Pinia store

Expose catalog, load status/error, query, status filter, immutable `visiblePlugins`, replaced `Set` pending IDs, per-ID action errors, `load`, `retry`, `setQuery`, `setStatusFilter` and `setEnabled`. Keep request sequence ownership outside reactive state and never optimistically mutate status.

### Step 5: Verify and commit

```powershell
.\node_modules\.bin\vitest.cmd run tests/frontend/pluginService.test.ts tests/frontend/pluginStore.test.ts
.\node_modules\.bin\vue-tsc.cmd --noEmit
```

Commit: `feat(plugin): add typed frontend state`

## Task 6: Plugin marketplace page and AppShell integration

**Files:**

- Create: `src/components/plugins/PluginCard.vue`
- Create: `src/components/plugins/PluginMarketplacePage.vue`
- Create: `src/components/plugins/PluginMarketplacePage.css`
- Modify: `src/components/layout/AppShell.vue`
- Create: `tests/frontend/pluginCard.test.ts`
- Create: `tests/frontend/pluginMarketplacePage.test.ts`
- Modify: `tests/frontend/settingsCenter.test.ts`

### Step 1: Write card RED tests

Test real rendered behavior:

- name, version, description, publisher, source, status and permission summary render as text; an HTML-shaped name never creates an element;
- enabled/disabled/blocked have visible labels;
- switch `checked` state and accessible name match the plugin/current state; `aria-describedby` reaches only status/error elements that actually exist;
- blocked or `canToggle=false` is disabled with reason; pending disables only this card and sets busy semantics;
- enabled switch emits `{ id, enabled: false }`, disabled emits true, and non-toggleable emits nothing;
- per-card error uses `role="alert"`.

Run and capture RED:

```powershell
.\node_modules\.bin\vitest.cmd run tests/frontend/pluginCard.test.ts
```

### Step 2: Implement `PluginCard`

Use props `{ plugin, pending, error }` and emit `toggle(id, enabled)`. Use a native checkbox with `role="switch"`; do not import the store. Render strings only through Vue interpolation and show requested/granted capability summaries independently, which both read “无需额外权限” in Phase 0.

### Step 3: Write page and Shell RED tests

Test:

- narrow prop `section: PluginSection`; first mount loads once and `installed → market → installed` keeps query/filter and does not reload;
- focusable `h1` receives focus once on entry; mount the focus test into `document.body` and await `nextTick`;
- loading `role=status`, first-load `role=alert` + retry, refresh error retains cards;
- installed view search/filter, distinct empty-catalog and no-match states;
- market shows only trusted built-in catalog, “随应用提供”, and contains no install/download/update button;
- manage uses the same catalog to count enabled/disabled/blocked and list requested/granted permissions;
- card toggle delegates to store without local optimistic mutation;
- AppShell replaces the placeholder and forwards installed/market/manage;
- Settings Center test stubs the new page so unrelated navigation tests never call real IPC; Settings → Plugins remains the existing placeholder.

Run and capture RED:

```powershell
.\node_modules\.bin\vitest.cmd run tests/frontend/pluginMarketplacePage.test.ts tests/frontend/settingsCenter.test.ts
```

### Step 4: Implement page and integration

Mount one page component for all three existing sidebar sections. Call `store.load()` from `onMounted`, focus the title after `nextTick`, and keep query/filter in the store. Use the existing AppShell navigation state only as a section prop. Reuse existing design tokens and responsive patterns; do not add a second settings store or install workflow.

### Step 5: Verify and commit

```powershell
.\node_modules\.bin\vitest.cmd run tests/frontend/pluginCard.test.ts tests/frontend/pluginMarketplacePage.test.ts tests/frontend/settingsCenter.test.ts
.\node_modules\.bin\vue-tsc.cmd --noEmit
.\node_modules\.bin\eslint.cmd src
```

Commit: `feat(plugin): build marketplace management UI`

## Final branch verification

After all task reviews are clean, run from the worktree root:

```powershell
.\node_modules\.bin\vitest.cmd run
.\node_modules\.bin\vue-tsc.cmd --noEmit
.\node_modules\.bin\vite.cmd build
.\node_modules\.bin\eslint.cmd src
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked --target-dir D:\EasiFlux\EasiFlux-Desktop-Tauri\target\plugin-system-cargo
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets --target-dir D:\EasiFlux\EasiFlux-Desktop-Tauri\target\plugin-system-cargo -- -D warnings
```

If clippy fails only on warnings proven present at `main@b628874`, record the exact baseline evidence and ensure no new warning appears in changed files. Do not weaken lint levels or edit unrelated code to hide baseline debt.
