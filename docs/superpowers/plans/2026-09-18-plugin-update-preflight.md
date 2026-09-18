# Plugin Update Preflight Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver an advisory same-ID manifest comparison in the existing import preview while retaining duplicate-import rejection.

**Architecture:** Rust captures catalog comparison with the preview generation; the strict frontend parser validates it; a pure semantic diff and presentation component explain changes. Both dialog and store reject attempts to import existing IDs. Real update/rollback transactions are separate work.

**Tech Stack:** Existing Rust/Serde/semver, Vue 3/Pinia/TypeScript/Vitest, Tauri 2. No dependencies added.

**Spec:** `docs/superpowers/specs/2026-09-18-plugin-update-preflight-design.md`

## Global Constraints

- Only ready import previews become schemaVersion 2 with a required assessment; cancelled/cancel stay v1, commit stays v2, catalog/manifest/state/ownership versions unchanged.
- Assessment is advisory captured-catalog comparison, never update authorization. notInCatalog does not mean safe to install. existingId always disables import confirmation and store commit; backend duplicate rejection stays unchanged.
- No new IPC, ACL, dependency, network, credentials, paths/receipts/slots/file identities in responses, account access or trading action. No storage/discovery/removal/import-commit algorithm changes.
- Preserve one-shot token expiry, source capture, generation/revision/unknown-result/command-authority and UI lifecycle gates. Snapshot and assessment are captured from one post-picker registry read.
- Existing get_catalog reconciliation may write; only the new comparison performs no lifecycle writes. Do not claim the entire prepare path is zero-write.
- Never launch production app or access real AppData/keyring; preserve root checkout and sibling worktrees. No repeated whole suites and no historical OS 5 fix claim.

## Task 1: Captured catalog assessment and comparison UI

**Files:**
- Create: `src-tauri/src/plugin/import/assessment.rs` (pure DTO/version assessment with unit tests).
- Modify: `src-tauri/src/plugin/import.rs`, `import/session.rs`, `import/tests.rs`, `runtime/import.rs`, targeted `runtime/import/tests.rs` and directly affected command tests only.
- Create: `src/services/pluginManifestDiff.ts`, `src/components/plugins/PluginManifestComparison.vue`, `tests/frontend/pluginManifestDiff.test.ts`, `tests/frontend/pluginImportAssessment.test.ts`.
- Modify: `src/types/plugin.ts`, `src/services/pluginService.ts`, `src/stores/plugin.ts`, `src/components/plugins/PluginImportDialog.vue`, `PluginMarketplacePage.css` if needed.
- Extend existing service/dialog/store tests; update only ready-preview fixtures in affected plugin tests, preserving unrelated cancelled/envelope schema assertions.
- Controller owns spec/plan, `docs/plugin-authoring.md`, `docs/plugin-local-manifests.md`, `examples/plugins/workspace-shortcuts/README.md` and new `update-candidate.json`; do not edit them without coordinating.

**Interfaces (exact public wire names):**

```typescript
type ImportAssessment =
  | { kind: 'notInCatalog' }
  | { kind: 'existingId'; current: PluginCatalogItem;
      versionRelation: 'incomingLower' | 'samePrecedence' | 'incomingHigher' }
// ReadyLocalManifestImport: previous fields, schemaVersion: 2, assessment: ImportAssessment.

type PluginManifestField = 'schemaVersion' | 'publisherId' | 'publisher' | 'name'
  | 'description' | 'version' | 'requestedCapabilities'
interface PluginManifestDiff {
  changedFields: PluginManifestField[]
  added: PluginCommandContribution[]
  removed: PluginCommandContribution[]
  changed: { before: PluginCommandContribution; after: PluginCommandContribution }[]
  orderChanged: boolean
  sameContent: boolean
  publisherIdChanged: boolean
}
// Export from pluginManifestDiff.ts:
function comparePluginManifests(current: PluginManifest, incoming: PluginManifest): PluginManifestDiff
```

The pure diff expects parsed same-ID manifests; defensively reject mismatched IDs (throw), never silently compare unrelated plugins. Compare params structurally by action-specific fields, not raw property order. Do not duplicate semver parsing in TS: versionRelation is a closed backend-authored advisory enum; strict parser validates that enum and matching current ID, not a second independent semver implementation.

Rust interface may use `ImportAssessment::from_catalog(incoming: &PluginManifest, catalog: &[PluginCatalogItem])` and `PrepareLease::publish(content, generation, catalog, now)`. Constructor finds matching ID itself, clones only the matching item, and computes incoming.version.cmp_precedence(&current.manifest.version). Runtime passes `snapshot.plugins` and generation captured by its existing single post-picker read lock. No additional scan/storage calls, no public command changes.

- [ ] **Step 1: RED protocol.** Add production-path serializer tests for missing assessment/new ready schema and pure assessment tests using literal expected wire JSON; runtime test returns comparison from the snapshot published while a picker was waiting. Tests must fail on current schema 1/missing comparison before implementation. Existing token/session tests retain exact 300 second/replay behavior.

