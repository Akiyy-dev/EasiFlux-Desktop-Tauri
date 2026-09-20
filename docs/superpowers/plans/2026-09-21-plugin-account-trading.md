# Plugin Account Trading Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable real authorized account-data access and host-confirmed placement/cancellation from executable plugins.

**Architecture:** Manifest v5 runs an import-free JSON Wasm guest against granted sanitized snapshots. Native content/account-bound grants and one-shot confirmation tokens mediate all access. Production uses existing account serialization, risk, order journal and trading services; smoke injects a fake host.

**Tech Stack:** Rust/Tauri 2, wasmi 2.0.0, Vue 3/TypeScript/Pinia, Vitest, WAT fixtures.

**Spec:** `docs/superpowers/specs/2026-09-21-plugin-account-trading-design.md` (read the full spec before implementing; wire names/shapes there are binding).

## Global Constraints

- Preserve v1-v4 behavior, exact canonical fingerprints and the catalog transport schema (3).
- Manifest/file limit remains 16 KiB; each canonical base64 Wasm module remains at most 8192 decoded bytes.
- Runtime stays import-free/no-start/no-WASI/no network, 1 MiB memory, 2,000,000 total fuel, 10,000-fuel slices and a 2-second cooperative deadline. Share the existing process-wide actual-worker-owned slot with v4; lifecycle invalidates late results.
- Decimal financial values remain strings. Never derive account totals with f64. No API key, secret, credential digest, raw exchange payload, diagnostics, local path or arbitrary API URL is exposed to the guest.
- Orders always pass invariant validation even when configurable risk is disabled, then use existing TradingService and durable submit_once. Plugin code cannot select its own orderLinkId.
- Never launch production AppState/app, read actual profiles/keychain, or place real trades in development. Tests use mock host/network boundaries and temporary storage.
- Focused RED/GREEN tests per task; one final typecheck/build and CI validation, not repeated full local suites.

## Review Focus

- Account/credentials changing while confirmation waits: recheck native private authority under the final account mutation guard.
- Revocation or catalog reload after guest success but before publication: reject queued old results and confirmations.
- Dropped confirmation IPC during a network request: native task owns admission until completion; never allow replay.
- Market data partial/missing or decimal values malformed: no misleading zero defaults, totals or completeness claims.
- V4 fallthrough into the new action or outdated ACL registration: old workflows keep working; new commands are narrowly registered and validated end-to-end.

### Task 1: Native workflow authority, executor and production trading bridge

**Files:**
- Create focused modules under `src-tauri/src/plugin/workflow/` (`mod.rs`, `contract.rs`, `host.rs`, `production.rs`, `tests.rs` and focused tests if needed), plus `plugin/registry/workflow.rs`, `plugin/runtime/workflow.rs`, `commands/plugin_workflow.rs`.
- Modify `plugin/mod.rs`, `plugin/manifest.rs`, `plugin/contribution.rs`, `plugin/registry.rs`, `plugin/runtime.rs`, `plugin/compute/sandbox.rs` (share bounded execution machinery), command module/registration, `state.rs` or production setup, `api/client.rs` for a safe internal session identity if required, `build.rs`, narrowly scoped capability/permission files and existing ACL tests.
- Modify normal trading command helpers only if needed to reuse the existing submit_once route; preserve normal behavior and root dirty files.

**Interfaces:**
- Produces all five IPC routes and exact wire shapes in the spec, plus a mockable native WorkflowHost seam usable by Task 3 without AppState.
- Consume existing `PluginIdentity`, registry publication authority, `operation_gate`, `compute_epoch`, `ComputeSlot`, `AccountLifecycleCoordinator`, `TradingService`, `OrderSubmissionStore`/`submit_once` and safe sanitized models.
- Public crate-internal seam: `WorkflowHost` as a Send+Sync host abstraction (boxed Send futures are suitable), `WorkflowHostState` owns `Arc<dyn WorkflowHost>`, and `PluginRuntime` methods receive an owned Arc host for run/confirmation. Host exposes its lifecycle coordinator so the broker holds the final read/write guard rather than nesting fair locks. Clearly suffix methods requiring that guard with `_locked`.
- Production host owns only required existing dependency handles; injected host must need no credential repository. Expose confirmation receipt and snapshot contract types for smoke. Notify controller immediately of a necessary wire/seam adjustment before frontend depends on it.

- [ ] **Step 1: Add focused contract tests and observe RED.** Tests must parse v5, reject v5 action in v4, unknown/duplicate/unrequested capabilities, invalid JSON object default and preserve existing v1-v4 golden hashes. Use real serde parsing, not text assertions:

```rust
let manifest = serde_json::from_value::<PluginManifest>(workflow_manifest()).unwrap();
assert_eq!(manifest.schema_version, 5);
let mut old = workflow_manifest(); old["schemaVersion"] = 4.into();
assert!(serde_json::from_value::<PluginManifest>(old).is_err());
```

Run `cargo test --manifest-path src-tauri/Cargo.toml --lib plugin:: -- --test-threads=1` with the shared cache once to establish focused RED; do not run whole suite per edit.

