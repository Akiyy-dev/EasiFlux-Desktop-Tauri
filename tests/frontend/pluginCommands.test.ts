import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref } from 'vue'
import PluginMarketplacePage from '../../src/components/plugins/PluginMarketplacePage.vue'
import { usePluginCommandResult } from '../../src/composables/usePluginCommandResult'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { usePluginStore } from '../../src/stores/plugin'
import type { PluginCatalogItem } from '../../src/types/plugin'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

function item(status = 'disabled', text = 'Read-only guide') {
  return {
    manifest: {
      schemaVersion: 2, id: 'com.example.guide', publisherId: 'com.example',
      publisher: 'Example', name: 'Workspace guide', description: 'Manual example', version: '1.0.0',
      contributions: [{
        kind: 'command', contributionId: 'guide.overview', title: 'Show guide',
        actionId: 'host.showInfo', params: { title: 'Guide', text },
      }],
      requestedCapabilities: [],
    },
    source: 'localDeclarative', management: 'managed', canRemove: status === 'disabled',
    toggleBlockReasonCode: null, status, statusReasonCode: null, canToggle: true,
    grantedCapabilities: [],
  }
}

function secondItem() {
  return {
    ...item('enabled', 'Second content'),
    manifest: {
      ...item().manifest,
      id: 'com.example.second',
      name: 'Second guide',
      contributions: [{
        kind: 'command', contributionId: 'guide.overview', title: 'Second guide',
        actionId: 'host.showInfo', params: { title: 'Second result', text: 'Second content' },
      }],
    },
  }
}

function snapshot(status = 'disabled', revision = '1', generation = '1', text?: string) {
  return {
    schemaVersion: 3, revision, catalogGeneration: generation,
    availability: 'available', availabilityReasonCode: null,
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
    managedOwnership: {
      status: 'available', conflictingEntryCount: 0, rollbackPendingCount: 0, cleanupPendingCount: 0,
    },
    plugins: [item(status, text)],
  }
}

function mutation(status: string, revision = '2', text?: string) {
  return { schemaVersion: 3, revision, catalogGeneration: '1', plugin: item(status, text) }
}

