# Executable plugin runtime verification

Work date: 2026-09-20 (Asia/Shanghai). This is the local verification snapshot; final review/CI and merge status are recorded on [PR #40](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/pull/40). This is not a packaged-release claim.

## Baseline and scope

- PR #39 initially failed two obsolete ready-preview assertions. Test-only commit `a504c8a` kept the exact privacy whitelist and asserted schema 2 plus `notInCatalog`; 13 command tests and 5 lifecycle smoke tests passed locally, followed by a clean independent scoped review.
- All nine remote checks then passed. [PR #39](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/pull/39) was merged as `3f4169d0d32d811d20604f7e37886097eea18177`.
- New branch: `plugin/compute-runtime`, isolated at the existing `target/worktrees/plugin-update-preflight` linked worktree. Merged main synchronized as `2018aa0`; unrelated root `dev/trading-risk-order-recovery` changes were left alone.
- This phase implements the [local compute design](../specs/2026-09-20-plugin-compute-runtime-design.md), not automatic updates, arbitrary third-party plugin compatibility or a packaged release.

## Decisions and dependency evidence

The user delegated implementation decisions without repeated approval questions and prioritized a usable runtime. Work is split by exclusive file ownership across backend, frontend and example/docs; shared-index commits and the native smoke launch are controller-coordinated. This overlaps independent work but still requires integrated tests and review. The tradeoff is possible integration rework if an interface changes.

Pinned wasmi 2.0.0 uses `std`, `validate` and `portable-dispatch`, with default features disabled. Safe module validation is deliberately retained. Its official [2.0.0 release](https://github.com/wasmi-labs/wasmi/releases/tag/v2.0.0) documents validation and dispatch configuration. Backend source inspection verified MSRV 1.86; local compiler is 1.97.1. Explicit smaller structural limits use a direct pinned wasmparser version already present in wasmi's dependency graph because wasmi exposes only its larger strict preset, not setters for those counts. This adds parser maintenance, not guest host capabilities.

On this date the official [linear-memory advisory](https://github.com/wasmi-labs/wasmi/security/advisories/GHSA-g4v2-cjqp-rfmq) lists fixes from 1.0.1, and the [host-call parameter advisory](https://github.com/wasmi-labs/wasmi/security/advisories/GHSA-75jp-vq8x-h4cq) lists fixes from 0.31.1. Pinned 2.0.0 is outside the affected ranges listed in those advisories. This targeted check is not a complete dependency audit or a claim of no vulnerabilities.

Controller rulings, in order:

1. Continue the bounded v4 design without another approval menu under the user's explicit delegation. A wrong scope choice costs ABI/UI rework, not access to production accounts.
2. Overlap backend and frontend work under disjoint file ownership and coordinated commits. The cost of an interface mistake is integration rework; final verification is still required.
3. Retain wasmi validation and portable dispatch with other default features off. This adds the necessary validator dependency, not guest host authority.
4. Overlap independent example/docs work, deferring native execution until runtime/UI and isolation review are ready. The cost is possible example adaptation, not an unreviewed production launch.
5. Add pinned wasmparser preflight for smaller structural limits not configurable through wasmi's public strict preset. The cost is maintaining the small parser boundary; interpreter validation stays enabled.
6. Diagnose the hidden smoke focus failure with a single-setting change and one controlled fresh run. If incorrect, the cost would be one isolated failed attempt and reverting that test-host setting; production windows and dependencies remain unchanged.
7. Publish an explicitly draft PR so remote CI and final read-only review can overlap. A later finding costs a follow-up commit/CI run; merge still requires both gates to pass.

## Verification boundary

Inputs and results are neither logged nor persisted. No production application/account/keychain/AppData access is authorized by this verification procedure.

Interpreter memory/fuel and cooperative cancellation are not a separate OS process sandbox or a hard real-time deadline. Existing Windows OS 5 reliability findings are not claimed fixed by this work.

## Focused local verification

All commands ran in the isolated worktree. Rust unit tests used the shared build cache at `../plugin-local-manifest-removal/src-tauri/target`; the dedicated native build used this worktree's `src-tauri/target`. No production application was launched. The full trading suite was not repeatedly run locally, following the user's request for focused testing.

| Scope | Result |
| --- | --- |
| Rust `cargo test --locked --lib plugin::` | 346 passed, 0 failed, 2 pre-existing crash-child fixtures ignored (invoked by parent tests) |
| Generated production `capability_tests` | 6 passed, including main-only compute and forbidden host expansion |
| `--features plugin-smoke plugin_smoke::tests` | 10 passed, including local-main allow and secondary/remote denial |
| Frontend lifecycle/CI security guards | 5 passed |
| Frontend compute, parser, store, UI, navigation, import/comparison compatibility (13 files) | 756 passed |
| Final host-only/executable disclosure regressions | 33 passed |
| `vue-tsc --noEmit`, scoped ESLint, production Vite build | Passed; pre-existing large-chunk advisory only |
| Checked-in WAT-to-manifest reproducibility and production interpreter example | 1 passed; series 1,2,3,4,5 with Period 3 returns 4; invalid periods rejected |
| Native-smoke frontend driver | 4 passed, including exact scalar comparison (40 must not pass as 4) |
| Review-fix lifecycle admission boundary regression | Deterministic failure before fix, then 1 passed |
| Affected runtime and compute groups after review fix | 97 and 11 passed; 2 pre-existing ignored child fixtures |
| Frontend review-fix comparison, cancellation and coexisting-form regressions | 12 passed after 4 expected RED failures; typecheck and four-file lint passed |
| Final-review first-import metadata regression | 11 passed after 1 expected RED failure; typecheck and two-file lint passed |
| CI scheduling-test correction | Exact late-result regression 1 passed; nearby runtime-compute group 9 passed |

Core implementation is `ee3966a`; frontend is `f5c222b`; admission-order fix is `bb8cf889`. Meaningful failure-first checks covered v4 parsing, real guest arithmetic, authority rejection, worker lease lifetime, late result invalidation, frontend request identity/context/cancellation, and code-change disclosure. Existing Rust dead-code warnings remain (36 default / 35 smoke); none were introduced in compute modules.

The independent backend reviewer found that invalidating an epoch before reserving the lifecycle gate admitted a narrow unchanged-reload race. The fix reserves/queues the real FIFO gate first, then advances the epoch and cancels active work. Its regression uses the actual coordinator mutex and gate, without a production scheduling hook or timing sleep. Scoped re-review approved the fix with no remaining findings or new lock-ordering issue.

## Isolated native verification

The dedicated `plugin-smoke` binary built successfully, but the first isolated Windows run exited 1 before the UI was created: WebView2 returned `HRESULT(0x80070057)` (invalid parameter). This is not a successful native compute test. Its retained report is `target/native-run-88177316-4e8f-4b14-9c20-80d921bb5325/plugin-smoke-e7266876-2bd6-435b-8429-6cea214a66aa/report.json` in the worktree, with separate native/Vite stdout and stderr logs under the parent.

The failed report confirms source unchanged and empty local/ownership/staging directories. `uiSuccess` and `disabledDecisionRetained` are false because no UI actions ran. Only the task-owned Vite helper was stopped; the fixture and logs remain.

Bounded diagnosis identified pinned wry 0.55.1's default creation-time focus request as the leading cause, matching the exact error mechanism in upstream [issue #1798](https://github.com/tauri-apps/wry/issues/1798) and [fix #1799](https://github.com/tauri-apps/wry/pull/1799). The smoke host hid its automatic window but still requested focus. Commit `b990dbe` adds only `.focused(!args.self_test)`, leaving visible manual mode, production windows, dependencies, profile isolation and command authority unchanged. The first generic error alone did not prove its call site; the one-variable controlled result below supplies the local evidence.

Exactly one controlled rerun followed a fresh dedicated build, with the same long parent-path pattern and final frontend fix `8738bf4`. The native process exited **0** and the retained report has `passed: true`, `uiSuccess: true`, `detailValid: true`, `nativeError: null`, and all five native checks true. Report: `target/native-run-56ba67dd-0d67-4b0b-a8cd-c6f3839043fd/plugin-smoke-e1ab7894-7c65-4591-98ac-9ecba8fb2fc6/report.json`. Binary SHA-256: `FBDEC9994E9895D4090FD63E5711B5291DC0273AE726CAAC598168BC0A01105E`.

Real DOM actions verified preview cancellation, managed v4 import, disabled computation rejection, explicit enable, guest SMA output exactly **4**, disable/rejection, isolated-host account IPC rejection, removal and reload. Source stayed unchanged, the disabled decision remained, and local/ownership/staging directories ended empty. Only the owned Vite helper was stopped. The same post-report Chromium class-unregistration warning (1412) seen in earlier passing smoke evidence remains; it did not prevent successful exit. Native picker, full application startup, real profiles, restart behavior and installers were not tested.

Frontend review found comparison disclosure and cancel-then-error precedence gaps, plus duplicate DOM IDs across coexisting forms. Commit `8738bf4` adds disclosure whenever either compared manifest contains compute, gives requested cancellation precedence over late success and error, and assigns per-instance label/ARIA IDs. Focused regressions and scoped re-review passed with all three findings addressed and no new breakage. Independent example/docs review approved with no findings; its native-evidence gap is now addressed by the controlled passing run.

## Final integration corrections

The whole-branch review found one additional first-import UI/documentation mismatch: the preview showed the compute title but not the runtime, ABI, decoded byte count or parameter metadata promised by the usage guide. Commit `c282516` displays those fields without module Base64 or guest execution. Its first-import regression failed before the change and the 11-test file passed afterward; typecheck and scoped lint also passed. Existing host-only import behavior remains covered.

The first remote Rust full-suite run on `22b8d4d` passed 1,141 tests but failed the test's first-poll-Pending assumption in `completed_worker_cannot_publish_after_reload_before_ipc_resumes` (2 pre-existing child fixtures ignored). A fast real blocking worker can legally finish before its first join poll; this was not a demonstrated production failure. The test-only correction uses its own single-worker blocking pool and a channel-confirmed blocker, then releases the actual guest worker, waits for its slot release, reloads, and checks the same cancelled late-output result. There are no production hooks, sleeps, repeated-until-green runs or weakened assertions. Exact and nearby tests passed as recorded above.

The first CI revision also passed frontend, Actions/JavaScript analysis, and all three Windows/macOS/Linux plugin-security jobs. Those results do not substitute for checking the amended final head. The scoped final-fix re-review and final-head CI outcome are recorded on PR #40 so this snapshot does not claim results that did not yet exist when it was committed.
