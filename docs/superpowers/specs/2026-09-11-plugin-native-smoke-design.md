# Isolated Native Plugin Smoke Design

## Purpose and approval context

After PR #28 merged, the user delegated the remaining work and previously requested autonomous decisions with minimal repeated tests. The agreed next outcomes are to explain the Windows lifecycle failure and obtain native plugin UI evidence without touching live trading configuration or credentials. This design addresses the second outcome only; green smoke results do not explain the historical OS 5 failure.

This is an architectural, developer-only test entrypoint. It is not a new end-user plugin feature or a whole-application profile switch.

## Options and decision

1. **Dedicated native plugin host with a shared command-state seam (selected).** Reuse the real seven plugin command wrappers, runtime/storage, Marketplace components, store and service. Construct no production `AppState`. The scope is small and the isolation boundary is explicit.
2. Duplicate the seven command wrappers in a harness. Smaller initial edit, but it would not verify production command wiring and would create drift.
3. Inject a global profile into every production store, credential repository, scheduler and frontend bootstrap. This can eventually test the whole app, but expands into trading subsystems and makes a missed injector consequential.

The selected host intentionally does not satisfy a claim that the entire production application has been smoke-tested. That larger validation remains explicitly unverified.

## Shared command seam

Introduce `PluginCommandState` in `src-tauri/src/commands/plugin.rs`, owning the existing `Arc<PluginRuntime>`. The existing seven commands consume this state instead of `State<AppState>`; names, inputs, outputs, helpers and lifecycle behavior remain unchanged. Production setup manages a clone of `state.plugins` in this state alongside the existing `AppState`. No scheduler or storage initialization is removed from normal startup.

An optional selector override is available only to tests and the `plugin-smoke` feature. Production construction always uses the existing native selector. Automated smoke uses a selector returning only its fixture source, so it can run unattended; it does not claim to automate the operating-system file picker. Manual harness mode retains the real native picker.

## Isolated fixture profile

The opt-in host requires `--parent <absolute existing directory>` within the build checkout's `target` directory (derived from the parent of `CARGO_MANIFEST_DIR`); there is no default to platform configuration or data directories. Resolve this parent once, reject missing/non-directory/relative/out-of-workspace parents before reserving a child, and create one fresh `plugin-smoke-<UUID>` child with an exclusive create. Compare canonical paths against the canonical artifact root, rejecting an artifact-root symlink that resolves outside the canonical checkout. Never accept an existing profile for reuse, overwrite it, delete it on exit, or automatically retry creation. This developer-only host is intentionally tied to the source checkout where it was built; arbitrary/system-temp parents are unsupported and reject without writes. This enforces the workspace-owned test-artifact boundary, not a workaround or explanation for the separate Windows directory-open error.

All application-owned plugin files live beneath that fresh child:

```text
plugin-smoke-<UUID>/
  source/manifest.json
  plugins/state.json
  plugins/managed-ownership.json
  plugins/local/
  plugins/import-staging/
  plugins/removal-staging/
  webview2/
  report.json
```

Compose `PluginRegistry::initialize`, `PluginStateStore::with_path`, `ManagedOwnershipStore::with_plugins_root`, `SystemLocalPluginPackageStorage::with_plugins_root`, `discover_from_plugins_root` and `PluginRuntime::with_lifecycle_services`. Both import and removal use the same explicit package root; never use a default system-backed constructor as a fallback. Initialize local/staging directories as needed inside the new profile.

The fixed fixture is metadata-only, ID `com.easiflux.smoke`, version `1.0.0`, empty contributions and requested capabilities. Its source bytes must remain unchanged through removal. A harness-only reader accepts only the canonical fixture source and then delegates to `SystemLocalManifestReader`; selecting any other file rejects before reading its contents. The fixed selector is an input boundary replacement, not a mock runtime or fake persistence.

Profile helpers are compiled only for tests or `plugin-smoke`. The root remains owned for the host lifetime and is retained afterward for inspection. No recursive cleanup is introduced.

## Native host and security