```rust
// Hand-derived essential assertions on the public prepare result:
assert_eq!(wire["schemaVersion"], 2);
assert_eq!(wire["assessment"]["kind"], "existingId");
assert_eq!(wire["assessment"]["versionRelation"], "samePrecedence");
assert_eq!(wire["assessment"]["current"]["manifest"]["version"], "1.0.0+old");
// Incoming 1.0.0+new is samePrecedence but remains a content change in the UI.
```

Use available Rust cache, serially:
`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib plugin::import::tests --target-dir D:/EasiFlux/EasiFlux-Desktop-Tauri/target/worktrees/plugin-local-manifest-removal/src-tauri/target`

- [ ] **Step 2: GREEN backend and strict service.** Implement closed assessment DTO and attach same-snapshot current item to schema-2 ready preview. Keep cancel and commit envelopes unchanged. Test notInCatalog, built-in/external/managed/status conflicts, incoming numeric/pre-release/build precedence and zero new stage/persist calls in an initialized fake runtime. Keep direct duplicate commit rejection unchanged and run its existing targeted test. Frontend uses requireExactObject + existing parseCatalogItem; reject arrays, unknown/missing keys, unknown kind/relation, wrong ready schema and mismatched manifest IDs. Update ready fixtures across affected tests; never loosen exact-object parsing to avoid fixture fixes.

```typescript
const parsed = await prepareLocalManifestImport()
expect(parsed).toEqual({ schemaVersion: 2, status: 'ready', token: 'a'.repeat(32),
  expiresInSeconds: 300, catalogGeneration: '1', manifest: incoming,
  assessment: { kind: 'notInCatalog' } })
```

- [ ] **Step 3: RED/GREEN semantic diff and UI.** Literal fixtures cover metadata fields, publisherId vs display name, same-version changed text, same semver precedence with different build strings, v1/v2/v3, added/removed/changed actions+params, keys reordered (same), and relative order of common command IDs. Data never mutates. Show plain-text before/after metadata and command details with host-owned navigation labels. Use bounded native details for long command text and a clear unchanged-content state. Current source/management/status uses existing presentation helpers; no trust badge.

```typescript
expect(comparePluginManifests(before, after).changed.map(x => x.after.contributionId))
  .toEqual(['workspace.charts']) // destination change charts -> home
expect(comparePluginManifests(before, after).publisherIdChanged).toBe(false)
// Real service + store, only native invocation mocked:
await store.prepareImport()
await store.commitImport()
expect(nativeCallsFor('commit_local_manifest_import')).toHaveLength(0)
```

`PluginImportDialog` renders the comparison for existingId, labels the dialog as comparison and cancel as close, clearly says no overwrite/update/rollback; disables confirm and guards requestConfirm independently. `store.commitImport` also rejects existingId before setting committing, mutating authority, or issuing IPC. No new update/rollback button or chaining remove/import. notInCatalog retains new import, with an advisory copy. Existing stale/expiry/lifecycle messages remain. The real integration test must test click and direct store commit rejection (native-boundary call count is meaningful here), cancel releases token and does not change plugin state, newId can still commit and stale cannot; no mocking store methods.

- [ ] **Step 4: Focused verification and example.** Controller adds update-candidate.json; include it in a real Rust PreparedManifest parser fixture test. Run relevant frontend files once after GREEN (new diff/assessment, pluginService, pluginImportDialog, pluginStore, pluginMarketplacePage, pluginCommands; navigation if affected). Typecheck, scoped ESLint, Vite build once; scoped rustfmt and only affected Rust modules/tests. Existing warnings are reported, not fixed out of scope. No native/production launch.

```powershell
& 'C:/Program Files/nodejs/node.exe' D:/EasiFlux/EasiFlux-Desktop-Tauri/node_modules/vitest/vitest.mjs run tests/frontend/pluginManifestDiff.test.ts tests/frontend/pluginImportAssessment.test.ts tests/frontend/pluginService.test.ts tests/frontend/pluginImportDialog.test.ts tests/frontend/pluginStore.test.ts tests/frontend/pluginMarketplacePage.test.ts tests/frontend/pluginCommands.test.ts
& 'C:/Program Files/nodejs/node.exe' D:/EasiFlux/EasiFlux-Desktop-Tauri/node_modules/vue-tsc/bin/vue-tsc.js --noEmit
& 'C:/Program Files/nodejs/node.exe' D:/EasiFlux/EasiFlux-Desktop-Tauri/node_modules/vite/bin/vite.js build
git diff --check
```

Use normal Vite config loader (existing __dirname), approved elevated lane for dependency .vite-temp writes; no config-loader edits. Node modules junction is already prepared; no install.

- [ ] **Step 5: Self-review, explicit commit, report.** Commit only implementation/tests; report RED/GREEN commands and output, changed paths, limitations and warning sources in task-1-report.md. Source/example integration must be complete before controller publication. Do not push/merge/release or spawn subagents; controller reviews.

## Controller handoff

- [ ] Update author/local-manifest docs and same-ID update example; clearly separate comparison from actual update/rollback.
- [ ] Task-scoped review, then whole-branch review; record evidence and final merge baseline.
- [ ] Publish source/tests/docs/examples to the already authorized repository as a new PR, without assuming the authorization to merge PR #37 automatically covers this new PR.
- [ ] Report PR #37 merged and this actual next-phase scope, limitations and validation.
