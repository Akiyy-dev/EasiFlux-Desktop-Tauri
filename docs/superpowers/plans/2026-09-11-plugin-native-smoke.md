# Isolated Native Plugin Smoke Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an opt-in Windows native host that exercises real plugin UI/IPC/storage without initializing live trading state.

**Architecture:** Extract one shared plugin command state, compose an explicit fresh fixture-root runtime, and host the existing Marketplace in a separate feature-gated binary and frontend entry. The automatic lane substitutes only fixture selection, not service, IPC, runtime or persistence.

**Tech Stack:** Existing Rust/Tauri 2, Vue, Pinia, Vite, Cargo and Vitest. No new dependency installation.

**Spec:** `docs/superpowers/specs/2026-09-11-plugin-native-smoke-design.md`.

## Global Constraints

- Metadata-only; no plugin execution or capability expansion in production.
- Default production binary, scheduler/startup and plugin command names/payloads remain unchanged.
- Never construct production `AppState`, platform-default plugin stores, account/keyring/provider services or the opener plugin in the smoke runner.
- Require an absolute existing `--parent` canonically within the build checkout's `target` (parent of `CARGO_MANIFEST_DIR` plus `target`); reject an artifact-root symlink resolving outside the canonical checkout. Reject out-of-workspace parents before reserving a child. Create one fresh exclusive `plugin-smoke-<UUID>` child. No existing-profile reuse, platform-root fallback, overwrite, creation retry or recursive cleanup.
- All application-owned plugin state, source, ownership, package/staging, WebView2 user data and report files stay beneath that child. Retain it on exit.
- Only the canonical fixture source may be read; automatic selection returns that source. Other selected files reject before reading contents.
- Reuse the exact seven production command wrappers and existing Marketplace/store/service. Do not mock IPC or persistence.
- Smoke feature is opt-in, disabled by default; runner refuses release/non-Windows launch before writes.
- Native window/webview is local `main`; explicit capabilities are existing `plugin-runtime` and the harness-only completion permission. Absolute WebView2 data_directory is set before creation.
- Before profile writes, reject WebView2 environment override key presence (case-insensitive WEBVIEW2_/COREWEBVIEW2_) and existing/inaccessible WebView2 policy roots in HKCU/HKLM both views. Presence-only, read-only checks; no value inspection/logging or environment/registry mutation.
- Native self-test remains bounded to 90 seconds, with condition waits rather than mutation retries. Only first completion can write the fixed report.
- Automatic native picker, whole-app startup/scheduler/trading/shell, installers and restart recovery are not claimed covered.
- Serialize Cargo runs and implementation agents. Intermediate checks are focused; controller authorizes one native self-test only after isolation review.
- Keep historical Windows failure unresolved unless actual causal evidence is obtained; green smoke is not its fix.

## Task 1: Shared command state and isolated fixture runtime

**Files:**
- Modify: `src-tauri/Cargo.toml` (declare opt-in feature only; no dependencies)
- Modify: `src-tauri/src/commands/plugin.rs`
- Modify: `src-tauri/src/lib.rs` (production manage call only)
- Modify: `src-tauri/src/plugin/mod.rs`
- Create: `src-tauri/src/plugin/smoke.rs`

**Interfaces:**
- `PluginCommandState::new(runtime: Arc<PluginRuntime>) -> Self` for production.
- `PluginCommandState::with_selector(runtime: Arc<PluginRuntime>, selector: Arc<dyn LocalManifestSelector>) -> Self`, available only for tests/smoke.
- `PluginSmokeProfile::create(parent: &Path) -> Result<Self, String>`; fields `root: PathBuf`, `source: PathBuf` are crate-visible, while its one shared runtime remains internal.
- `PluginSmokeProfile::runtime(&self) -> Arc<PluginRuntime>` clones that runtime; `selector(&self) -> Arc<dyn LocalManifestSelector>` returns fixture selection.
- `PluginSmokeProfile::inspect_final_state(&self) -> Result<SmokeNativeChecks, String>` produces serializable booleans `source_unchanged`, `disabled_decision_retained`, `ownership_empty`, `local_empty`, `staging_empty`.
- `SmokeNativeChecks::passed(&self) -> bool` is the conjunction of all five checks, consumed by Task 2.

