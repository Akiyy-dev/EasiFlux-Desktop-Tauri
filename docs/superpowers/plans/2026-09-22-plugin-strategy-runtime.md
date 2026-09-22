# Plugin Strategy Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Host continuously running, explicitly authorized strategy plugins that automatically read account data and place/cancel orders without per-order user confirmation.

**Architecture:** Separate v6 strategy callbacks from v5 confirmed workflows. A native supervisor owns admission, fresh snapshots, bounded Wasm callbacks, durable state/intent/receipt/ownership, order-journal acknowledgment, lifecycle controls and reconciliation. Vue exposes explicit start policy and an always-accessible monitor.

**Tech Stack:** Existing Rust/Tokio/serde/rust_decimal/wasmi, Vue/Pinia/TypeScript/Vitest, Tauri 2 ACL. No new runtime dependency.

**Spec:** `docs/superpowers/specs/2026-09-22-plugin-strategy-runtime-design.md`

## Global Constraints

- User delegated implementation and suitable PR integration without intermediate approval; never use live credentials, production AppState/profile/keychain, live HTTP or real trades in verification.
- Worktree `D:/EasiFlux/EasiFlux-Desktop-Tauri/target/worktrees/plugin-update-preflight`; branch `plugin/strategy-runtime`; baseline `f1c3ec48a9df88f7f06c9b9630088dc2067acfe5`.
- v1-v5 behavior and canonical bytes remain unchanged; catalog schema remains 3; v6 alone adds sandbox.strategy / strategy-json-v1 and strategy.run.
- Manifest <=16 KiB, module <=8,192 bytes; guest context <=65,536 bytes, input/state <=4,096 bytes each, output <=16,384 bytes; import-free sandbox retains 1 MiB / 2,000,000 fuel / 2 seconds.
- Policy intervalMs 5,000..60,000, maxRunSeconds 60..86,400, maxActions 1..1,000, positive <=64-byte Decimal quantity limits; native enforcement cannot be disabled.
- One active run per account, one action/callback in flight, max four workers, max 32 persistent runs, <=16 MiB durable snapshot; no automatic restart or retry of uncertain actions.
- Exact IPC/guest shapes, start-ticket binding, persistence rules and lifecycle semantics are defined by the spec. Treat that document as the binding cross-task interface.
- No new dependency, raw host error/secret logging, generated build artifacts, unrelated root changes or real profile data in commits.
- Focused tests during development; root owns broad final CI/build. Use explicit Cargo target-dir src-tauri/target, coordinate Cargo with root; preserve and report existing warnings.

## Review Focus

- Credential reinstall with unchanged public epoch: captured ticket/run must invalidate before automatic dispatch (Task 1 race test).
- Pause/stop while HTTP is already admitted: no second action; receipt must still persist and uncertain result must not become stopped (Task 1 barrier test).
- Accepted order but order-journal finish/ack fails: no next callback; durable reconciliation uses same identity (Task 1 injected persistence test).
- Same-shape v5/v6 Wasm params and requested capabilities: no untagged fallback or silently broadened v5 authority (Tasks 1 and 2 parser tests).
- Closed launch dialog/disabled plugin/disconnected account: stop-all remains available and closing does not stop or restart background run (Task 2 DOM/service test).

---

### Task 1: Native automatic strategy host

**Files:**
- Create `src-tauri/src/plugin/strategy/{mod,contract,store,supervisor,execution,recovery,tests}.rs`; split focused test modules under strategy/tests as needed.
- Create `src-tauri/src/plugin/registry/strategy.rs` and `src-tauri/src/plugin/runtime/strategy.rs` as narrow registry/runtime access seams.
- Modify Rust plugin module/contribution/manifest/registry/runtime wiring, workflow host/production adapter, `src-tauri/src/events/emitter.rs`, `src-tauri/src/commands/plugin.rs`, `src-tauri/src/lib.rs`, `src-tauri/build.rs`, main capability JSON and exact Rust ACL tests.
- Modify smoke Rust command inventories only as required to preserve its explicit surface; do not run native app or change its frontend test flow.

**Interfaces:**
- Consumes `WorkflowHost`, native AccountAuthority, `snapshot_locked`, `compute::sandbox::execute_json`, PluginRuntime operation/epoch/registry seams, production submit_once and submission recovery.
- Produces six exact IPCs, DTOs, v6 manifest/action, native wake signal and injectable StrategyStore/host described by spec. Task 2 consumes only those wire contracts; Task 3 consumes guest ABI. Publish any necessary seam adjustment to root before changing contract.

- [x] Step 1: Read spec and relevant existing interfaces; add failing tests for v6 acceptance, unknown fields, wrong ABI and rejection of strategy.run in v5. For example, independently constructed manifest v6 must round-trip and the same object with schemaVersion=5 must reject.

