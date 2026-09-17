import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import PluginMarketplacePage from '../../src/components/plugins/PluginMarketplacePage.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { usePluginStore } from '../../src/stores/plugin'
import type {
  PluginCatalogItem,
  PluginCatalogSnapshot,
  PluginCommandContribution,
  PluginStatus,
} from '../../src/types/plugin'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

function command(
  contributionId: string,
  title: string,
  resultTitle: string,
  text: string,
): PluginCommandContribution {
  return {
    kind: 'command',
    contributionId,
    title,
    actionId: 'host.showInfo',
    params: { title: resultTitle, text },
  }
}

function commandPlugin(
  id: string,
  name: string,
  status: PluginStatus,
  contributions: PluginCommandContribution[],
  reason: 'stateUnavailable' | null = null,
): PluginCatalogItem {
  return {
    manifest: {
      schemaVersion: 2,
      id,
      publisherId: 'com.example',
      publisher: 'Example publisher',
      name,
      description: `${name} description`,
      version: '1.0.0',
      contributions,
      requestedCapabilities: [],
    },
    source: 'localDeclarative',
    management: 'external',
    canRemove: false,
    toggleBlockReasonCode: null,
    status,
    statusReasonCode: reason,
    canToggle: status !== 'blocked',
    grantedCapabilities: [],
  }
}

function legacyPlugin(): PluginCatalogItem {
  return {
    manifest: {
      schemaVersion: 1,
      id: 'com.example.legacy',
      publisherId: 'com.example',
      publisher: 'Example publisher',
      name: 'Legacy metadata',
      description: 'Metadata-only plugin',
      version: '1.0.0',
      contributions: [],
      requestedCapabilities: [],
    },
    source: 'localDeclarative',
    management: 'external',
    canRemove: false,
    toggleBlockReasonCode: null,
    status: 'enabled',
    statusReasonCode: null,
    canToggle: true,
    grantedCapabilities: [],
  }
}

const alphaCommands = [
  command('alpha.overview', 'First command', 'First result', 'First content'),
  command('shared.info', 'Shared alpha', 'Alpha shared result', 'Alpha shared content'),
]

const betaCommands = [
  command('shared.info', 'Shared beta', 'Beta shared result', 'Beta shared content'),
  command('beta.second', 'Second command', 'Second result', '<b>Second content</b>'),
]

function snapshot(
  plugins: PluginCatalogItem[] = [
    commandPlugin('com.example.alpha', 'Alpha tools', 'enabled', alphaCommands),
    commandPlugin('com.example.beta', 'Beta workspace', 'enabled', betaCommands),
    commandPlugin('com.example.disabled', 'Disabled tools', 'disabled', [
      command('disabled.only', 'Disabled command', 'Disabled result', 'Disabled content'),
    ]),
    legacyPlugin(),
  ],
  revision = '1',
  catalogGeneration = '1',
): PluginCatalogSnapshot {
  return {
    schemaVersion: 3,
    revision,
    catalogGeneration,
    availability: 'available',
    availabilityReasonCode: null,
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
    managedOwnership: {
      status: 'available',
      conflictingEntryCount: 0,
      rollbackPendingCount: 0,
      cleanupPendingCount: 0,
    },
    plugins,
  }
}

function unavailableSnapshot(): PluginCatalogSnapshot {
  return {
    ...snapshot([
      commandPlugin(
        'com.example.blocked',
        'Blocked commands',
        'blocked',
        [command('blocked.only', 'Blocked command', 'Blocked result', 'Blocked content')],
        'stateUnavailable',
      ),
    ]),
    availability: 'unavailable',
    availabilityReasonCode: 'stateUnavailable',
  }
}

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T) => void
  reject: (reason: unknown) => void
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

async function mountLoaded(catalog = snapshot()) {
  setActivePinia(createPinia())
  vi.mocked(tauriInvoke).mockResolvedValueOnce(catalog)
  const wrapper = mount(PluginMarketplacePage, { props: { section: 'installed' } })
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.resetAllMocks()
  setActivePinia(createPinia())
})