- [ ] Add focused tests before implementation; no fallback to live stores even in RED. A constructor stub may safely return an error to allow compiling RED. Required cases: relative/missing/non-directory/out-of-workspace parent rejects before creating a child; two fresh workspace profiles are distinct; a real import/remove cycle in one profile preserves source bytes and disabled preference while the other profile stays empty. Use a fixture-owned parent beneath the build checkout's target, not the system temporary root, for successful lifecycle cases.

Use real runtime calls and typed/serialized results, not mock persistence:

```rust
let artifact_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("target");
std::fs::create_dir_all(&artifact_root).unwrap();
let parent = tempfile::tempdir_in(artifact_root).unwrap();
let profile = PluginSmokeProfile::create(parent.path()).unwrap();
let other = PluginSmokeProfile::create(parent.path()).unwrap();
let runtime = profile.runtime();
runtime.get_catalog().await.unwrap();
let ready = serde_json::to_value(runtime.prepare_import(profile.selector()).await.unwrap()).unwrap();
assert_eq!(ready["status"], "ready");
let result = runtime.commit_import(
    ready["token"].as_str().unwrap(),
    ready["catalogGeneration"].as_str().unwrap(),
).await.unwrap();
assert_eq!(serde_json::to_value(result).unwrap()["status"], "imported");
let catalog = runtime.get_catalog().await.unwrap();
let removed = runtime.remove_managed_local_plugin("com.easiflux.smoke", &catalog.catalog_generation).await.unwrap();
assert_eq!(serde_json::to_value(removed).unwrap()["status"], "removed");
assert!(profile.inspect_final_state().unwrap().passed());
assert!(!other.runtime().get_catalog().await.unwrap().plugins.iter()
    .any(|item| item.manifest.id.as_str() == "com.easiflux.smoke"));
```

Confirm exact existing serialization field names against `src-tauri/src/plugin/import.rs` before running; the test must exercise the production contract rather than change it to fit this example. Tests must also reject a non-fixture source before reading it; use an independent sentinel file in the fixture-owned parent, never real user data.

- [ ] Run RED: `cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::smoke::tests --lib` from the worktree root, using the shared target-dir provided by the controller. Capture full failure output to a log if needed.
- [ ] Add `plugin-smoke = []` feature and gate `plugin::smoke` with `cfg(any(test, feature = "plugin-smoke"))`. Do not alter lockfile dependencies.
- [ ] Implement the shared state and change the seven wrappers to `State<PluginCommandState>`, preserving the existing helper calls. Only prepare consults the optional selector; default construction still creates `NativeLocalManifestSelector` for the received window. Production setup manages `PluginCommandState::new(Arc::clone(&state.plugins))` before exposing commands, alongside the same `AppState`.
- [ ] Implement fresh root creation, canonical fixture-only reader, one runtime composition and native final checks. Use real `builtin_manifests`, `PluginStateStore::with_path`, `ManagedOwnershipStore::with_plugins_root`, `SystemLocalPluginPackageStorage::with_plugins_root` for both import/removal, and `discover_from_plugins_root` through a fixed-root discovery adapter. There is no system-default fallback branch.

Fixture JSON is literal:

```json
{"schemaVersion":1,"id":"com.easiflux.smoke","name":"Plugin Smoke Fixture","version":"1.0.0","description":"Isolated metadata-only fixture","publisherId":"com.easiflux","publisher":"EasiFlux smoke test","contributions":[],"requestedCapabilities":[]}
```

- [ ] Run focused GREEN once, then existing `commands::plugin::tests` and `capability_tests` filters once each. Do not run the full Rust/frontend suites. Run scoped rustfmt (`skip_children=true`) and diff checks.
- [ ] Self-review and commit only the listed files; report exact RED/GREEN commands/results and any warnings. The task report must identify every explicit root binding and confirm production behavior is unchanged. Task 2 starts only after this task's review.

## Task 2: Opt-in native host, real UI smoke and bounded report

