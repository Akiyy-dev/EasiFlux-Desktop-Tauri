# Plugin Host Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a complete manifest-v3 local plugin workflow that opens allowlisted host pages only on explicit user clicks.

**Architecture:** Preserve all existing lifecycle gates and v1/v2 identities. Rust validates declarative data; the frontend projects commands; AppShell re-resolves identity and owns page navigation. The implementation is one vertical slice with one independently reviewable completion gate; author documentation is prepared by the controller alongside it.

**Tech Stack:** Existing Rust/Serde, Vue 3, Pinia, TypeScript, Vitest, Tauri 2. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-18-plugin-host-navigation-design.md`

## Global Constraints

- Only six destination strings: `home`, `trading`, `charts`, `settings.general`, `settings.notifications`, `settings.about`.
- Manifest v1 and v2 acceptance and canonical fingerprints remain unchanged; only v3 adds `host.openPage`. v3 accepts 1–16 commands and zero capabilities; builtIn remains v1 only.
- No new IPC, ACL grants, dependency, arbitrary URL/path/script, account data access, setting mutation, or trade submission.
- Preserve existing unknown-result, loading, mutation, identity, generation and revision authorization gates. Click-time identity lookup is mandatory in both the UI command path and AppShell.
- navigationAvailable defaults false; normal AppShell opts in. Plugin-only smoke must not navigate.
- Never launch the production app or access real AppData/keyring. Do not modify root checkout or sibling worktrees. No full-suite repetition or historical OS 5 fix claim.

## Task 1: Manifest v3 to safe host navigation vertical slice

**Files:**
- Modify: `src-tauri/src/plugin/contribution.rs`, `manifest.rs`, `registry.rs` and only directly affected plugin unit-test modules.
- Modify: `src/types/plugin.ts`, `src/services/pluginService.ts`, `src/stores/plugin.ts`.
- Create: `src/services/pluginNavigation.ts`.
- Modify: `src/composables/usePluginCommandResult.ts`, `src/components/plugins/PluginCommands.vue`, `PluginCommandWorkbench.vue`, `PluginMarketplacePage.vue`, `PluginImportDialog.vue`, `PluginCard.vue` (copy only if needed), `src/components/layout/AppShell.vue`.
- Test: extend relevant `tests/frontend/plugin*.test.ts`; add `tests/frontend/pluginNavigation.test.ts` with real shell/plugin components and mocked external I/O. Reuse existing notificationNavigation test isolation patterns where applicable.
- Controller owns: design/plan, author docs and `examples/plugins/workspace-shortcuts/`; do not edit these without coordinating.

**Interfaces:**
- Consumes current catalog snapshots, import preview and lifecycle gate unchanged.
- Produces `PluginPageDestination` union; `PluginManifestV3`; discriminated `PluginCommandContribution` and `PluginCommandExecution`.
- `runCommand(pluginId: string, contributionId: string): PluginCommandExecution | null` where showInfo wraps existing Info in `info`; openPage carries identity and destination.
- `PluginCommandSummary` carries actionId and, for openPage, destination. Keep all existing identity/title fields.
- UI emits `open-page` with `{pluginId: string; contributionId: string}` only. `PluginMarketplacePage` forwards it to AppShell. Navigation-enabled prop passes from shell through page to command components/composable; default false.
- `pluginNavigation.ts` exports `pluginPageLabel(destination: PluginPageDestination): string` and `pluginNavigationTarget(destination: unknown): NavigationTarget | null`. Runtime invalid inputs return null. Labels: 首页、交易页、图表工作区、通用设置、通知设置、关于.

- [x] **Step 1: RED protocol and frontend parser checks.** Add raw JSON tests for accepted v3 navigation/mixed commands and rejected v2 navigation, v3 builtIn, unknown targets, mixed params, positional arrays, duplicate fields and non-string enums. Add one content-identity test showing destination edits revoke enablement, plus a literal v2 canonical-bytes regression. Frontend service checks use the existing real service with only tauriInvoke mocked.

```json
{"kind":"command","contributionId":"workspace.charts","title":"打开图表","actionId":"host.openPage","params":{"destination":"charts"}}
```

Run affected new tests and retain the expected rejection of schemaVersion 3 as RED before implementation. Use existing cached dependencies only. Rust command:

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib plugin::contribution::tests --target-dir D:/EasiFlux/EasiFlux-Desktop-Tauri/target/worktrees/plugin-local-manifest-removal/src-tauri/target
```

- [x] **Step 2: Implement typed protocol and parser.** Extend typed action/params with strict object visitors and action-parameter pairing. Do not deserialize raw JSON into a Value/Map that silently drops duplicate fields. Existing showInfo serializes exactly as before. Manifest validate enforces action by version; every builtIn gate rejects non-v1. Extend frontend parser with exact object keys, version-specific action validation and normalized parameter order. Keep transport/state schemas unchanged. Run focused protocol tests once GREEN, then affected registry test.