```rust
#[test]
fn v5_cannot_acquire_unattended_authority() {
    let mut manifest = strategy_manifest_fixture();
    manifest["schemaVersion"] = serde_json::json!(5);
    assert!(serde_json::from_value::<PluginManifest>(manifest).is_err());
}
```

- [x] Step 2: Run focused RED, implement closed contract/action dispatch preserving old canonical bytes, then run GREEN. Use `cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::strategy --lib --target-dir src-tauri/target`; retain expected failing output in report.
- [x] Step 3: Add fail-first native-loop tests using injected host and explicit temporary store. Specify observable mutation counts and journal state, not mock existence: launch one bounded guest, allow two changing snapshots, assert exactly one placement and cancellation; no confirm_workflow calls occur. Use paused Tokio time or Notify barriers instead of wall-clock sleeps.

```text
start(ticket, selected caps, symbol, parameters, mandatory policy, acknowledged=true)
advance one pass -> fresh snapshot -> Wasm output -> durable Pending -> place
assert host sees Pending and debited budget at dispatch
persist receipt+ownership -> acknowledge original submission -> next pass
owned active order snapshot -> cancel once -> persist accepted-request receipt
pause/stop -> advance -> mutation count unchanged
```

- [x] Step 4: Implement bounded store and supervisor modules to satisfy the exact spec. Use structural typed deserialization, action-specific ABI parsing, checked counters/Decimal, content+private-authority bindings, owned futures, monotonic cancellation admission, and fresh pre-dispatch checks. Build the production acknowledgment/reconciliation seam by reusing existing exact-match query logic; never acknowledge before durable ownership/receipt. Add internal native-only Notify wakeups to emitter, not a Tauri frontend event listener.
- [x] Step 5: Add targeted safety tests: empty/partial snapshots cannot establish ownership/terminal state; budget still applies when risk off; forbidden symbol/side/oversize/duplicate output; credential same-epoch reinstall; disable/reload; paused/stop during blocked compute and HTTP; dropped IPC; storage write/read corruption, conflicting generations, symlinks; restart pending not replayed; accepted-unack recovery; failed cancel not retried; resumed limits/state retained and mismatch denied; capacity/expiry/data staleness.
- [x] Step 6: Wire six commands to local main ACL and production host, including constructor safety (no automatic worker/start on app startup). Run focused plugin, submission, emitter and capability checks relevant to changed files, then coordinate one Rust plugin regression with root. Keep errors sanitized. Write report with RED/GREEN evidence, exact commands/results, files and limitations; request root-coordinated commit of Rust files only.

### Task 2: v6 import, launch authorization and strategy monitor

**Files:**
- Create `src/types/pluginStrategy.ts`, `src/services/pluginStrategyService.ts`, `src/components/plugins/PluginStrategyDialog.vue`, `src/components/plugins/PluginStrategyMonitor.vue`.
- Modify `src/types/plugin.ts`, `src/services/pluginService.ts`, plugin command/import helpers, PluginCard/PluginMarketplacePage/PluginCommandWorkbench and other direct plugin frontend consumers required for the new action.
- Add `tests/frontend/pluginStrategyService.test.ts`, `tests/frontend/pluginStrategyUi.test.ts`; extend touched manifest/import/workbench/lifecycle security tests to exact v6 and new command inventory.

**Interfaces:**
- Consumes the exact six IPCs, StrategyAccess/StartRequest/RunView/Receipt/Policy shapes from spec; v6 action and strategy.run. Do not change these independently of root.
- Produces launch intent/display controls compatible with current marketplace/workbench and an always-mounted plugin-page monitor. Existing v5 component remains unchanged in meaning.

- [x] Step 1: Write failing tests for strict service parsing/correlation and v6 import capabilities. Include copied v5 manifest with strategy.run rejection, unrecognized fields/status/counters, full requested-capability comparison on preview/commit, and ABI discrimination. Preserve old canonical test fixtures.

```ts
it('does not treat enabling or opening a strategy as permission to trade', async () => {
  const wrapper = await mountStrategyDialogWithAccess()
  expect(wrapper.findAll('input[type=checkbox]').every(box => !(box.element as HTMLInputElement).checked)).toBe(true)
  expect(wrapper.get('button[data-start-strategy]').attributes('disabled')).toBeDefined()
  expect(startRequests).toHaveLength(0)
})
```

- [x] Step 2: Implement strict wire service and domain types with bounded exact shapes, u64 string validation, decimal-string validation without Number conversion, allowed capabilities/policy. Start uses one-shot ticket and user acknowledgment. Correlate account/plugin and requestId; uncertain start outcome must refresh list rather than resubmit automatically.
- [x] Step 3: Write RED DOM tests for user-selected capabilities + policy + explicit automatic-trading acknowledgment, one start invocation, no per-order confirm command, modal closing preserving running monitor, pause/stop errors, and emergency stop while disconnected/plugin removed. Implement launch dialog and polling monitor with disposed/generation guards. Monitor handles statuses honestly and supports explicit reconcile and new-ticket resume with unchanged policy/counters.