**Files:**
- Modify: `src-tauri/Cargo.toml` (default-run, required-feature binary and existing windows-sys Win32_System_Registry feature for read-only preflight; no new crate)
- Modify: `src-tauri/src/lib.rs` (feature-gated runner export only)
- Modify: `src-tauri/build.rs` (completion command manifest only when feature enabled; reject inherited smoke config overrides)
- Create: `src-tauri/permissions/autogenerated/finish_plugin_smoke.toml` if required by Tauri generation (definition only; default runtime grants none)
- Create: `src-tauri/src/bin/plugin_smoke.rs`
- Create: `src-tauri/src/plugin_smoke.rs`
- Create: `src-tauri/plugin-smoke/tauri.conf.json`
- Create: `plugin-smoke.html`
- Create: `vite.plugin-smoke.config.ts`
- Create: `src/plugin-smoke/main.ts`
- Create: `src/plugin-smoke/PluginSmokeApp.vue`
- Create: `src/plugin-smoke/selfTest.ts`
- Create: `tests/frontend/pluginSmokeSelfTest.test.ts`
- Create: `docs/plugin-native-smoke.md`
- Modify: `.gitignore` only if a new frontend build-artifact directory needs an explicit ignore.

**Interfaces:** consumes Task 1's command state/profile/checks. Exposes library `run_plugin_smoke() -> Result<(), String>` only with `plugin-smoke`, called only by the dedicated binary. Harness command `finish_plugin_smoke(success: bool, detail: String)` is registered only in this runner and its permission is local-main-only; detail limit is 2,000 UTF-8 bytes. The frontend's exported `runPluginSmokeSelfTest()` drives real DOM controls and reports once through this command.

- [ ] Add focused tests for the runner's argument parsing and completion decision before implementation. Unknown/missing/relative parent args reject; `--self-test` is explicit. Literal verdict cases must include UI failure with all native checks true, UI success with one native check false, and both true. Test first-decision ownership and duplicate rejection without a WebView or actual process exit. Test timeout/failure report handling against a new fixture-owned root, not production data.

The success contract is:

```rust
let passed = ui_success && native_checks.passed();
let exit_code = if passed { 0 } else { 1 };
```

Do not compute expected test values by calling `passed()` itself. Independently assert false when each required check is false. The completion code must actually use the tested decision and must write only `profile.root.join("report.json")`.