- [x] **Step 3: RED then implement execution and UI.** Test current service/store gating, action/target display, real card/workbench clicks and shell navigation, plus disabled/unknown/stale rejection. Use literal expectations, e.g. `pluginNavigationTarget('settings.notifications')` is `{page:'settings',settingsSection:'notifications'}`, while `https://example.com`, `settings.account`, arrays and objects return null. A shell test must exercise the real plugin page/store rather than merely emitting a fake event from a stub; mock only native invoke and unrelated account/chart/provider components.

```typescript
function handlePluginOpenPage(intent: { pluginId: string; contributionId: string }): void {
  const command = pluginStore.runCommand(intent.pluginId, intent.contributionId)
  if (command?.actionId !== 'host.openPage') return
  const target = pluginNavigationTarget(command.destination)
  if (target) void navigateTo(target)
}
```

Update store clone/projection for v3 and preserve the existing gate. Update Info consumers for the execution union without changing the Info render model. Navigation intents carry only identity. A no-navigation host disables nav controls and explains why. All visible labels distinguish display-info from page-navigation; import preview shows host-owned destination labels. Do not introduce automatic execution, global listeners, generic dynamic dispatch or direct business-store calls. Existing showInfo lifecycle tests must remain meaningful, not be deleted.

- [x] **Step 4: GREEN and final focused verification.** Run affected frontend tests, then the related plugin regression once; typecheck, scoped ESLint and Vite build once after final changes. Run only affected Rust modules, serially. Controller prepares node_modules junction to the existing root dependency directory. Commands from this worktree:

```powershell
& 'C:/Program Files/nodejs/node.exe' D:/EasiFlux/EasiFlux-Desktop-Tauri/node_modules/vitest/vitest.mjs run tests/frontend/pluginNavigation.test.ts tests/frontend/pluginCommands.test.ts tests/frontend/pluginCommandWorkbench.test.ts tests/frontend/pluginService.test.ts tests/frontend/pluginImportDialog.test.ts tests/frontend/pluginMarketplacePage.test.ts
& 'C:/Program Files/nodejs/node.exe' D:/EasiFlux/EasiFlux-Desktop-Tauri/node_modules/vue-tsc/bin/vue-tsc.js --noEmit
& 'C:/Program Files/nodejs/node.exe' D:/EasiFlux/EasiFlux-Desktop-Tauri/node_modules/vite/bin/vite.js build
git diff --check
```

Scoped ESLint uses the root `node_modules/eslint/bin/eslint.js` with the changed frontend files. Treat existing warnings separately; do not modify unrelated formatting or dependency locks.

Use the normal bundled config loader: existing Vite/Vitest configs use `__dirname`, which is not defined by runner mode. If the sandbox rejects the existing dependency cache `.vite-temp` write, use the approved escalated test lane instead of changing product configuration.

- [x] **Step 5: Self-review, commit and report.** Commit only explicit implementation/test files. Record RED and GREEN commands/output, changed paths, limitations and any concern in the task report. No push, merge, release or native app launch by the implementer. Controller independently reviews the diff, adds author documents, and performs the whole-branch integration check before PR publication.

## Controller handoff

- [x] Add sample v3 manifest with three navigation targets and one showInfo, README and author guide; preserve the existing v2 example.
- [x] Check example with the actual Rust manifest deserializer through a targeted test or existing validated fixture path, not a second ad hoc validator.
- [x] Review implementation against this spec and re-check authoritative main before publication.
- [ ] Publish plugin-related source/tests/docs/examples to the already authorized repository as a PR; do not auto-merge.
- [ ] Report the actual completed phase and remaining ecosystem milestones without claiming the whole ecosystem is complete.

## Verification and review record (2026-09-18)

- Implementation: `aeb9a67`; author documentation/example: `6e6bdf9`; review wording correction: `87ec232`.
- Related frontend regression: 8 files / 590 tests passed. Controller independently repeated only the critical navigation integration file: 20/20 passed.
- Targeted Rust checks: manifest 20, contribution 6, record 6, discovery compatibility 1, destination-identity revocation 1; 34 passed in total. The actual shipped v3 example is a deserialization fixture.
- Typecheck, scoped ESLint/rustfmt, Vite build and Git whitespace checks passed. Existing Rust dead-code warnings and Vite's large-chunk advisory remain.
- Task review approved after one documentation-only correction; whole-branch review of `c177b236..e5d8393` approved with no remaining findings. No product source changed afterward.
- Main was verified through the GitHub branch API at `c177b236`. This branch preserves unrelated root-worktree changes and contains no dependency, IPC, or ACL modifications.
- No production-app/native-window acceptance test, real AppData/keyring access, or account/trade request was performed. This is not a claim that the shipped installer has been manually verified.
- Published v0.6.1 does not support manifest v3. A build containing this phase is required; release versioning and publication are separate.
- This phase completes controlled local navigation commands and author examples, not online marketplace, update/rollback, signing, or script execution.
