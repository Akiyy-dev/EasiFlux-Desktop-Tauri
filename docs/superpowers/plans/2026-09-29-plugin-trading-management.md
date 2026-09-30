# Plugin Trading Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give autonomous plugins owned-order amendment and native protected entry without bypassing authorization, risk, or recovery.

**Architecture:** Extend the existing native-supervised strategy pipeline with manifest v7 opt-in capabilities and two strict action variants. Preserve v1-v6, canonical durable submissions, native admission, and the existing UI launcher/monitor.

**Tech Stack:** Rust/Tauri/Wasmi, Vue/TypeScript, Vitest, Node fixture verifier; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-29-plugin-trading-management-design.md`

## Global Constraints

- Manifest v7 alone adds `trade.amend` and `trade.protect`; v1-v6 remain compatible and cannot request new capabilities.
- `trade.amend` requires `trade.place`, `orders.read`, and `market.read`; `trade.protect` requires `trade.place` and `market.read`.
- Catalog transport version 3 and strategy IPC schema 1 remain unchanged.
- Existing caps remain: interval 5000-60000 ms, lifetime 60-86400 seconds, 1-1000 actions, mandatory positive maxOrderQty/maxTotalQty, maxOrderQty <= maxTotalQty.
- Amendment is owned normal active Limit orders only, new qty <= current qty and > filled qty; full new qty debits cumulative budget.
- Protected entry is a single opening create-order request; accepted is acknowledgement, not proof of active protection.
- No credentials, authenticated network, real account, installed profile, app launch or exchange mutation in verification.
- No new dependencies, arbitrary network/WASI, new Tauri permission broadening, or unrelated root edits.
- User delegated routine design/plan/execution choices; focused tests during implementation and consolidated broad tests at integration.

## Review Focus

- An amended order can fill while preparation awaits: reject stale/non-open/increased targets and never send cancel/place fallback (Task 1).
- A stopped strategy can regain a queued hand-off: assert zero mutation when final admission is revoked during native preparation (Task 1).
- Protected payload can be silently dropped by mapping or legacy journal comparison: assert exact protected body and failed reconciliation without required raw fields (Task 1).
- Old v6 plugin or durable record can accidentally gain amendment authority: enforce version/grant dependencies and no automatic recovery admission (Tasks 1-2).
- A UI receipt can imply completed amendment or active protection: explicitly render request acknowledgement and keep resume/stop semantics (Task 2).

---

### Task 1: Native supervised trading-management pipeline

**Files:**
- Modify: `src-tauri/src/plugin/manifest.rs`, runtime strategy capture/version checks and native manifest tests.
- Modify: `src-tauri/src/plugin/strategy/{contract,types,execution,store,recovery}.rs`, focused tests under `strategy/tests/`.
- Create: `src-tauri/src/plugin/strategy/management.rs` for strict new action DTOs and semantic validation; register in `mod.rs`.
- Modify: `src-tauri/src/plugin/workflow/{host,production}.rs`, `workflow/production/strategy.rs` and tests.
- Create: `src-tauri/src/plugin/workflow/production/management.rs` for authoritative amendment preparation/ack/protection recovery helpers and focused tests.
- Modify: `src-tauri/src/models/trading.rs`, API request/mapping/private adapter, `services/trading.rs` and its `strategy.rs`, submission canonical validation as needed.
- Mechanical: update existing Rust request literals for optional native protection without altering legacy test expectations.

**Interfaces:**
- Produces strict `AmendProposal { symbol:String, order_id:String, price:String, qty:String }` and native `OrderProtection { take_profit:Option<String>, stop_loss:Option<String>, trigger_by:String }`.
- Produces `StrategyAction::AmendOrder { order:AmendProposal }` and `StrategyAction::PlaceProtectedOrder { order:PlaceProposal, protection:OrderProtection }`.
- `ReceiptKind` adds corresponding camelCase variants; existing receipt shape is retained.
- `PlaceOrderRequest` adds `protection:Option<OrderProtection>` with default/skip-if-none; old journal data and ordinary payloads remain readable/unchanged.
- `OwnedOrder` adds default/skip-if-none `placement:Option<PlaceOrderRequest>` for canonical original placement identity; legacy absent metadata never authorizes amendment. Validate metadata against owned submission ID, run symbol/capabilities and reduce-only constraints when loading.
- New `WorkflowHost::strategy_amend_locked(context:SessionContext, request:AmendProposal, original:PlaceOrderRequest, admission:&StrategyAdmission)->HostFuture<OrderAcknowledgement>` and a narrow protected-placement/ack seam if the existing placement seam cannot retain its canonical request. The typed acknowledgement carries identity, not a fabricated observed order. Default trait implementation denies unavailable. Production preparation verifies raw target metadata against original immutable identity before native risk and final hand-off. No extra Tauri command is required.

- [x] **Step 1: Add failing behavior tests and observe RED.** Existing serde should reject a valid v7 manifest and protected/amend guest output before implementation. Add supervisor injected-host tests proving exact new dispatch, no unauthorized dispatch, no-op/increase/filled/foreign target rejection, conservative debit, stop during preparation, unknown/restart recovery. Add literal real API fixtures for protected create payload, strict ack, desired amend-state reconciliation, missing/conflicting raw protection fields and ordinary backward compatibility.
- [x] **Step 2: Implement contract and native request mapping.** Follow the spec's exact JSON and constraints; reject duplicate/unknown fields. Keep workflow v5 PlaceProposal unchanged. New permissions must be checked at manifest and start/resume. Native protection must survive submission serialization and map to take_profit/stop_loss and each specified leg's trigger selector.
- [x] **Step 3: Implement durable supervised actions and production host.** Extend all exhaustive action/receipt/store validation and identity paths; debit before dispatch. Reconstruct amendment immutable fields from a strict exact raw query; apply existing native risk path to amended effective order, never bypass it via raw command. Keep fresh quote/protection checks and final admission after awaits. Unknown calls never retry; read-only reconciliation validates desired state and exact IDs. Existing placement/cancel paths must retain behavior.
- [x] **Step 4: Run focused GREEN and inspect the diff.** Run `cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::strategy --lib --target-dir src-tauri/target` and the focused native adapter/mapper/submission/trading filters covering changed behavior. Consolidated full Rust suite belongs to final integration. Use scoped rustfmt, not repository-wide formatting debt cleanup.
- [x] **Step 5: Commit and report evidence.** Commit only Task 1 files. Include fail-first commands/output, final focused results, changed files, residual warnings, and exact interfaces for frontend/example. Report concerns instead of inventing exchange response semantics.

### Task 2: v7 frontend authorization and execution feedback

**Files:**
- Modify: `src/types/plugin.ts`, `src/types/pluginStrategy.ts`, `src/services/pluginService.ts`, `src/services/pluginStrategyService.ts`.
- Modify: `src/stores/plugin.ts` for typed catalog cloning and strategy launch/version gates.
- Modify: `src/components/plugins/PluginStrategyDialog.vue`, `PluginStrategyMonitor.vue`, `pluginPresentation.ts`, manifest diff/import presentation where required.
- Modify where v6-only gates occur: `PluginCard.vue`, `PluginManifestComparison.vue`, `PluginImportDialog.vue`, `PluginMarketplacePage.vue` in the same component directory.
- Modify: `tests/frontend/pluginService.test.ts`, `pluginStrategyService.test.ts`, `pluginStrategyUi.test.ts`, and `pluginManifestDiff.test.ts` where their behavior changes.
- Modify affected store/card/import/workbench tests to prove v7 is reachable through the actual launcher, not only accepted by a parser.

**Interfaces:**
- Consumes Task 1 manifest v7, capability names/dependencies and two new receipt kinds; no IPC changes.
- Produces `PluginManifestV7`, strict catalog/import/strategy response parsing and explicit unchecked new permission controls.

- [x] **Step 1: Write and run focused RED.** Test v7 import/catalog acceptance, v6 rejection of new capabilities, missing grant dependencies, wrong receipt submission/order identities, and UI new permissions initially unchecked. Include a legitimate immutable resume and a mismatched grant response.
- [x] **Step 2: Implement v7 through existing flows.** Share dependency validation where appropriate without loosening strict unknown-field/correlation parsing. Add Chinese descriptions explaining owned non-increasing amendment and attached protection. Prevent invalid grants at launch and resume. Render new receipt kinds as accepted requests, never completed trading/protection.
- [x] **Step 3: Run GREEN/typecheck/lint.** Run affected Vitest files, `node_modules/.bin/vue-tsc.cmd --noEmit` and scoped ESLint; no full suite per edit. Verify existing v6 launch/monitor/stop behavior.
- [x] **Step 4: Commit and report evidence.** Only frontend/test files, commands and results; keep new operations inaccessible without explicit grants.

### Task 3: Executable management example and authoring handoff

**Files:**
- Create: `examples/plugins/managed-entry/{README.md,strategy.wat,manifest.json,build.mjs,verify.mjs}`.
- Create: `src-tauri/examples/build_managed_entry_manifest.rs` following the existing fixed-path locked generator.
- Reuse the existing single-strategy generator logic; if needed, extract a small helper under `src-tauri/examples/support/` and make the existing threshold generator a fixed-constant caller. Preserve its output bytes and allowed CLI flags; do not add arbitrary user-selected paths or refactor the separate account-workflow generator.
- Modify: `src-tauri/src/plugin/strategy/tests/supervisor.rs` or a small registered example-test module for actual packaged Wasmi execution.
- Modify: `docs/plugin-strategy-runtime.md`, `docs/plugin-authoring.md`; create `docs/superpowers/verification/2026-09-29-plugin-trading-management.md`.
- Modify: existing CI generator/example verification step to include the new fixture without new dependencies.

**Interfaces:**
- Consumes exact Task 1 JSON action/receipt and Task 2 manifest v7 capability contract.
- Example input is `{ "symbol":"BTCUSDT", "qty":"0.001", "entryPrice":"50000", "amendPrice":"50010", "takeProfit":"55000", "stopLoss":"45000" }`.
- Example lifecycle: one protected Buy Limit entry at start, wait for matching accepted protected receipt and currently visible owned open order, amend once without increasing qty, then stop after matching accepted amend receipt. Unknown/rejected/malformed/missing data never provoke retries or unrelated trades. It must respond to input/state/receipts, not emit a canned action.

- [x] **Step 1: Add verifier before generated artifact and observe failure.** Instantiate actual Wasm and call run with literal synthetic contexts. Cases cover input dependence, first protected action, ownership/missing-visible wait, exactly one amendment, rejected/unknown receipt and normalized persisted state ordering.
- [x] **Step 2: Implement bounded WAT and deterministic generator.** Reuse import-free native subset: no bulk-memory, at most32 data segments, module8192 bytes, manifest16KiB, state/input4096 bytes. Preserve v6 example. Do not open app or use account data.
- [x] **Step 3: Verify generated parity and actual native loop.** `node examples/plugins/managed-entry/verify.mjs`; `node examples/plugins/managed-entry/build.mjs --check`; focused packaged management Wasmi supervisor test must observe exactly one protected place and one amend, canonical values and completion. Use synthetic host/temp storage only.
- [x] **Step 4: Document capabilities, limits and verification truthfully.** Distinguish request acknowledgement from active protection; explain quantity debit, native order IDs, unresolved outcomes, unsupported position TP/SL/leverage/margin and no live acceptance. Link checked public API references from spec.
- [x] **Step 5: Commit and report scoped evidence.** Commit Task 3 files and report actual packaged verification, source parity and native-loop evidence. The controller owns the consolidated integration gate below after task review, so the implementer does not duplicate the full suites.

## Controller integration gate

After Task 3 review, run the full frontend suite/typecheck/lint/build and full locked Rust suite once, all three packaged verifiers (account-workflow, threshold-strategy, managed-entry), their source-parity generators and whitespace checks. Record exact counts/warnings in the verification document. Independent whole-branch review and exact-head CI precede integration; publication/merge use standing authorization and never include local profiles or secrets. Fixes run their covering tests; do not repeat a green whole-suite run without a concrete reason.