```text
access -> all unchecked -> user chooses caps/policy -> explicit automatic-trading consent
click launch -> start_plugin_strategy once -> running status
close dialog -> list_plugin_strategies still shows run -> pause/stop controls work
IPC stop failure -> do not show stopped; refresh authoritative list
recoveryRequired -> reconcile only, never automatic resume
```

- [x] Step 4: Extend import preview and action descriptions to make automatic behavior conspicuous. Add strategy intent/call sites without conflating v5 grants. Show quantity-unit/gross-volume caps, remaining expiry, accepted-vs-filled difference, and no implicit exchange cancellation on stop. Ensure inactive plugin/account UI does not hide emergency controls.
- [x] Step 5: Run focused new/touched Vitest files, vue-tsc and scoped ESLint. Root handles full frontend CI. Report exact RED/GREEN evidence and touched files, then request root-coordinated frontend-only commit.

### Task 3: Executable strategy example and authoring/acceptance documentation

**Files:**
- Create `examples/plugins/threshold-strategy/{strategy.wat,manifest.json,build.mjs,verify.mjs,README.md}` and `src-tauri/examples/build_threshold_strategy_manifest.rs`.
- Create `docs/plugin-strategy-runtime.md`; update `docs/plugin-authoring.md`, `docs/plugin-local-manifests.md` and example indexes only where relevant.
- Create `docs/superpowers/verification/2026-09-22-plugin-strategy-runtime.md` with actual evidence as it becomes available.
- Root owns `.github/workflows/ci.yml`, Rust/frontend implementation and all commits/index writes; coordinate generator Cargo use first.

**Interfaces:**
- Consumes exact v6 manifest and strategy guest ABI from spec. Example must remain <=8,192 module bytes and <=16 KiB manifest; use existing pinned wat compiler without new deps.
- Produces reproducible checked-in manifest, synthetic Node guest verifier and exact user runbook; root uses verifier and source parity in CI.

- [x] Step 1: Build a synthetic verification harness first with hand-derived expected outcomes. Load packaged Wasm bytes with no imports and send actual encoded context/state/input. Cases: below threshold=>none, crossing threshold=>parameterized place, next native accepted receipt=>no duplicate place, visible owned active order=>cancel that exact orderId once, later state=>stop, malformed/oversized input fails safely.

```js
assert.equal(run(contextAt('99'), {threshold:'100', order:limitTemplate}).action.kind, 'none')
assert.deepEqual(run(contextAt('101'), params).action, {kind:'placeOrder',order:limitTemplate})
assert.equal(run(afterReceipt('owned-123'), params).action.order.orderId, 'owned-123')
```

- [x] Step 2: Implement readable small WAT using bounded parsing (reject unsupported formats rather than guessing), explicit state transitions and context/receipt inspection. Ensure copied input cannot inject unrelated action fields. Create fixed-path generator patterned after account-workflow example, then run source-to-Wasm parity check and synthetic verifier.
- [x] Step 3: Document import -> enable -> select current account/caps -> configure symbol, parameters and mandatory limits -> explicit real automatic-trading start -> monitor -> pause/stop -> reconcile -> explicit resume. Describe accepted vs filled, partial snapshots, quantity cap semantics, stop not cancelling orders, app/sleep limitations and no credential access. Clearly label execution demo, not investment advice or a proven profitable strategy.
- [x] Step 4: Coordinate with backend implementer for native injected-loop acceptance evidence; do not claim native pass before seeing it and do not launch production app. Keep verification doc honest about tests not run, initial failures and any remaining scope limits. Report generated sizes, commands/results, files, and request root-coordinated commit.

## Controller integration and final gate

- [ ] Read all task reports, inspect owned diffs and obtain fresh task-scoped reviews; return findings to original implementer with focused covering tests.
- [ ] Run final type/build and broad CI once at updated PR head; add source-parity verifier and nonzero strategy security filter to existing CI. Check ALL check-runs including CodeQL, not only the five CI jobs.
- [ ] Obtain whole-branch review on the most capable model. Verify startup wiring, authority, persisted pending actions, missing-data semantics, stop races and frontend visibility across task boundaries.
- [ ] Push only scoped code/tests/docs/examples, create and attach PR. Merge only if review and all checks are green, using expected head SHA. Keep root dirty checkout untouched and preserve isolated implementation/evidence.
- [ ] Final handoff states shipped behavior, exact verification, known limitations, PR state, no real-account test and no installer release. Collect ledger rulings with costs before removing this plan's scratch directory only.
