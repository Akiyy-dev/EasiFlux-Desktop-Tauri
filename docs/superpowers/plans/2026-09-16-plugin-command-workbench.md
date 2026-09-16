# Plugin Command Workbench Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an installed-only searchable workbench for currently enabled read-only plugin commands.

**Architecture:** Derive summaries from the existing Pinia catalog and its fail-closed authority. Share one result lifecycle and renderer between existing cards and the new workbench; page orchestration selects one installed view at a time.

**Tech Stack:** Existing Vue 3, TypeScript, Pinia and Vitest; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-16-plugin-command-workbench-design.md`

## Global Constraints

- Base main is `a5eeabe`; work only in `plugin/command-workbench`.
- No new Rust, IPC, manifest version, permissions, dependencies, network, downloads, scripts, routes or global hotkeys.
- Reuse existing `commandsAvailable`, `commandContextKey` and `runCommand`; preserve epoch/generation/revision/request-ownership algorithms.
- Command identity is the pair `pluginId + contributionId`; cached summaries never authorize execution.
- All plugin text is plain text; no HTML/Markdown interpretation and no URL activation.
- No persistent command registry, localStorage, history, favorites or automatic execution.
- Installed-only workbench; manage keeps card commands, market has no command execution.
- Focused tests only; final related frontend regression, typecheck, scoped lint and build once. No Rust/native UI rerun or OS 5 fix claim.

---

### Task 1: Shared command projection and result lifecycle

**Files:**
- Modify: `src/types/plugin.ts`, `src/stores/plugin.ts`, `src/components/plugins/PluginCommands.vue`
- Create: `src/composables/usePluginCommandResult.ts`, `src/components/plugins/PluginCommandResult.vue`
- Test: `tests/frontend/pluginCommands.test.ts`

**Interfaces:**
- Consumes existing `PluginCatalogItem`, `PluginCommandInfo`, store `commandsAvailable`, `commandContextKey`, `runCommand(pluginId: string, contributionId: string): PluginCommandInfo | null`.
- Produces `PluginCommandSummary { pluginId: string; pluginName: string; contributionId: string; title: string }` and store computed `availableCommands: PluginCommandSummary[]`.
- Produces `usePluginCommandResult(pluginSource?: () => PluginCatalogItem)` returning `{ result: Ref<PluginCommandInfo | null>, run: (pluginId: string, contributionId: string) => void, clear: () => void }`. Cards pass their own plugin getter; the workbench uses the no-argument default. This implementation-time refinement avoids measured repeated full-catalog traversal without changing authority.
- Produces `PluginCommandResult` with required `result: PluginCommandInfo` prop and `close` event; preserves existing result test ID and markup styling.

- [ ] **Step 1: Add projection/lifecycle failing tests using real service/store.** Reuse fixtures in the existing test file. Include two enabled v2 plugins sharing a contribution ID, plus disabled/v1 exclusions; verify exact identities and titles, click resolves each plugin's own text. Verify projection empties during a deferred disable and stays empty after unknown failure until retry. Existing result tests cover escaped text and synchronous invalidation.

```ts
expect(store.availableCommands.map(({ pluginId, contributionId, title }) =>
  [pluginId, contributionId, title])).toEqual([
  ['com.example.guide', 'guide.overview', 'Show guide'],
  ['com.example.second', 'guide.overview', 'Second guide'],
])
expect(store.runCommand('com.example.second', 'guide.overview')?.text).toBe('Second content')
```

- [ ] **Step 2: Run RED:** `node node_modules/vitest/vitest.mjs run tests/frontend/pluginCommands.test.ts`. New expectations must fail because projection does not exist; existing tests remain valid. Use elevated execution if Vite cache is denied by the sandbox, not source/config changes.
- [ ] **Step 3: Implement projection and shared result.** Extract a private selector for the existing plugin/command eligibility checks and use it for both projection and click-time lookup. Return newly created summary objects, retaining catalog/manifest order and excluding result text. Do not modify authority algorithms. Replace card result markup with renderer and local result logic with composable, keeping card prop change invalidation and command button test IDs.

```ts
const result = ref<PluginCommandInfo | null>(null)
const clear = () => { result.value = null }
watch([() => store.commandContextKey, pluginSource ?? (() => store.catalog)], clear,
  { flush: 'sync', deep: true })
