# Declarative Plugin Commands Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Deliver an importable local plugin whose enabled command buttons display host-rendered read-only information and disappear on revocation.

**Architecture:** Strict manifest v2 introduces a single host.showInfo contribution. Rust validates typed content and existing content-bound state; Vue derives action availability directly from confirmed catalog state. No new IPC or global command subsystem.

**Tech Stack:** Existing Rust/Serde/SHA-256, Vue 3/Pinia, Vitest. No dependencies added.

**Spec:** `docs/superpowers/specs/2026-09-15-plugin-declarative-commands-design.md`

## Global Constraints

- v1 remains metadata-only; v2 is localDeclarative-only, with 1–16 commands.
- Contribution keys exactly kind, contributionId, title, actionId, params; kind=command, actionId=host.showInfo; params keys exactly title,text.
- IDs use existing PluginId grammar; contribution IDs unique per plugin. Titles <=80 UTF-8 bytes, text <=2000 UTF-8 bytes, nonblank using existing whitespace rule.
- Reject unknown/duplicate fields, positional objects, unknown actions and nonempty capabilities. Render only ordinary text.
- Preserve v1 canonical bytes and state/ownership formats. Hash all v2 contribution semantics using existing local fingerprint envelope. Reject v2 builtins.
- No code execution, network, arbitrary navigation, new IPC, trading access, automatic import, background work, or dependencies.
- Commands/output require confirmed ready/available state and no in-flight or uncertain mutation; revoke on pending/failure/disable/removal/reload/content change. Failed toggle requires full snapshot confirmation before reuse.
- Preserve root checkout and prior worktrees. Test only focused changed boundaries and final frontend build; do not repeat whole suites or native UI.

---

### Task 1: Versioned Rust manifest and identity

**Files:**
- Create: `src-tauri/src/plugin/contribution.rs` for typed strict contribution validation and tests.
- Modify: `src-tauri/src/plugin/mod.rs`, `manifest.rs`, `record.rs`.
- Test/adjust if required: `src-tauri/src/plugin/import.rs`, `discovery.rs` and their test modules. No storage algorithm changes.

**Interfaces:**
- Produce `PluginManifest` common struct, `pub type PluginManifestV1 = PluginManifest` documented compatibility alias, `schema_version: u32`, `contributions: Vec<PluginCommandContribution>`.
- Produce serializable/deserializable `PluginCommandContribution` with `kind`, `contribution_id: PluginId`, `title: String`, `action_id`, `params: PluginInfoParams { title: String, text: String }`; closed enum literal serialization as specified in global constraints.
- Preserve current catalog/import transport versions and record API alias compatibility.

- [ ] Write one failing test parsing a real v2 manifest and an import prepared from it; assert declared command text remains intact, then run the focused filter before implementation. Use the literal contribution:

```rust
let contribution = r#"{"kind":"command","contributionId":"guide.overview","title":"Guide","actionId":"host.showInfo","params":{"title":"Guide","text":"Read-only guide"}}"#;
let json = format!(r#"{{"schemaVersion":2,"id":"com.example.guide","publisherId":"com.example","publisher":"Example","name":"Guide","description":"Read-only guide","version":"1.0.0","contributions":[{contribution}],"requestedCapabilities":[]}}"#);
let manifest: PluginManifestV1 = serde_json::from_str(&json).unwrap();
assert_eq!(serde_json::to_value(&manifest).unwrap()["contributions"][0]["params"]["text"], "Read-only guide");
```

