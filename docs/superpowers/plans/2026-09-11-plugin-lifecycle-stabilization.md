# Plugin Lifecycle Stabilization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Correct the two demonstrated CI failures without weakening lifecycle safeguards, then obtain bounded Windows and isolated-UI evidence before advancing the plugin platform.

**Architecture:** Keep the metadata-only runtime and its existing safety barrier unchanged. Align a legacy test with publication-before-mutation semantics and use the established portable FIFO fixture. Investigate the independent Windows I/O failure before changing production behavior.

**Tech Stack:** Rust, Tauri 2, Vue 3, existing Cargo/Vitest tooling and GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-10-local-manifest-removal-implementation-notes.md`, plus the user-approved sequence: fix CI, establish stability and isolated UI evidence, then design the first read-only functional plugin.

## Global Constraints

- Preserve metadata-only manifests; no plugin execution or additional capability grants in this stabilization patch.
- While adopted documents and the public snapshot differ, retain the old complete DTO but reject lifecycle mutations until authoritative scan and publication complete.
- Preserve disabled decisions and existing real-filesystem assertions; do not skip failing tests or remove FIFO coverage on macOS.
- No blind production retries, permission broadening, path-based destructive fallback, or recursive cleanup.
- Never run destructive tests against real user configuration; use only fixture-owned temporary roots.
- Reuse the successful frontend CI. Intermediate checks cover affected modules only; run the final cross-platform gate once per completed corrective batch.
- Keep PR #30 draft while Windows OS 5 or required validation remains unresolved. A later passing test alone is not a root-cause explanation.
- Work in the existing `plugin/local-manifest-removal` worktree. Root checkout changes are unrelated and must be preserved.

## Task 1: Correct the two CI test contracts

**Files:**
- Modify: `src-tauri/src/plugin/runtime/import/tests.rs`
- Modify: `src-tauri/src/storage/safe_plugin_document/tests.rs`
- Reference: `src-tauri/src/plugin/discovery/safe_fs/tests.rs` portable FIFO fixture

**Interfaces:** consume the existing `PluginRuntime::set_enabled`, `reload_catalog`, `PluginCatalogSnapshot::catalog_generation`, and recording persistence; produce no new production API.

- [x] Reproduce `committed_state_errors_preserve_documents_without_promotion_or_publication` using:

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml committed_state_errors_preserve_documents_without_promotion_or_publication --lib
```

Expected RED matches CI run 34392763374: `plugin_catalog_stale` at the old success unwrap. macOS RED is already recorded in the same run: E0425 at `safe_plugin_document/tests.rs:424`, unavailable `rustix::fs::mknodat`.

- [x] Retain every state/package preservation assertion. Replace the stale-generation success assumption with rejection plus no-new-save evidence, followed by explicit successful reload and a new-generation toggle. Keep the original assertion that the independent decision preserves the imported disabled entry:

```rust
let saves_before = fixture.persistence.saves.load(Ordering::SeqCst);
let error = fixture.runtime
    .set_enabled("com.example.builtin", true, &preview.catalog_generation)
    .await.unwrap_err();
assert_eq!(serde_json::to_value(error).unwrap()["code"], "plugin_catalog_stale");
assert_eq!(fixture.persistence.saves.load(Ordering::SeqCst), saves_before);
let refreshed = fixture.runtime.reload_catalog().await.unwrap();
fixture.runtime
    .set_enabled("com.example.builtin", true, &refreshed.catalog_generation)
    .await.unwrap();
```

Use the repository's existing safe error-code accessor if `AppError` is not directly serializable. Verify the existing revision/entry assertions still express preservation, not discarded state.

- [x] Replace the unsupported FIFO creator with the same direct POSIX command pattern already used by discovery tests. Do not invoke a shell or interpolate a command string:

```rust
use std::os::unix::fs::FileTypeExt;
let path = root.path().join(name);
assert!(path.is_absolute());
let output = std::process::Command::new("mkfifo")
    .args(["-m", "600"])
    .arg(&path)
    .output()
    .expect("POSIX mkfifo must be available for Unix security tests");
assert!(output.status.success(), "mkfifo failed: {output:?}");
assert!(fs::symlink_metadata(&path).unwrap().file_type().is_fifo());
```

Retain the persist rejection assertion for every reserved name and both document families.

