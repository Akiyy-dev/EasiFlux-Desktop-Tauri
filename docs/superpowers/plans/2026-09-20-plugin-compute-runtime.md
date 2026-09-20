# Executable local computation implementation plan

> Use superpowers:subagent-driven-development. Execute continuously under the user's existing autonomous development authorization; keep focused local validation and one final CI run.

**Goal:** usable local WebAssembly numerical plugins with a real SMA example.
**Spec:** `docs/superpowers/specs/2026-09-20-plugin-compute-runtime-design.md`.
**Architecture:** closed v4 contribution → Rust content-authorized no-import interpreter → finite scalar result; host-owned input UI. No new general host bridge.

## Global Constraints

- Work in the isolated plugin worktree on a `plugin/` branch, not the dirty root checkout.
- Preserve v1–v3 serialization/fingerprints and the existing managed manifest+receipt storage shape.
- All guest execution is explicit, backend-authorized, memory-only, and has no file/network/OS/Tauri/account/credential/trade access.
- Limits: manifest 16 KiB, module 8192 bytes, 1–4096 finite values, guest memory 1 MiB, total fuel 2000000, slice 10000, cooperative deadline 2 seconds, one active worker retained until exit.
- Do not launch the production application or read real account/AppData/keychain data. Use isolated fixtures/smoke only.
- Use TDD for meaningful behavior, focused local test groups, scoped formatting; do not repeat full suites per change.
- Do not push/merge from implementer agents. Controller performs authorized PR operations after review/checks.

## Task 1: Rust contract and authoritative executable runtime

Implement the spec's manifest v4 and full Rust runtime as one integrated task. Read the spec before edits. Primary files: `src-tauri/Cargo.toml`, lockfile, `plugin/{manifest,contribution,record,registry,runtime,mod}.rs`, new focused `plugin/compute*.rs` and `plugin/runtime/compute.rs`, `commands/plugin.rs`, `build.rs`, `lib.rs`, `plugin_smoke.rs`, `capabilities/plugin-runtime.json`, generated permissions and relevant ACL tests. Keep focused modules; avoid broad registry/runtime refactors.

1. RED: strict v4 roundtrip/old-version rejection and actual guest numeric run tests, then add dependency pinned to wasmi 2.0.0 with minimal features and canonical Base64 dependency if absent. Verify official crate APIs/MSRV and report necessary adjustments before changing architecture.
2. Exact params and ABI/limits from spec. Integer metadata retains Rust Eq; supplied parameter is finite f64 within metadata bounds. Parser validates encoding/header/size; executable validation enforces imports/start/ABI/structural restrictions before invocation.
3. Add narrow registry admission/captured identity revalidation. Reject unavailable/unreconciled authority, disabled content, stale generations/revisions, non-compute actions. Use fixed error codes and no raw guest/input logging.
4. Implement resource-bounded worker with lifetime-owned busy slot and request-ID cancellation. Cover drop/cancel/new-run ordering and lifecycle invalidation without holding registry locks or operation gate around guest code.
5. IPC wire contract: `execute_plugin_compute({request:{requestId,pluginId,contributionId,expectedCatalogGeneration,expectedRevision,values,parameter}})` returns `{schemaVersion:1,requestId,pluginId,contributionId,catalogGeneration,revision,value,inputCount,parameter}`. `cancel_plugin_compute({requestId})` returns `{schemaVersion:1,requestId,cancelled:boolean}`. Fixed opaque request ID: 1–64 ASCII alphanumeric/hyphen, rejected otherwise. Errors may use established AppError code transport; enumerate exact codes in report for frontend. Raw execute request denies unknown fields.
6. Add main-only ACL/build/handler registrations, including isolated smoke registrations, and update closed-list security assertions. Test guest success (inline small WAT fixture compiled with dev dependency if necessary), loop budget/cancel, invalid imports/start/ABI/OOB/nonfinite, busy, disabled/stale/content changes, late invalidation, and v1–v3 golden compatibility. Reuse shared cargo target cache. Run focused plugin/command/ACL groups once after development, not whole trading suite.
7. Commit task; report exact final wire contract, errors, limits, commands/results/TDD evidence and concerns to assigned ignored report.