function run(pluginId: string, contributionId: string): void {
  result.value = store.runCommand(pluginId, contributionId)
}
return { result, run, clear }
```

Card call: `usePluginCommandResult(() => props.plugin)`; this shared synchronous deep watch replaces the redundant standalone card-prop watcher. Add a focused regression for an explicit plugin source's in-place content change and replacement (outside the store array) clearing an existing result; keep the workbench's default whole-catalog behavior and all global-authority invalidation tests. No wall-clock threshold test.

- [ ] **Step 4: GREEN and scope check:** rerun the targeted command tests; run scoped ESLint for changed TS/Vue files and `git diff --check`. Record outputs and RED/GREEN evidence in the task report.
- [ ] **Step 5: Commit only these task source/test files:** `git commit -m "feat(plugin): share authorized command projection and results"` after explicit-path staging.

### Task 2: Installed command workbench and page integration

**Files:**
- Create: `src/components/plugins/PluginCommandWorkbench.vue`, `tests/frontend/pluginCommandWorkbench.test.ts`
- Modify: `src/components/plugins/PluginMarketplacePage.vue`, `src/components/plugins/PluginMarketplacePage.css`, `docs/plugin-local-manifests.md`, `examples/plugins/workspace-guide/README.md`

**Interfaces:**
- Consumes Task 1 store `availableCommands`, `commandsAvailable`, composable `usePluginCommandResult()` and `PluginCommandResult`.
- Produces `PluginCommandWorkbench` without props; only installed page mounts it. Search is a local ref and never mutates the plugin query/status filter.

- [ ] **Step 1: Add real page integration tests.** Mock only `tauriInvoke`; use real service, Pinia, page and workbench. Establish complete schema 3 snapshots as in command tests. Cover default cards/installed view switch; list/count with enabled entries and duplicate contribution IDs across plugins; search command title, plugin name/ID, contribution ID with trim/case folding; no-result vs no-enabled vs unavailable states; click attribution/plain text; local search doesn't alter plugin filters; query/close/view/section transitions clear output; deferred reload and failed disable hide commands/result, retry never revives old result.

```ts
await wrapper.get('[data-testid="plugin-command-view"]').trigger('click')
await wrapper.get('[data-testid="plugin-command-search"]').setValue('  SECOND  ')
expect(wrapper.findAll('[data-testid="plugin-workbench-command"]')).toHaveLength(1)
await wrapper.get('[data-testid="plugin-workbench-command"]').trigger('click')
expect(wrapper.get('[data-testid="plugin-command-result"]').text()).toContain('Second content')
await wrapper.get('[data-testid="plugin-command-search"]').setValue('missing')
expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(false)
```

- [ ] **Step 2: Run RED:** `node node_modules/vitest/vitest.mjs run tests/frontend/pluginCommandWorkbench.test.ts`. Confirm missing workbench behavior, not malformed fixtures, causes failure.
- [ ] **Step 3: Implement the workbench and integration.** Use native labelled search input and buttons, no custom keyboard layer. `data-testid` names in Step 1 are binding. Each action's accessible name includes command title and plugin identity. Render only current summaries; invoke `run(pluginId, contributionId)` at click time. Keep one result and clear it on any search change synchronously.

```ts
const query = ref('')
const { result, run, clear } = usePluginCommandResult()
const matches = computed(() => {
  const needle = query.value.trim().toLowerCase()
  return store.availableCommands.filter(command =>
    [command.title, command.contributionId, command.pluginName, command.pluginId]
      .some(value => value.toLowerCase().includes(needle)))
})
watch(query, clear, { flush: 'sync' })
```

Page has `installedView: 'plugins' | 'commands'`, default `'plugins'`, reset on section change; `aria-pressed` view buttons and mutually exclusive mounting. Workbench lives independently of existing plugin search/status controls; hide those controls while workbench is selected, retain their values on return. Public page header/health surfaces remain untouched. Native button/list semantics, responsive CSS, safe wrapping for long IDs. Distinct empty messages and live count. No automatic selection/execution, no autofocus steal.

- [ ] **Step 4: Update user docs/example README.** Describe where to find the workbench after import and explicit enable, searchable fields, state gates, result revocation and difference from global command palette/online marketplace.
- [ ] **Step 5: Final focused verification:** run new workbench, existing command, card, marketplace page and store test files together; run scoped ESLint for all branch-changed TS/Vue files, `node node_modules/vue-tsc/bin/vue-tsc.js --noEmit`, `node node_modules/vite/bin/vite.js build`, `git diff --check`. Record any existing bundle warning honestly. Do not run whole-app/Rust suites.
- [ ] **Step 6: Self-review and commit task files:** `git commit -m "feat(plugin): add searchable installed command workbench"` after explicit-path staging. Report evidence and any concern; never stage `.superpowers` reports.