describe('installed plugin command workbench', () => {
  it('defaults to cards, keeps plugin filters local, and resets the view after leaving installed', async () => {
    const wrapper = await mountLoaded()
    const pluginView = wrapper.get('[data-testid="plugin-list-view"]')
    const commandView = wrapper.get('[data-testid="plugin-command-view"]')

    expect(pluginView.attributes('aria-pressed')).toBe('true')
    expect(commandView.attributes('aria-pressed')).toBe('false')
    expect(wrapper.findAllComponents({ name: 'PluginCard' })).toHaveLength(4)
    expect(wrapper.find('[data-testid="plugin-command-workbench"]').exists()).toBe(false)

    await wrapper.get('#plugin-search').setValue('Alpha')
    await wrapper.get('#plugin-status-filter').setValue('enabled')
    await commandView.trigger('click')
    expect(commandView.attributes('aria-pressed')).toBe('true')
    expect(wrapper.find('#plugin-search').exists()).toBe(false)
    expect(wrapper.get('[data-testid="plugin-command-workbench"]').exists()).toBe(true)
    expect(usePluginStore().query).toBe('Alpha')
    expect(usePluginStore().statusFilter).toBe('enabled')

    await pluginView.trigger('click')
    expect(wrapper.get<HTMLInputElement>('#plugin-search').element.value).toBe('Alpha')
    expect(wrapper.get<HTMLSelectElement>('#plugin-status-filter').element.value).toBe('enabled')

    await commandView.trigger('click')
    await wrapper.setProps({ section: 'market' })
    expect(wrapper.find('[data-testid="plugin-command-view"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="plugin-command-workbench"]').exists()).toBe(false)
    await wrapper.setProps({ section: 'installed' })
    expect(wrapper.get('[data-testid="plugin-list-view"]').attributes('aria-pressed')).toBe('true')
    expect(wrapper.find('[data-testid="plugin-command-workbench"]').exists()).toBe(false)
  })

  it('lists only enabled commands, keeps shared contribution IDs distinct, and reports a live count', async () => {
    const wrapper = await mountLoaded()
    await wrapper.get('[data-testid="plugin-command-view"]').trigger('click')

    const entries = wrapper.findAll('.plugin-command-workbench__item')
    expect(wrapper.findAll('[data-testid="plugin-workbench-command"]')).toHaveLength(4)
    expect(entries.map((entry) => entry.get('h3').text())).toEqual([
      'First command', 'Shared alpha', 'Shared beta', 'Second command',
    ])
    expect(entries.map((entry) => entry.findAll('dd').map((value) => value.text()))).toEqual([
      ['com.example.alpha', 'alpha.overview'],
      ['com.example.alpha', 'shared.info'],
      ['com.example.beta', 'shared.info'],
      ['com.example.beta', 'beta.second'],
    ])
    expect(wrapper.get('[data-testid="plugin-command-count"]').text()).toContain('4')
    expect(wrapper.get('[data-testid="plugin-command-count"]').attributes('aria-live')).toBe('polite')
  })

  it.each([
    ['  SECOND  ', ['Second command']],
    ['beta WORKSPACE', ['Shared beta', 'Second command']],
    ['COM.EXAMPLE.ALPHA', ['First command', 'Shared alpha']],
    ['  SHARED.INFO  ', ['Shared alpha', 'Shared beta']],
  ])('searches title, plugin name, plugin ID and contribution ID for %s', async (query, titles) => {
    const wrapper = await mountLoaded()
    await wrapper.get('[data-testid="plugin-command-view"]').trigger('click')
    await wrapper.get('[data-testid="plugin-command-search"]').setValue(query)

    expect(wrapper.findAll('.plugin-command-workbench__item h3').map((entry) => entry.text()))
      .toEqual(titles)
    expect(usePluginStore().query).toBe('')
    expect(usePluginStore().statusFilter).toBe('all')
  })

  it('runs the current plugin-qualified command explicitly and renders attributed plain text', async () => {
    const wrapper = await mountLoaded()
    await wrapper.get('[data-testid="plugin-command-view"]').trigger('click')
    await wrapper.get('[data-testid="plugin-command-search"]').setValue('second')
    const action = wrapper.get<HTMLButtonElement>('[data-testid="plugin-workbench-command"]')

    expect(action.attributes('aria-label').startsWith(action.text())).toBe(true)
    expect(action.attributes('aria-label')).toContain('Second command')
    expect(action.attributes('aria-label')).toContain('Beta workspace')
    expect(action.attributes('aria-label')).toContain('com.example.beta')
    expect(action.attributes('aria-label')).toContain('beta.second')
    await action.trigger('click')

    const result = wrapper.get('[data-testid="plugin-command-result"]')
    expect(result.text()).toContain('Beta workspace（com.example.beta）')
    expect(result.text()).toContain('<b>Second content</b>')
    expect(result.find('b').exists()).toBe(false)
    await result.get('button').trigger('click')
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(false)
  })

  it('clears output synchronously on search and distinguishes no-match, no-enabled and unavailable states', async () => {
    const wrapper = await mountLoaded()
    await wrapper.get('[data-testid="plugin-command-view"]').trigger('click')
    await wrapper.findAll('[data-testid="plugin-workbench-command"]')[0].trigger('click')
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(true)

    await wrapper.get('[data-testid="plugin-command-search"]').setValue('missing')
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(false)
    expect(wrapper.get('[data-testid="plugin-command-no-match"]').text()).toContain('没有匹配命令')

    wrapper.unmount()
    const noEnabled = await mountLoaded(snapshot([
      commandPlugin('com.example.disabled', 'Disabled tools', 'disabled', [
        command('disabled.only', 'Disabled command', 'Disabled result', 'Disabled content'),
      ]),
      legacyPlugin(),
    ]))
    await noEnabled.get('[data-testid="plugin-command-view"]').trigger('click')
    expect(noEnabled.get('[data-testid="plugin-command-empty"]').text()).toContain('没有已启用的只读命令')
    noEnabled.unmount()

    const unavailable = await mountLoaded(unavailableSnapshot())
    await unavailable.get('[data-testid="plugin-command-view"]').trigger('click')
    expect(unavailable.get('[data-testid="plugin-command-unavailable"]').text())
      .toContain('当前状态尚未确认')
  })

  it('clears output across view and section transitions without replaying it', async () => {
    const wrapper = await mountLoaded()
    await wrapper.get('[data-testid="plugin-command-view"]').trigger('click')
    await wrapper.findAll('[data-testid="plugin-workbench-command"]')[0].trigger('click')
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(true)

    await wrapper.get('[data-testid="plugin-list-view"]').trigger('click')
    await wrapper.get('[data-testid="plugin-command-view"]').trigger('click')
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(false)
    await wrapper.findAll('[data-testid="plugin-workbench-command"]')[0].trigger('click')
    await wrapper.setProps({ section: 'manage' })
    await wrapper.setProps({ section: 'installed' })
    await wrapper.get('[data-testid="plugin-command-view"]').trigger('click')
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(false)
  })

  it('hides commands and output during reload and after a failed disable, and retry never revives output', async () => {
    const wrapper = await mountLoaded()
    const store = usePluginStore()
    await wrapper.get('[data-testid="plugin-command-view"]').trigger('click')
    await wrapper.findAll('[data-testid="plugin-workbench-command"]')[0].trigger('click')

    const reload = deferred<PluginCatalogSnapshot>()
    vi.mocked(tauriInvoke).mockReturnValueOnce(reload.promise)
    const reloading = store.reload()
    await nextTick()
    expect(wrapper.findAll('[data-testid="plugin-workbench-command"]')).toHaveLength(0)
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(false)
    reload.resolve(snapshot(undefined, '2', '2'))
    await reloading
    await nextTick()
    expect(wrapper.findAll('[data-testid="plugin-workbench-command"]')).toHaveLength(4)
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(false)

    await wrapper.findAll('[data-testid="plugin-workbench-command"]')[0].trigger('click')
    const disabling = deferred<unknown>()
    vi.mocked(tauriInvoke).mockReturnValueOnce(disabling.promise)
    const disable = store.setEnabled('com.example.alpha', false)
    await nextTick()
    expect(wrapper.findAll('[data-testid="plugin-workbench-command"]')).toHaveLength(0)
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(false)
    disabling.reject(new Error('unknown disable result'))
    await disable
    expect(wrapper.findAll('[data-testid="plugin-workbench-command"]')).toHaveLength(0)

    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot(undefined, '2', '2'))
    await store.retry()
    await nextTick()
    expect(wrapper.findAll('[data-testid="plugin-workbench-command"]')).toHaveLength(4)
    expect(wrapper.find('[data-testid="plugin-command-result"]').exists()).toBe(false)
  })
})