function deferred() {
  let resolve!: (value: unknown) => void
  let reject!: (error: unknown) => void
  const promise = new Promise((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

beforeEach(() => {
  vi.resetAllMocks()
  setActivePinia(createPinia())
})

describe('declarative commands with real service and store', () => {
  it('projects only authorized commands by plugin identity and revokes them through unknown results', async () => {
    const guide = item('enabled', 'Guide content')
    const second = secondItem()
    const disabled = {
      ...item('disabled'),
      manifest: { ...item().manifest, id: 'com.example.disabled', name: 'Disabled guide' },
    }
    const legacy = {
      ...item('enabled'),
      manifest: {
        ...item().manifest,
        schemaVersion: 1,
        id: 'com.example.legacy',
        name: 'Legacy guide',
        contributions: [],
      },
    }
    const enabledSnapshot = {
      ...snapshot('enabled'),
      plugins: [disabled, guide, legacy, second],
    }
    const store = usePluginStore()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(enabledSnapshot)

    await store.load()

    expect(store.availableCommands.map(({ pluginId, contributionId, title }) => (
      [pluginId, contributionId, title]
    ))).toEqual([
      ['com.example.guide', 'guide.overview', 'Show guide'],
      ['com.example.second', 'guide.overview', 'Second guide'],
    ])
    expect(store.availableCommands.every((command) => !('text' in command))).toBe(true)
    expect(store.runCommand('com.example.guide', 'guide.overview')?.text).toBe('Guide content')
    expect(store.runCommand('com.example.second', 'guide.overview')?.text).toBe('Second content')

    const disable = deferred()
    vi.mocked(tauriInvoke).mockReturnValueOnce(disable.promise)
    const flight = store.setEnabled('com.example.second', false)
    expect(store.availableCommands).toEqual([])
    disable.reject(new Error('unknown result'))
    await flight
    expect(store.availableCommands).toEqual([])

    vi.mocked(tauriInvoke).mockResolvedValueOnce({ ...enabledSnapshot, revision: '2' })
    await store.retry()
    expect(store.availableCommands.map(({ pluginId }) => pluginId)).toEqual([
      'com.example.guide',
      'com.example.second',
    ])
  })

  it('resolves shared contribution IDs through each mounted plugin card', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce({
      ...snapshot('enabled'),
      plugins: [item('enabled', 'Guide content'), secondItem()],
    })
    const wrapper = mount(PluginMarketplacePage, { props: { section: 'installed' } })
    await flushPromises()
    const cards = wrapper.findAll('.plugin-card')
    expect(cards).toHaveLength(2)

    await cards[0].get('[data-testid="plugin-command-button"]').trigger('click')
    const guideResult = cards[0].get('[data-testid="plugin-command-result"]')
    expect(guideResult.text()).toContain('Workspace guide（com.example.guide）')
    expect(guideResult.text()).toContain('Guide content')
    expect(cards[1].find('[data-testid="plugin-command-result"]').exists()).toBe(false)

    await cards[1].get('[data-testid="plugin-command-button"]').trigger('click')
    const secondResult = cards[1].get('[data-testid="plugin-command-result"]')
    expect(secondResult.text()).toContain('Second guide（com.example.second）')
    expect(secondResult.text()).toContain('Second content')
    expect(guideResult.text()).not.toContain('Second content')
    expect(secondResult.text()).not.toContain('Guide content')
    wrapper.unmount()
  })

  it('synchronously clears a command result when its explicit plugin source changes', async () => {
    const store = usePluginStore()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled', '1', '1', 'Authoritative text'))
    await store.load()
    const pluginSource = ref(item('enabled', 'Detached source'))
    expect(pluginSource.value).not.toBe(store.catalog[0])
    const scope = effectScope()

    try {
      const lifecycle = scope.run(() => usePluginCommandResult(
        () => pluginSource.value as PluginCatalogItem,
      ))!
      lifecycle.run('com.example.guide', 'guide.overview')
      expect(lifecycle.result.value?.text).toBe('Authoritative text')

      pluginSource.value.manifest.contributions[0].params.text = 'Changed in place'
      expect(lifecycle.result.value).toBeNull()

      lifecycle.run('com.example.guide', 'guide.overview')
      expect(lifecycle.result.value?.text).toBe('Authoritative text')
      pluginSource.value = item('enabled', 'Replacement source')
      expect(lifecycle.result.value).toBeNull()
    } finally {
      scope.stop()
    }
  })

  it('retains contributions, enables only confirmed commands and fails closed after a failed disable', async () => {
    const store = usePluginStore()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot())
    await store.load()
    expect(store.catalog[0]?.manifest.contributions).toHaveLength(1)
    expect(store.runCommand('com.example.guide', 'guide.overview')).toBeNull()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(mutation('enabled'))
    expect(await store.setEnabled('com.example.guide', true)).toBe(true)
    expect(store.runCommand('com.example.guide', 'guide.overview')?.text).toBe('Read-only guide')
    expect(store.runCommand('com.other.guide', 'guide.overview')).toBeNull()
    expect(store.runCommand('com.example.guide', 'guide.missing')).toBeNull()
    const disable = deferred()
    vi.mocked(tauriInvoke).mockReturnValueOnce(disable.promise)
    const flight = store.setEnabled('com.example.guide', false)
    expect(store.commandsAvailable).toBe(false)
    disable.reject(new Error('unknown result'))
    await flight
    expect(store.runCommand('com.example.guide', 'guide.overview')).toBeNull()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled', '2'))
    await store.retry()
    expect(store.runCommand('com.example.guide', 'guide.overview')?.text).toBe('Read-only guide')
    // Removal confirmation owns a deep copy, not an emptied or mutable contribution list.
    vi.mocked(tauriInvoke).mockResolvedValueOnce(mutation('disabled', '3'))
    await store.setEnabled('com.example.guide', false)
    expect(store.beginRemoval('com.example.guide')).toBe(true)
    expect(store.removalTarget?.plugin.manifest.contributions[0]?.params.text).toBe('Read-only guide')
    expect(store.removalTarget?.plugin.manifest.contributions[0]?.params)
      .not.toBe(store.catalog[0].manifest.contributions[0]?.params)
  })

  it('does not authorize an ignored or pre-failure full snapshot or generation-cleared pending mutation', async () => {
    const store = usePluginStore()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled'))
    await store.load()
    expect(store.commandsAvailable).toBe(true)
    const disable = deferred()
    vi.mocked(tauriInvoke).mockReturnValueOnce(disable.promise)
    const changing = store.setEnabled('com.example.guide', false)
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled', '1', '2'))
    await store.reload()
    expect(store.pendingIds.size).toBe(0)
    expect(store.commandsAvailable).toBe(false)
    const staleLoad = deferred()
    vi.mocked(tauriInvoke).mockReturnValueOnce(staleLoad.promise)
    const loading = store.retry()
    disable.reject(new Error('unknown result'))
    await changing
    staleLoad.resolve(snapshot('enabled', '1', '2'))
    await loading
    expect(store.commandsAvailable).toBe(false)
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled', '1', '1'))
    await store.retry()
    expect(store.commandsAvailable).toBe(false)
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled', '1', '2'))
    await store.retry()
    expect(store.commandsAvailable).toBe(true)
  })

  it('never restores authority from an ignored mutation result', async () => {
    const store = usePluginStore()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled'))
    await store.load()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(mutation('disabled', '2', 'changed content'))
    expect(await store.setEnabled('com.example.guide', false)).toBe(false)
    expect(store.commandsAvailable).toBe(false)
    // A subsequent valid item-only response is not a full confirmation.
    vi.mocked(tauriInvoke).mockResolvedValueOnce(mutation('enabled', '3'))
    await store.setEnabled('com.example.guide', true)
    expect(store.commandsAvailable).toBe(false)
  })

  it('revokes newer authority when an older pre-failure reload replaces the catalog', async () => {
    const store = usePluginStore()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled'))
    await store.load()
    const oldReload = deferred()
    vi.mocked(tauriInvoke).mockReturnValueOnce(oldReload.promise)
    const reloading = store.reload()
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('unknown disable result'))
    await store.setEnabled('com.example.guide', false)
    // This newer request confirms the old catalog, but the earlier reload is
    // still in flight and may replace it with a higher generation.
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled'))
    await store.retry()
    expect(store.commandsAvailable).toBe(false)
    oldReload.resolve(snapshot('enabled', '1', '2', 'Old request replacement'))
    await reloading
    expect(store.catalogGeneration).toBe('2')
    expect(store.catalog[0].manifest.contributions[0]?.params.text).toBe('Old request replacement')
    expect(store.commandsAvailable).toBe(false)
    expect(store.runCommand('com.example.guide', 'guide.overview')).toBeNull()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled', '1', '2', 'Old request replacement'))
    await store.retry()
    expect(store.runCommand('com.example.guide', 'guide.overview')?.text).toBe('Old request replacement')
  })

  it('cannot authorize from a reload while an import is in flight, or by dismissing an unknown import', async () => {
    const store = usePluginStore()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled'))
    await store.load()
    const preview = {
      schemaVersion: 1, status: 'ready', token: 'a'.repeat(32), expiresInSeconds: 300,
      catalogGeneration: '1',
      manifest: {
        ...item().manifest, schemaVersion: 1, id: 'com.example.new', contributions: [],
      },
    }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(preview)
    await store.prepareImport()
    const commit = deferred()
    vi.mocked(tauriInvoke).mockReturnValueOnce(commit.promise)
    const importing = store.commitImport()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled', '1', '2'))
    await store.reload()
    expect(store.commandsAvailable).toBe(false)
    commit.resolve({
      schemaVersion: 2, status: 'notImported', disabledDecisionSaved: false,
      reasonCode: 'plugin_catalog_stale', snapshot: snapshot('enabled'),
    })
    await importing
    expect(store.commandsAvailable).toBe(false)
    store.clearImportResult()
    vi.mocked(tauriInvoke).mockResolvedValueOnce({ ...preview, catalogGeneration: '2' })
    await store.prepareImport()
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('unknown'))
    await store.commitImport()
    expect(store.importOutcomeUnknown).toBe(true)
    store.clearImportResult()
    expect(store.commandsAvailable).toBe(false)
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled', '1', '2'))
    await store.retry()
    expect(store.commandsAvailable).toBe(true)
  })

  it('shows escaped text on actual click, revokes output immediately, and requires a new click after enable', async () => {
    const text = '<img src=x onerror=alert(1)> Read-only guide'
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot('enabled', '1', '1', text))
    const wrapper = mount(PluginMarketplacePage, { props: { section: 'installed' } })
    await flushPromises()
    await wrapper.get('[data-testid="plugin-command-button"]').trigger('click')
    expect(wrapper.get('[data-testid="plugin-command-result"]').text()).toContain(text)
    expect(wrapper.get('[data-testid="plugin-command-result"]').text()).toContain('Workspace guide')
    expect(wrapper.find('img').exists()).toBe(false)
    const store = usePluginStore()
    const disable = deferred()
    vi.mocked(tauriInvoke).mockReturnValueOnce(disable.promise)
    const flight = store.setEnabled('com.example.guide', false)
    await nextTick()
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="plugin-command-button"]').exists()).toBe(false)
    disable.resolve(mutation('disabled', '2', text))
    await flight
    vi.mocked(tauriInvoke).mockResolvedValueOnce(mutation('enabled', '3', text))
    await store.setEnabled('com.example.guide', true)
    await nextTick()
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(false)
    await wrapper.get('[data-testid="plugin-command-button"]').trigger('click')
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(true)
    await wrapper.setProps({ section: 'market' })
    expect(wrapper.find('[data-testid="plugin-command-button"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(false)
    await wrapper.setProps({ section: 'manage' })
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(false)
    wrapper.unmount()
  })
})