- [ ] **Step 2: Implement v5 validation, strict types and the JSON guest executor.** Keep previous canonical serialization unchanged. Reuse sandbox preflight/config/budget; refactor a shared bounded invocation setup rather than duplicating its security rules. Test a real WAT module receiving both input buffers and producing a typed display, invalid pointer, import, loop budget and malformed/trailing/unknown result fields. Shared slot must remain worker-owned after dropped futures.

```rust
// Output is native validated before any proposal is retained.
match output { WorkflowOutput::Display { .. } => None, _ => Some(prepare_once(...)) }
```

- [ ] **Step 3: Implement access/grants and native run/confirm orchestration.** Follow the spec wire contract verbatim. Grant storage starts empty, is capped/garbage-collected and checks full identity, runtime epoch, private account authority and independent grant revision. Gate order is plugin operation admission -> account lifecycle -> risk; do not acquire the same fair account lock recursively. Reads query only granted sections and revalidate on publication. Confirm consumes an immutable 60-second token, rechecks after both guards, and moves admission into an owned native task through final side effect. Every grant update invalidates prior prepared/in-flight results; lifecycle epoch invalidates them without guest-controlled data. Hard cap prepared tokens at 32.

```rust
assert_eq!(fake.placed().len(), 0); // actual broker run never calls exchange port
let receipt = runtime.confirm_workflow(host.clone(), token.clone()).await.unwrap();
assert_eq!(receipt.status, TradeStatus::Accepted);
assert!(runtime.confirm_workflow(host, token).await.is_err());
assert_eq!(fake.placed().len(), 1); // authority/one-shot boundary, not a mocked broker
```

- [ ] **Step 4: Connect production host and narrow IPC/ACL.** Access current account/connected session without serializing credentials. Bind to a private authority fingerprint which changes when key/secret/environment/session changes (order_submission_scope alone omits secret). Snapshot copies validated Balance/Position/Order/quote fields, limits 100 entries with partial flags and per-section retrieval timestamps; unauthorized sections are null and unqueried. Place validates all invariants independently of risk, generates UUID/orderLinkId, then calls existing submit_once + TradingService::place_order; cancel uses TradingService::cancel_order with captured account context. Never create a new API client from guest input. Sanitize errors into rejected/unknown receipts. Register permissions only on trusted host surfaces, not guest content; preserve legacy ACL behavior.

- [ ] **Step 5: Cover authority races and focused GREEN.** Test no-grant denial/no private section requests, subset/revoke, content reload, account switch and same-key secret replacement, revoked queued success, stale/expired/replayed token, cancel absent from captured orders, invalid quantity with risk disabled, placement/cancel exact DTOs and unknown no retry. Deterministic barriers rather than sleeps for races. Run focused Rust plugin and changed adapter tests, record command/output and red evidence, self-review, then request controller commit coordination. Do not run Cargo concurrently with Task 3. Report full evidence in the assigned task report file.

### Task 2: Usable permission, account snapshot and transaction confirmation UI

**Files:**
- Create `src/types/pluginWorkflow.ts`, `src/services/pluginWorkflowService.ts`, focused service tests, `src/components/plugins/PluginWorkflowDialog.vue` and its tests (split snapshot/confirmation rendering if needed).
- Modify `src/types/plugin.ts`, `src/services/pluginService.ts` and tests, `src/stores/plugin.ts` and tests, `src/composables/usePluginCommandResult.ts` and tests, `PluginCommands.vue`, `PluginCommandWorkbench.vue`, `PluginImportDialog.vue`, `PluginManifestComparison.vue`, relevant presentation/marketplace/removal copy and styles.
- Do not edit Rust, smoke harness, examples or backend files owned by other tasks.

**Interfaces:**
- Consume exact manifest v5 / IPC wire contract from the full spec. Backend unavailable during initial UI work is expected; tests inject only the invoke boundary with exact DTO fixtures.
- Produce explicit `sandbox.accountWorkflow` command summary and `PluginWorkflowExecutionIntent` carrying plugin/contribution/title, defaultInput and expected catalog counters. Avoid existing compute fallthroughs by explicitly narrowing action variants. V4 contributions exclude workflow.

- [ ] **Step 1: Add focused RED parser and action-routing tests.** Verify v5 requested capabilities preserved, v4 new action rejected, deep clones preserve workflow params and capability arrays, stale/corrupt access/receipt DTOs rejected, and workflow commands open the correct dialog without invoking trade.

```ts
expect(parseWorkflowAccess(accessFixture(), intent).grantedCapabilities).toEqual([])
expect(() => parseWorkflowAccess({ ...accessFixture(), pluginId: 'wrong' }, intent)).toThrow()
expect(store.runCommand(pluginId, workflowId)?.actionId).toBe('sandbox.accountWorkflow')
```

Use `node_modules/.bin/vitest.cmd run <changed-test-files>`; record actual missing-behavior RED. No full suite on each edit.

- [ ] **Step 2: Implement strict closed service parsing and catalog integration.** Validate exact keys, objects, capabilities and subset relations, bounded strings, canonical counters/timestamps, numeric enum constraints and correlated identities, including requestId/account/session/grant revision for runs and token for receipts. Keep decimal values as strings, never Number for financial values. Build outbound requests from allowlisted fields, not arbitrary spreading of guest payloads. Deep clone all v5 params/arrays. Display fixed safe error messages; no raw errors or module bytes.