- [ ] Run focused RED with `cargo test --locked --manifest-path src-tauri/Cargo.toml --features plugin-smoke plugin_smoke::tests --lib`. No GUI launch yet.
- [ ] Add a `[[bin]]` named `plugin-smoke`, path `src/bin/plugin_smoke.rs`, `required-features = ["plugin-smoke"]`, and preserve normal binary selection with `default-run = "easiflux-desktop"`. Keep feature disabled by default. Refuse release/non-Windows runner execution before profile creation.
- [ ] Implement the runner: parse args, create profile, configure tracing only to process output if needed, build the alternate Tauri context, register only the seven existing plugin commands plus completion, manage profile/state, and create the `main` WebView with its absolute `webview2` directory. Automatic lane uses the profile selector and a hidden window; manual lane keeps native selection and visible fixture instructions. Never call the normal `run()` or `AppState::new()`.
- [ ] Before profile creation, reject case-insensitive WEBVIEW2_/COREWEBVIEW2_ environment key presence and any existing policy root at Software/Policies/Microsoft/Edge/WebView2 under HKCU/HKLM in both registry views. RegOpenKeyExW/RegCloseKey must only probe key presence; allow only missing-key/path statuses and reject all other errors. Do not inspect values, enumerate configuration contents or change any registry/environment state. Cover name/status decisions with pure test inputs; no real policy mutations in tests. Document that managed-policy machines are conservatively unsupported.
- [ ] Configure `app.windows` as empty and `app.security.capabilities` explicitly as `plugin-runtime` plus an inline local-main-only completion capability. Use the dedicated config directory's canonical `tauri.conf.json`, not an alternate filename next to production config. Reject inherited `TAURI_CONFIG` in smoke-feature builds, without reading/logging its value or changing caller environment. Add completion to the application command manifest only for the smoke feature. Keep existing production capability files unchanged. A generated permission definition is not an active grant: default runtime must resolve no completion access and register no completion handler. Use dev URL `http://127.0.0.1:1430`, local-only navigation and CSP supporting only same-origin assets, IPC and local HMR. Do not enable the custom-protocol feature or require a production frontend build for this debug harness. Tests must inspect the actual generated context identifier, dev URL, empty automatic windows and authority.
- [ ] Add the separate HTML/Vue+Pinia bootstrap and a Vite config rooted at this worktree, `host: "127.0.0.1"`, `port: 1430`, `strictPort: true`. Import existing global CSS and real Marketplace; no import of `App.vue`, `AppShell` or account/market stores. Minimal section controls supply `installed`, `market`, `manage` props.
- [ ] Implement automatic real-DOM flow from the spec using existing `data-testid` selectors. For fixture item/card selection, use `data-plugin-id` if available; otherwise scope by the existing displayed fixture ID/name, without changing production component behavior. Require enabled controls and verify states/results between mutations; do not invoke another lifecycle mutation while waiting. Each condition has a 15-second maximum within the host's 90-second global deadline. Report any thrown error once as failure. The selected source is fixed host-side, never supplied by frontend IPC.
- [ ] Completion validates the final native profile independently, writes the bounded fixed report once, prints profile/report locations to the local process log, and exits with its verified code. Duplicate completions cannot overwrite the first decision. Timeout writes failure evidence if no decision has been accepted and exits nonzero. Keep artifacts and source on all paths. Manual close must not claim an automatic pass.
- [ ] Run focused GREEN, the affected default capability filter, typecheck and a dedicated Vite smoke build (not the whole frontend suite). Verify the default runtime has no resolved smoke completion grant and that the smoke context denies it to remote/secondary webviews. Tests use Tauri authority resolution, not source-string matching.
- [ ] Add one focused frontend driver failure-contract test with bounded/fake time: if initial catalog readiness never arrives, report failure once and never dispatch a lifecycle mutation. Transport mocks are allowed only in this unit test; the actual native smoke entry must retain real service/IPC. Run only this frontend file, not the whole suite.
- [ ] Document exact launch commands and limitations. Developer setup is a separate local-only Vite process plus `cargo build --locked --manifest-path src-tauri/Cargo.toml --features plugin-smoke --bin plugin-smoke`, then that exact binary with `--parent <canonical workspace-owned directory> --self-test`. Do not launch the GUI during implementation; controller reviews isolation first, then authorizes one actual native run. No dependency install, production app launch, automatic package cleanup or unbounded retries.
- [ ] Commit listed files and provide full report including focused evidence, build command, binary path and isolation checklist. Preserve any failure. Controller conducts scoped review, followed by the single native run and final integration review.

## Controller final gate

Verify exact feature binary/config, canonical workspace artifact parent, loopback port ownership and redirected process-log paths. Start only task-owned hidden helper processes, use bounded waits and never kill an unrelated listener. Run the automatic native flow once, retain profile/report/logs, verify process exit and backend-checked report. If it fails, diagnose the specific evidence and make a minimal test-first correction rather than repeating until green. Report native picker and whole-app exclusions explicitly.

This branch is a follow-on to #30. Reconcile reviewed diagnostic commits from #30 before integration if they landed after the fork; they touch package diagnostics, not the command/profile/harness files. Review this plan's own delta, not the previously reviewed 40,000-line plugin history.

## Plan self-review

| Interface | Consistency check |
| --- | --- |
| Task 1 internal | Tests never fall back to system stores; one profile owns one shared runtime; every lifecycle dependency gets its plugin root |
| Task 1 → Task 2 | Shared command state, selector, root/source and native check fields have explicit signatures; runner does not need AppState |
| Task 1 / Task 2 shared files | Cargo feature/state seam precede binary/export additions; tasks are sequential and file ownership is explicit |
| Task 2 internal | Exact seven handlers and one feature-only completion; same main-only capability; no default capabilities or production bootstrap |
| Native verification | Self-test selector bypass is disclosed; UI and independent native checks must both pass; global timeout is nonzero |
| Release/diagnostic boundary | Default binary remains selected; debug Windows only; no claim to resolve OS5 or validate whole production startup |
