# Native plugin smoke verification — 2026-09-11

## Verified result

One controller-reviewed, hidden Windows native run passed on implementation commit
`d43f7ced6a12ed153a138d5f7cb1adddf7870f0e` at approximately 21:50 Asia/Shanghai.
The process exited **0**, the fixed backend report recorded `passed: true` and
`uiSuccess: true`, and all five independent checks were true:

- unchanged fixture source;
- retained disabled state decision;
- empty managed ownership;
- empty local package directory;
- empty import/removal staging directories.

The real Marketplace DOM drove preview/cancel, managed import, enable/disable,
removal/cancel, confirmed removal and explicit reload/absence. The test used the real
store, service, seven native command wrappers, runtime and filesystem storage.
Only the native file selector was replaced with a host-owned fixed fixture selector.

This is plugin-only native UI evidence, **not** whole-application or OS-picker
verification. It neither reproduces nor fixes PR #30's historical Windows promotion
failure; that draft hold remains.

## Executable and retained artifacts

Executable SHA-256:
`2E368B6F7B444C0C7ECE1484D4884D304C4B5CB7693CFB36F5FA6380002BB49E`.

The exact reviewed build was
`target/worktrees/plugin-local-manifest-removal/src-tauri/target/debug/plugin-smoke.exe`
under the repository root. It was built with `--features plugin-smoke --bin plugin-smoke`;
the production default binary was not launched.

Artifacts remain in the native-smoke checkout:

```text
target/native-run-4401d5cb-a54c-4329-b9bb-1f5e9bf8a8ad/
  vite.stdout.log
  vite.stderr.log
  native.stdout.log
  native.stderr.log
  plugin-smoke-33b58be8-0af0-4a6b-ac62-e241dcf3deed/
    source/manifest.json
    plugins/
    webview2/
    report.json
```

The controller verified executable hash, vacant port, Vite's ownership of
`127.0.0.1:1430`, and the correct dedicated document before launching. Both helpers
were hidden, with separate logs and an external 115-second bound in addition to the
native 90-second hard deadline. The native host exited normally; the controller
stopped only its own Vite process. No artifacts were deleted. The run used the
unsandboxed execution lane to avoid the separately demonstrated sandbox ancestor
restriction; no ACL, antivirus, environment or registry setting was changed.

Native stderr retained a Chromium/WebView2 shutdown message:
`Failed to unregister class Chrome_WidgetWin_0. Error = 1412`.
It followed the successful report and did not change exit code 0. Its cause and
broader shutdown implications are unverified; this is not a claim of pristine logs.
The native test was not repeated to remove or hide that message.

## Focused supporting checks

| Scope | Evidence |
| --- | --- |
| Fixture profile | 5 focused tests passed after explicit parent-boundary correction |
| Shared commands / existing capability boundary | 13 / 4 tests passed after the state seam |
| Host / actual generated context and authority | 9 tests passed on `87caf8a` |
| Hard-deadline correction | Focused RED then GREEN 1/1 on `d43f7ce`; claimed or blocked report work cannot suppress nonzero stop |
| Real UI driver contracts | 2 tests passed on `d43f7ce` |
| Owned frontend lint / typecheck / dedicated Vite build | Passed after the final source correction |
| Native executable build | Passed on `d43f7ce`; hash independently checked before launch |

No full local suite was repeated. Existing Rust warnings remain. The hard-timeout
evidence worker is best-effort: a partial/missing report cannot prevent the independent
nonzero stop, and a report alone never proves success without the process exit code.

## Windows error distinctions and integration

PR #30's earlier failure occurred during initial import between BeforePromotion and
AfterPromotion. Its exact failing operation/NTSTATUS was not retained, so its root
cause remains unknown. Test-only native diagnostics now retain those details.

A separate system-temp fixture failed early at NtCreateFile with OS 5 / `0xC0000022`.
A controlled read-only comparison using the same empty fixture and native opening
flags failed at ancestor component 1 in the sandbox and opened all components outside
it. That establishes environment-sensitive ancestor access for that path; it does
not establish a production lifecycle defect or explain the historical promotion case.

The intended merge order is import PR #29, managed-removal PR #30, then this follow-on
branch. This branch is published as a narrow stacked draft against
`plugin/local-manifest-removal`. Current CI triggers pull requests targeting `main`
only: a stacked draft does not automatically have its own CI run. Retarget to `main`
after dependencies land and verify its CI before merging.

Remaining exclusions: OS picker interaction, whole AppState startup, scheduler/trading
coexistence and shutdown, accounts/keyring/providers, main-shell close guard,
installer packaging and restart recovery. The harness is deliberately tied to the
build checkout's target directory, rejects configuration overrides and WebView2
policy/environment overrides, and does not control OS/vendor telemetry.