- [ ] **Step 3: Implement real host UI.** Use accessible labels/useId, pending states and plain text. Load Access to show connected account/environment and selectable requested grants; separate grant/revoke from run. JSON input limited to 4096 UTF-8 bytes and object-only, symbol validation, default/example clearly visible. Show granted sections with timestamps and partial warnings; never call null sections empty/complete. Show immutable canonical order fields and account/plugin/expiry in a separate explicit real-trade confirmation. Confirmation sends token only; duplicate click blocked; accepted/rejected/unknown differ, unknown explains to check existing trading/recovery UI without resubmitting. Changing input/symbol/grants invalidates prior output/confirmation. Observe store catalog context and account/session changes; ignore late async responses after close/unmount/context change. Permit revocation without a new private snapshot; explain session-only grants and inability to undo transmitted orders.

```ts
await wrapper.get('[data-testid="workflow-run"]').trigger('click')
expect(invokeCalls.filter(([name]) => name === 'confirm_plugin_workflow')).toHaveLength(0)
await wrapper.get('[data-testid="workflow-confirm"]').trigger('click')
expect(lastConfirmArgs).toEqual({ token: 'fixed-test-token' })
```

- [ ] **Step 4: Update plugin copy and focused GREEN.** Import/update views explicitly disclose requested data/trading capabilities and that installation/enabling gives no grant; avoid blanket no-trading claims for v5 while preserving accurate legacy descriptions. Tests cover subset grant, missing authority, no execution before explicit confirm, exact-token confirm, unknown/no retry and account/context changes suppressing late result. Run changed Vitest tests, vue-tsc and lint for touched TS/Vue files. Self-review and coordinate commit with controller; full report at assigned path.

### Task 3: Runnable examples, isolated native acceptance and author handoff

**Files:**
- Create `examples/plugins/account-workflow/` with importable manifests, readable WAT source and reproducible generation/check instructions; create `docs/plugin-account-workflow.md`.
- Modify focused smoke modules under `src-tauri/src/plugin_smoke/` (resolve actual existing harness paths), smoke frontend and service tests, `docs/plugin-native-smoke.md`, `docs/plugin-authoring.md`; add `docs/superpowers/verification/2026-09-21-plugin-account-trading.md`.
- Do not alter production broker contracts without controller approval; consume Task 1's WorkflowHost seam and Task 2 dialog.

**Interfaces:**
- Use exact v5 module/ABI and IPC wire contract in spec. Native smoke manages injected WorkflowHostState and real PluginRuntime, never AppState. Existing smoke isolation and forbidden account-IPC probe stay in place.
- Example includes data display reading an actual granted field from guest context and user-parameterized place/cancel proposals, with explicit real-trade warnings. Entire manifest remains <=16 KiB; individual modules <=8192 bytes. Split into multiple manifests if combined modules exceed cap, with distinct stable plugin IDs.

- [ ] **Step 1: Build fixtures from readable WAT.** Write tests exercising actual packaged bytes against fake snapshots: available balance read is derived from context (different balances give different display), placement uses supplied input, cancellation uses selected captured order ID. Require data grant/connection and confirmation in instructions. No demo keys, personal endpoints, real account IDs or private artifacts.

```rust
assert_eq!(execute_fixture(snapshot_with_available("12.5")), "可用余额: 12.5");
assert_eq!(execute_fixture(snapshot_with_available("8")), "可用余额: 8");
```

- [ ] **Step 2: Extend isolated smoke with a mock account host.** Verify real UI enable -> access/no grants -> explicit grants -> snapshot from fake account -> run place (no mutation) -> confirm -> accepted exactly once -> run cancel -> confirm -> accepted -> revoke blocks future access -> disable/reload. Use fixed synthetic data and native counters/receipts to prove no implicit/double mutation. Keep smoke-only host unable to create production API clients/load credentials. Existing short bounded watchdog, visible/focus fix and profile isolation remain.

- [ ] **Step 3: Run the one native acceptance and document evidence.** Coordinate Cargo cache/build with controller. Use standard Vite loader and explicit worktree root; helpers hidden, retain PID, stop only owned helper. Report any unavailable native acceptance accurately instead of claiming live trading tested. Update author docs with exact ABI, capabilities, examples, confirmation/recovery limitations and remaining unattended-strategy scope. Run final frontend build/typecheck plus native focused checks once, save results and self-review, then coordinate commit.

## Final integration

- [ ] Review each task against its brief and exact diff, resolve material findings, then run one whole-branch security/correctness review.
- [ ] Inspect exact staged paths/diff for accidental credential or unrelated file inclusion. Create and attach a PR to Akiyy-dev/EasiFlux-Desktop-Tauri; let CI supply final broad regression evidence. Merge only if the reviewed head and required checks are suitable.
- [ ] Report usable account/order/cancel workflow, PR status, precise validation and explicit limits; never claim tested real exchange execution when only mock acceptance was run.