- [ ] Implement typed map-only deserialization (reuse manifest's deserialize_object helper with crate visibility if appropriate); strict wire structs reject duplicate/unknown keys before normalization. validate() is also called for trusted constructors.

```rust
match self.schema_version {
    1 if self.contributions.is_empty() => {},
    2 if (1..=16).contains(&self.contributions.len()) => {
        // validate every contribution and reject duplicate contribution_id
    },
    _ => return Err("unsupported plugin manifest schema or contributions".into()),
}
```

- [ ] Type canonical record contributions; reject non-v1 builtins; preserve local digest domain/envelope. A golden v1 canonical byte/digest test must prove old identities stay unchanged. Assert reordered v2 nested keys yield same fingerprint, changed params yield a different fingerprint, and v2 builtins rejected.
- [ ] Cover v1 nonempty rejection, v2 empty/17 entries/duplicate IDs/unknown action/unknown nested field/duplicate nested key/array params/UTF-8 size limits/nonempty capabilities with compact table tests. Assert v2 is accepted by existing prepare/discovery boundary and remains disabled via existing state defaults.
- [ ] Run focused Rust plugin manifest/contribution/record/import/registry tests as necessary (one grouped plugin filter is acceptable), record actual RED/GREEN commands and output. Reuse existing target cache under root `src-tauri/target` if useful, with exclusive cargo execution. Format changed files only using rustfmt --edition 2021 --config skip_children=true, then commit explicit files.

### Task 2: Frontend command lifecycle, UI, example and docs

**Files:**
- Modify: `src/types/plugin.ts`, `src/services/pluginService.ts`, `src/stores/plugin.ts`.
- Create: `src/components/plugins/PluginCommands.vue` (plain text command buttons and result).
- Modify: `src/components/plugins/PluginCard.vue`, `PluginMarketplacePage.vue`, `PluginImportDialog.vue`, `PluginMarketplacePage.css`.
- Create: `examples/plugins/workspace-guide/manifest.json`, `examples/plugins/workspace-guide/README.md`.
- Update: `docs/plugin-local-manifests.md` (current scope only; retain historical safety details).
- Tests: `tests/frontend/pluginService.test.ts`, `pluginStore.test.ts`, `pluginMarketplacePage.test.ts`, `pluginImportDialog.test.ts`, new `pluginCommands.test.ts` as needed.

**Interfaces:**
- Consume Task 1 manifest JSON with schema 1 or 2 and exact closed contribution fields. Keep `PluginManifestV1` narrow; add `PluginManifestV2` and `PluginManifest` union; item/import preview use union.
- Store expose computed `commandsAvailable` and synchronous `runCommand(pluginId: string, contributionId: string): PluginCommandInfo | null`, where info contains pluginId, pluginName, contributionId, title, text. Also expose a reactive command context key including current revision/generation and availability for UI revocation.
- `PluginCommands` receives plugin and store gate/context, or accesses store by established project pattern; render only enabled commands and keep active result keyed to current authorization context. Market never mounts command executor.

- [ ] Add failing service v2 round-trip and real-store/UI flow tests first. Mock only IPC service boundary; keep real Pinia and component behavior. Required flow:

```ts
// A literal v2 snapshot is returned by the mocked transport.
await store.load()
expect(store.runCommand('com.example.guide', 'guide.overview')).toBeNull()
await store.setEnabled('com.example.guide', true)
expect(store.runCommand('com.example.guide', 'guide.overview')?.text).toBe('Read-only guide')
// Start disable with deferred IPC: command and already visible result disappear immediately.
// Reject disable: remain unavailable until an explicitly loaded full snapshot confirms state.
```

- [ ] Strictly parse v2 command objects in service, preserving normalized key order, IDs, text, and all exact size/uniqueness rules. Reject v2 builtin item and reserved actions/capabilities. Fix cloneCatalogItem so typed contribution parameters survive without mutable aliasing. Update immutable comparison comment/logic to support normalized nested values.
- [ ] Implement store availability and command resolution from current catalog, not a persistent registry. Include loading/error/reloadError, import/removal busy/unknown, pending toggle, and unconfirmed failed-toggle guards. Clear uncertainty only after a confirmed full snapshot (not an arbitrary stale response). If required, use an internal monotonic command epoch to prevent replay after pending transitions. Keep API additions minimal.
- [ ] Build PluginCommands with ordinary buttons, text-only result, plugin attribution, close button and aria-live. Reset displayed result whenever gate/context/plugin changes; no automatic restore. Check synchronously again in click handler. Update card/import copy conditionally for v1 versus v2 and preview command names. Keep market listing nonexecutable.
- [ ] Add the two-command example JSON and concise Chinese README with manual import steps and limitations; update current local-manifest docs for v2. Do not insert production builtins.
- [ ] Cover service rejection, store contribution retention and unknown-result fail-closed behavior; actual DOM click shows escaped HTML text, disable removes output, re-enable requires click, market cannot run commands. Reuse fixtures rather than copy huge store setups.
- [ ] Run focused changed frontend tests, scoped ESLint, `vue-tsc --noEmit` and `vite build` once at end. Record command output and any baseline warnings. Commit explicit task files. Do not claim native UI/whole-app validation.