## Task 2: Host computation UI and v4 transport

Implement frontend against Task 1 report and spec. Primary files: `types/plugin.ts`, `services/pluginService.ts`, new focused compute service/parser if useful, `stores/plugin.ts`, `composables/usePluginCommandResult.ts`, `components/plugins/{PluginCommands,PluginCommandWorkbench,PluginImportDialog,PluginManifestComparison}.vue`, new `PluginComputeDialog.vue` (or equivalent host-owned form), `services/pluginManifestDiff.ts` and related `tests/frontend/plugin*.test.ts` tests. Update v4-aware copy in `pluginPresentation.ts`, `PluginCard.vue`, `PluginMarketplacePage.vue` and `PluginRemovalDialog.vue` where existing text wrongly claims all local plugins are declarative or never executed.

1. RED: strict v4 parsing/old schema rejection, explicit compute action selection (never fall through to navigation), input parsing, async stale-result/cancel UI tests.
2. Mirror exact closed wire union and metadata constraints; module encoding/size checked without execution. Existing v3 TS type must exclude v4 compute despite shared contribution union. Error messages use fixed backend codes, not arbitrary backend text.
3. Add explicit compute summary/execution-intent branches to store/composable; no execution until user clicks Run. Input accepts comma or whitespace separators, validates raw text ≤128 KiB and 1–4096 finite decimal/scientific values (reject empty/hex/Infinity/NaN). Parameter defaults to manifest value and must be finite/ranged. Show busy/running/cancelling/completed/error, provenance, scalar/input-count/parameter; never interpolate guest as HTML.
4. Cancel on close/unmount/context invalidation; discard late responses after context change or a newer request. Hold local busy until the actual request settles. Do not persist inputs/results or log them. Duplicate Run disabled; backend still authoritative.
5. Import/compare/command UI labels acknowledge executable code and local-only input boundary. For code changes compare exact canonical module contents, display size and a clear code-change indicator rather than raw Base64. Do not assert publisher verification. Check all old action ternaries/switches for unsafe compute fallthrough.
6. Run covering frontend tests, typecheck, scoped lint and build once; keep standard Vite config loader. Commit and report evidence.

## Task 3: Ready-to-run example, authoring and isolated smoke

Create `examples/plugins/series-sma/{manifest.json,plugin.wat,build.mjs,README.md}` (exact script choice may adapt to existing dependencies). WAT exports the fixed ABI and computes the last period's average, returning NaN for noninteger/out-of-series period so host rejects it. Memory bounded with explicit maximum. Example algorithm must be guest code. Source-to-manifest reproducibility is verified, not a handwritten unrelated Base64 blob. Use a pinned development compiler/tool; it must not execute untrusted code or require author production secrets. Add a focused test invoking the actual checked-in example via production interpreter with `[1,2,3,4,5]`, parameter `3`, expected `4`.

Update `docs/plugin-authoring.md`, `docs/plugin-local-manifests.md` and add runtime guidance covering input contract, limits, lifecycle, safe example installation, cancellation, failure codes and unsupported features. No claims of signed/trusted publishers or arbitrary-plugin compatibility.

Extend isolated `plugin-smoke` coverage/script with actual v4 import→disabled rejection→enable→compute→disable rejection and forbidden-webview IPC. Use only dedicated temp-root fixtures, existing smoke isolation. Run focused example/compute test and isolated native smoke if feasible; report exact failures/blockers, never launch production app. Avoid repeatedly rerunning prior passing suites. Commit docs/example/smoke and provide complete evidence.

## Final integration

One independent whole-branch review across Tasks 1–3; fix material findings with covering tests. Verify clean scoped diff, preserved root changes, no secrets/user data. Push and create PR for runtime MVP; attach it. Merge only if final review and required CI are green; do not bypass failed checks. Record verified usage steps and remaining boundaries in final handoff. No release packaging/publishing is implied.
