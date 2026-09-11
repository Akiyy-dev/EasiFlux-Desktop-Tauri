# Isolated native plugin smoke (developer-only)

This opt-in Windows debug binary loads the real Marketplace components, Pinia store,
service and seven production plugin commands against a fresh fixture-owned profile.
It never starts the production `run()`, `AppState`, accounts, keyring, providers,
scheduler, opener plugin, App.vue or AppShell.

## Build and launch

The following exact commands apply to this reviewed checkout. Dependencies must
already be installed; this procedure does not install anything. The normal default
binary remains `easiflux-desktop`; do not use it for this test. The host uses the
independent `src-tauri/plugin-smoke/tauri.conf.json`, not a merged production config.
Do not set `TAURI_CONFIG` or enable `custom-protocol`; feature builds reject inherited
`TAURI_CONFIG` overrides without logging their contents.

Before reserving any profile, the host also rejects every `WEBVIEW2_`/`COREWEBVIEW2_`
environment name (case-insensitive). It probes only the presence of the WebView2 policy
root in HKCU/HKLM, in both 32-bit and 64-bit registry views, and rejects any present
root or error other than absence. It reads no policy values and modifies no environment
or registry settings. This is conservative rejection, not support for policy-managed
machines: [WebView2 documents environment and registry overrides of creation options](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/webview2-idl?view=webview2-1.0.4129.50).

```powershell
Set-Location D:/EasiFlux/EasiFlux-Desktop-Tauri/target/worktrees/plugin-native-smoke
cargo build --locked --manifest-path src-tauri/Cargo.toml --features plugin-smoke --bin plugin-smoke --target-dir D:/EasiFlux/EasiFlux-Desktop-Tauri/target/worktrees/plugin-local-manifest-removal/src-tauri/target
```

In a separate terminal, start only the dedicated Vite server. This command uses the
existing Node/dependencies and a runner loader to avoid bundled-config temporary
writes into the ancestor checkout's node_modules:

```powershell
Set-Location D:/EasiFlux/EasiFlux-Desktop-Tauri/target/worktrees/plugin-native-smoke
& 'C:/Program Files/nodejs/node.exe' D:/EasiFlux/EasiFlux-Desktop-Tauri/node_modules/vite/bin/vite.js --config vite.plugin-smoke.config.ts --configLoader runner
```

It must own `127.0.0.1:1430`; strict port mode never falls back. Stop if another
listener owns the port—do not kill an unrelated process. Vite is rooted explicitly
at this checkout, uses Vue and the existing Tailwind plugin, and supplies a local-only
CSP response header. Navigation is limited to this origin; new windows are denied.

Only after an isolation review, create one parent under this build checkout's
`target` and launch the exact dedicated executable once:

```powershell
Set-Location D:/EasiFlux/EasiFlux-Desktop-Tauri/target/worktrees/plugin-native-smoke
$smokeParent = New-Item -ItemType Directory -Path (Join-Path (Get-Location) ('target/native-run-' + [guid]::NewGuid()))
$smokeCanonicalParent = (Resolve-Path -LiteralPath $smokeParent.FullName).Path
& 'D:/EasiFlux/EasiFlux-Desktop-Tauri/target/worktrees/plugin-local-manifest-removal/src-tauri/target/debug/plugin-smoke.exe' --parent $smokeCanonicalParent --self-test
$LASTEXITCODE
```

When a controller starts background helpers, use `Start-Process -WindowStyle Hidden`,
redirect stdout/stderr to distinct task-owned files under the canonical parent, retain
the returned process identities, use bounded waits, and stop only those owned helpers.
Do not launch either executable from a different build checkout by assuming its
`--parent` binding changed: the artifact boundary is compiled from `CARGO_MANIFEST_DIR`.

## What happens

The required absolute existing parent is canonicalized and must be inside this
build checkout's canonical `target` directory. Relative, missing, system-temp and
out-of-checkout parents reject before profile creation. The host reserves a new
`plugin-smoke-<UUID>` child, never reuses a profile, and keeps all artifacts.