- [x] Run the import-runtime module once after both corrections and scoped formatting/diff checks. Windows cannot verify the Unix-only branch; Linux/macOS CI must actually compile/run it. Do not install dependencies or rerun the frontend:

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::runtime::import::tests --lib
rustfmt --edition 2021 --config skip_children=true --check src-tauri/src/plugin/runtime/import/tests.rs src-tauri/src/storage/safe_plugin_document/tests.rs
git diff --check
```

- [ ] Commit only these two test files and report exact RED/GREEN commands/output. Review the correction as one scoped task, then push the existing PR branch without force.

## Parallel evidence tracks and final gate

These are diagnostic/verification tracks, not authorization for speculative production edits.

1. Windows: inspect the existing exact-operation diagnostics and held-handle promotion path. After Task 1 releases the Cargo build cache, run the package module once with diagnostics. If a failure occurs, trace the named operation and actual native status. If needed, use one bounded focused reproduction batch (at most five exact-case invocations), stopping at the first diagnostic failure. Do not repeatedly rerun until green. A demonstrated root cause gets a separate minimal test-first correction and scoped review; no reproduced cause means retain the reliability hold.
2. Native UI: inspect config-root, credentials and background startup behavior for a supported disposable profile. Launch only if every write stays in an explicit disposable profile and no real credentials/trading are activated. Otherwise report the precise missing isolation facility; do not guess that changing APPDATA redirects the Windows known-folder API. Browser-only checks must be labeled as such.
3. Final: inspect the corrective delta and CI results; update the existing PR body with actual evidence. Do not merge or start executable/functional extension implementation before the prerequisite stability decision. The first read-only plugin receives its own narrow design/plan after that gate.

## Plan self-review

| Boundary | Check |
| --- | --- |
| Legacy test vs new barrier | Rejection before reload and preservation after reload both remain asserted |
| Unix FIFO fixture vs macOS compilation | Uses existing direct executable pattern; retains actual FIFO identity and rejection |
| CI task vs Windows diagnosis | Disjoint source ownership; Cargo runs serialized |
| Diagnostic evidence vs release claim | Passing rerun cannot clear unresolved OS 5 by itself |
| UI smoke vs real user state | No native launch until isolation is proven |

## 2026-09-11 execution evidence

Task 1 is committed as `7a24f44` (`test(plugin): align lifecycle CI contracts`), changing only the two requested test files. The focused RED failed with `plugin_catalog_stale` at the legacy success unwrap. The corrected import module passed once: 38 passed, 0 failed, 1 intentionally ignored child-process helper. Scoped rustfmt and diff hygiene passed. The 35 existing dead-code warnings were retained; no production code or safety rule changed. Unix-specific compilation and execution still require CI.

The single Windows package-module diagnostic run returned 40 passed / 1 failed. The failing case was `successful_calls_and_failed_precheck_consume_mutation_budget_before_io`; the historical `postrename_reopen_reparses_both_files_and_three_identities` case passed. The first tool response was truncated, losing the failure's panic and exact-operation evidence. One authorized exact-case invocation with a sufficient output budget passed (1 passed / 1101 filtered). No additional module or focused batch was run. This new failure cannot be labeled OS 5 from the retained evidence, and the single passing invocation does not clear either failure.

A narrow read-only audit found that this case's hooks, event list, UUID counter and temporary plugin root are fixture-local; import diagnostics are thread-local. It found no concrete cross-test shared-state explanation. Existing diagnostics also discard original native NTSTATUS when mapping to a Win32 error, and final-child/content labels each cover multiple underlying I/O operations. The historical OS 5 root cause therefore remains unresolved; no retries, permission changes, relaxed checks or speculative production fix were added.

Native whole-app smoke was not launched. Production `AppState::new` selects stores with separate platform config/data roots (`src-tauri/src/state.rs`); native setup starts the scheduler unconditionally (`src-tauri/src/lib.rs`). Credentials use the fixed system keyring service, while scheduler/bootstrap and `src/App.vue` can initiate network work and account auto-connection. There is no supported disposable-profile entrypoint. Merely redirecting `APPDATA` is not a proven isolation contract.

The smallest prerequisite for future native smoke is an explicit profile selected before `AppState` construction, atomically providing disposable roots for all stores, a non-system empty credential repository, and disabled scheduler/bootstrap, auto-connect and provider networking. It must fail closed if any isolation element is missing. This profile is not implemented by Task 1 and needs its own narrow design. PR #30 remains draft; the first functional plugin remains deferred until the stability decision.