- New Cargo feature `plugin-smoke` is opt-in and disabled by default. A dedicated `plugin-smoke` binary has `required-features = ["plugin-smoke"]`. Existing default binary selection remains `easiflux-desktop`.
- The runner refuses non-debug builds before any profile write. The first supported execution platform is Windows; unsupported platforms report that limitation without launching.
- Use a separate Tauri configuration with a distinct application identifier, no automatic windows, and explicitly selected capabilities. Reuse only `plugin-runtime`, plus one local-main-only smoke completion permission. Do not load default opener/window capabilities implicitly.
- Setup constructs only the fixture profile and `PluginCommandState`, initializes the Rust-owned dialog plugin, and registers the exact seven production plugin handlers plus the harness-only completion handler. It never constructs `AppState`, scheduler, account stores, keyring repositories, providers, opener plugin or app-ready emitter.
- Create a window/webview labeled `main` programmatically, with an absolute `.data_directory(profile_root.join("webview2"))`. This must be set before WebView2 creation; a relative configuration field is not sufficient.
- The dedicated frontend uses only Vue, Pinia, existing global CSS and `PluginMarketplacePage`; never mount `App.vue` or `AppShell`.
- Development assets are served by a dedicated Vite configuration bound to `127.0.0.1:1430`, strict port, with no fallback to another server. A missing/busy server fails the bounded launch. Restrict navigation and CSP to the local development origin, required local IPC and HMR; no provider/network endpoints or remote browsing are enabled. This concerns application networking, not a promise to control operating-system or WebView2 vendor telemetry.
- No path, source JSON or new fields are added to the production plugin IPC responses. Profile/source paths may be printed to the developer's local process log for manual fixture selection.

## Native smoke flow

`--self-test` creates a hidden native window and uses the fixed fixture selector. Its dedicated frontend drives real rendered controls and real IPC, using existing test IDs and bounded condition waits:

1. Initial catalog becomes available.
2. Open import preview, cancel it, and verify no local fixture item appeared.
3. Open a new preview, confirm import, and verify a managed, disabled item appears.
4. Enable then disable the item using actual card controls, verifying displayed/store state.
5. Open removal confirmation and cancel once; verify the item remains.
6. Confirm removal; verify the success result and absence after explicit reload.

Do not replace `pluginService`, `tauriInvoke`, the store, the runtime, or storage with a fake. DOM interaction is the automated lane; native picker interaction remains a manual lane and must be reported separately.

The harness-only completion command accepts a bounded success/detail result and independently verifies the fixture source bytes, retained disabled state decision, empty ownership entries, and empty local/removal staging directories. It writes a fixed `report.json` within the owned profile and exits with zero only when both UI and native checks pass. Completion is one-shot: only the first decision is accepted, and duplicate calls cannot overwrite it. Failure, malformed detail, timeout or native check failure cannot create a passing report. A global 90-second self-test deadline exits nonzero; per-condition waits are bounded and are not retries of lifecycle mutations.

Manual mode displays the isolated Marketplace with section navigation and clear fixture-only instructions. It keeps the native file picker and exits normally on close. It does not run the automatic destructive fixture flow without `--self-test`.

## Verification and release boundary

- Task-level tests are focused: profile rejection/isolation, real fixture lifecycle, and one frontend smoke-driver contract. Reuse existing command/capability tests where they already cover behavior.
- Run one affected command/capability check after the state seam, and one native self-test after the complete host has been independently reviewed for isolation. Do not launch the production binary for testing.
- The controller must verify the exact executable, feature/config, canonical artifact parent, separate loopback server and log destinations before launch. Background helpers are hidden and only task-owned processes may be stopped.
- Retain the profile, native report and process logs; do not erase failure evidence. Do not rerun until green.
- Explicitly unverified: whole `AppState` startup, scheduler coexistence/shutdown, accounts/keyring/providers, main-shell navigation/close guard, packaged installers and restart recovery. Existing crash-recovery unit tests remain separate evidence.
- PR #30's historical Windows reliability hold remains until supported by actual failure/root-cause evidence. The harness lives on `plugin/native-smoke` and should be reviewed as a follow-on to #30, not silently folded into the import PR.

## Self-review

The command seam preserves production behavior while giving the harness exact wrapper reuse. Every runtime dependency has an explicit fixture-root binding; WebView2 has its own absolute directory. Automated selector substitution and whole-app exclusions are stated, so native smoke cannot be mistaken for native picker or full-application validation. The default binary/features/capabilities remain unchanged. Both success and timeout have bounded outcomes, and all fixture artifacts remain inspectable.