The hidden `--self-test` lane uses a fixed native fixture selector. Real DOM controls
perform preview/cancel, managed import (disabled), enable/disable, removal/cancel,
confirmed removal, and explicit reload/absence verification. Each condition has a
15-second bound; mutations are dispatched once, not retried. The native deadline
starts before WebView creation and is 90 seconds. The selector never receives a
frontend-supplied path, and the reader accepts only the fixture source.

`source/manifest.json`, `plugins/`, absolute `webview2/`, and the fixed `report.json`
are below the fresh profile. The process log prints the profile/source/report paths.
Completion is one-shot: UTF-8 detail is capped at 2,000 bytes; oversize detail forces
failure. The report independently checks unchanged source bytes, a retained disabled
decision, empty ownership, and empty local/import/removal staging directories.
Exit 0 requires both UI success and every native check. Timeouts, native-check failures,
premature UI success and report-write errors cannot pass. Report writes use exclusive
creation and sync before exit; duplicates cannot replace the first decision.

Without `--self-test`, the visible manual lane keeps the native picker and shows
fixture-only instructions and Installed/Market/Manage controls. Select only the
manifest printed in the local host log. Manual close does not produce an automatic
passing report, and the completion command rejects manual-mode submissions.

## Focused checks

From this checkout, use the same explicit Cargo target directory on every Cargo command:

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml --features plugin-smoke plugin_smoke::tests --lib --target-dir D:/EasiFlux/EasiFlux-Desktop-Tauri/target/worktrees/plugin-local-manifest-removal/src-tauri/target
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin_commands_are_available_only_to_the_local_main_webview --lib --target-dir D:/EasiFlux/EasiFlux-Desktop-Tauri/target/worktrees/plugin-local-manifest-removal/src-tauri/target
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin_smoke::tests::default_context_denies_smoke_completion --lib --target-dir D:/EasiFlux/EasiFlux-Desktop-Tauri/target/worktrees/plugin-local-manifest-removal/src-tauri/target
& 'C:/Program Files/nodejs/node.exe' D:/EasiFlux/EasiFlux-Desktop-Tauri/node_modules/vitest/vitest.mjs run tests/frontend/pluginSmokeSelfTest.test.ts --config vite.plugin-smoke.config.ts --configLoader runner --environment jsdom
& 'C:/Program Files/nodejs/node.exe' D:/EasiFlux/EasiFlux-Desktop-Tauri/node_modules/vue-tsc/bin/vue-tsc.js --noEmit
& 'C:/Program Files/nodejs/node.exe' D:/EasiFlux/EasiFlux-Desktop-Tauri/node_modules/vite/bin/vite.js build --config vite.plugin-smoke.config.ts --configLoader runner
```

The generated completion permission metadata is not a grant. Default context authority
denies completion, and its invoke handler is unchanged. Smoke authority tests resolve
the eight exact commands for local main and deny remote/secondary access and default
window/opener/dialog frontend permissions. Rust still owns the native dialog plugin.

## Limits and retained evidence

No automatic cleanup, dependency installs, whole frontend suite, or production app
launch is part of this procedure. Inspect the retained failing report and logs before
any minimal test-first correction; never repeat native runs until one happens to pass.
Smoke build output is under `target/plugin-smoke-dist`; old build assets are retained.
Inspect the stylesheet referenced by the current HTML, not an obsolete retained asset.

The automatic lane does **not** test the OS file picker, whole `AppState` startup,
scheduler coexistence/shutdown, accounts/keyring/providers, main-shell navigation/close
guard, installers, or restart recovery. It does not control WebView2/OS vendor telemetry.
The historical system-temp Windows NtCreateFile OS 5 failure is separate evidence:
workspace-owned fixtures do not explain or fix that failure or lift its reliability hold.
