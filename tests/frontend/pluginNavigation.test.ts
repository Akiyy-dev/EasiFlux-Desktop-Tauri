import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import AppShell from '../../src/components/layout/AppShell.vue'
import NavigationRail from '../../src/components/layout/NavigationRail.vue'
import PluginMarketplacePage from '../../src/components/plugins/PluginMarketplacePage.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { pluginNavigationTarget, pluginPageLabel } from '../../src/services/pluginNavigation'
import { flushActiveChartWorkspace } from '../../src/services/chartWorkspaceFlushRegistry'
import { usePluginStore } from '../../src/stores/plugin'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))
vi.mock('../../src/composables/useChartWorkspaceAutosaveHost', () => ({ useChartWorkspaceAutosaveHost: vi.fn() }))
vi.mock('../../src/services/chartWorkspaceFlushRegistry', () => ({ flushActiveChartWorkspace: vi.fn().mockResolvedValue(undefined) }))
vi.mock('../../src/services/errorService', () => ({ reportError: vi.fn() }))
vi.mock('../../src/components/market/KlineChart.vue', () => ({
  default: { template: '<div />' },
}))

function navigationSnapshot(status: 'enabled' | 'disabled' = 'enabled') {
  return {
    schemaVersion: 3,
    revision: '1',
    catalogGeneration: '1',
    availability: 'available',
    availabilityReasonCode: null,
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
    managedOwnership: {
      status: 'available', conflictingEntryCount: 0, rollbackPendingCount: 0, cleanupPendingCount: 0,
    },
    plugins: [{
      manifest: {
        schemaVersion: 3,
        id: 'com.example.shortcuts',
        publisherId: 'com.example',
        publisher: 'Example',
        name: 'Workspace shortcuts',
        description: 'Host navigation shortcuts',
        version: '1.0.0',
        contributions: [{
          kind: 'command',
          contributionId: 'workspace.notifications',
          title: 'Open notifications',
          actionId: 'host.openPage',
          params: { destination: 'settings.notifications' },
        }],
        requestedCapabilities: [],
      },
      source: 'localDeclarative',
      management: 'external',
      canRemove: false,
      toggleBlockReasonCode: null,
      status,
      statusReasonCode: null,
      canToggle: true,
      grantedCapabilities: [],
    }],
  }
}

function mountPluginPage(navigationAvailable = false) {
  const pinia = createPinia()
  setActivePinia(pinia)
  vi.mocked(tauriInvoke).mockResolvedValueOnce(navigationSnapshot())
  return mount(PluginMarketplacePage, {
    props: { section: 'installed', navigationAvailable },
    global: { plugins: [pinia] },
  })
}

function mountShell() {
  const pinia = createPinia()
  setActivePinia(pinia)
  vi.mocked(tauriInvoke).mockResolvedValueOnce(navigationSnapshot())
  return mount(AppShell, {
    attachTo: document.body,
    global: {
      plugins: [pinia],
      stubs: {
        TradingLayout: { template: '<div data-testid="trading-content" />' },
        ChartWorkspacePage: { template: '<div data-testid="chart-content" />' },
        DashboardPage: { template: '<div />' },
        SettingsCenterPage: {
          props: ['initialSection'],
          template: '<section data-testid="settings-content" :data-section="initialSection" />',
        },
      },
    },
  })
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

describe('plugin navigation allowlist', () => {
  it.each([
    ['home', '首页', { page: 'home' }],
    ['trading', '交易页', { page: 'trading' }],
    ['charts', '图表工作区', { page: 'charts' }],
    ['settings.general', '通用设置', { page: 'settings', settingsSection: 'general' }],
    ['settings.notifications', '通知设置', { page: 'settings', settingsSection: 'notifications' }],
    ['settings.about', '关于', { page: 'settings', settingsSection: 'about' }],
  ] as const)('maps %s to a fixed host-owned label and target', (destination, label, target) => {
    expect(pluginPageLabel(destination)).toBe(label)
    expect(pluginNavigationTarget(destination)).toEqual(target)
  })

  it.each([
    'https://example.com', 'settings.account', '/charts', '', null, 7, ['charts'], { page: 'charts' },
  ])('rejects runtime input outside the destination allowlist: %j', (destination) => {
    expect(pluginNavigationTarget(destination)).toBeNull()
  })
})

describe('plugin navigation UI boundary', () => {
  beforeEach(() => {
    vi.resetAllMocks()
    setActivePinia(createPinia())
    vi.mocked(flushActiveChartWorkspace).mockResolvedValue(undefined)
  })

  afterEach(() => {
    document.body.replaceChildren()
  })

  it('keeps navigation disabled in a host that did not opt in', async () => {
    const wrapper = mountPluginPage()
    await flushPromises()

    const button = wrapper.get<HTMLButtonElement>('[data-testid="plugin-command-button"]')
    expect(button.text()).toBe('打开页面：通知设置')
    expect(button.element.disabled).toBe(true)
    expect(wrapper.text()).toContain('当前宿主不提供页面导航')
    await button.trigger('click')
    expect(wrapper.emitted('open-page')).toBeUndefined()
  })

  it('forwards only plugin and contribution identity from a real plugin card click', async () => {
    const wrapper = mountPluginPage(true)
    await flushPromises()

    await wrapper.get('[data-testid="plugin-command-button"]').trigger('click')

    expect(wrapper.emitted('open-page')).toEqual([[
      { pluginId: 'com.example.shortcuts', contributionId: 'workspace.notifications' },
    ]])
  })

  it('forwards the same identity-only intent from the real command workbench', async () => {
    const wrapper = mountPluginPage(true)
    await flushPromises()
    await wrapper.get('[data-testid="plugin-command-view"]').trigger('click')

    await wrapper.get('[data-testid="plugin-workbench-command"]').trigger('click')

    expect(wrapper.emitted('open-page')).toEqual([[
      { pluginId: 'com.example.shortcuts', contributionId: 'workspace.notifications' },
    ]])
  })

  it('rejects disabled and unknown-outcome navigation through the existing command gate', async () => {
    const store = usePluginStore()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(navigationSnapshot('disabled'))
    await store.load()
    expect(store.runCommand('com.example.shortcuts', 'workspace.notifications')).toBeNull()

    vi.mocked(tauriInvoke).mockResolvedValueOnce({ ...navigationSnapshot(), revision: '2' })
    await store.retry()
    expect(store.runCommand('com.example.shortcuts', 'workspace.notifications')).toEqual({
      actionId: 'host.openPage',
      pluginId: 'com.example.shortcuts',
      pluginName: 'Workspace shortcuts',
      contributionId: 'workspace.notifications',
      destination: 'settings.notifications',
    })

    const unknown = deferred<never>()
    vi.mocked(tauriInvoke).mockReturnValueOnce(unknown.promise)
    const changing = store.setEnabled('com.example.shortcuts', false)
    expect(store.runCommand('com.example.shortcuts', 'workspace.notifications')).toBeNull()
    unknown.reject(new Error('unknown result'))
    await changing
    expect(store.runCommand('com.example.shortcuts', 'workspace.notifications')).toBeNull()
  })

  it('re-resolves the real plugin page click in AppShell and opens the allowlisted settings page', async () => {
    const wrapper = mountShell()
    wrapper.getComponent(NavigationRail).vm.$emit('select', 'plugins')
    await flushPromises()
    expect(wrapper.getComponent(PluginMarketplacePage).props('navigationAvailable')).toBe(true)

    await wrapper.get('[data-testid="plugin-command-button"]').trigger('click')
    await flushPromises()

    expect(flushActiveChartWorkspace).toHaveBeenCalledWith('page')
    expect(wrapper.get('[data-testid="settings-content"]').attributes('data-section'))
      .toBe('notifications')
    wrapper.unmount()
  })

  it('rejects a repeated identity intent after the plugin page has already been left', async () => {
    const wrapper = mountShell()
    wrapper.getComponent(NavigationRail).vm.$emit('select', 'plugins')
    await flushPromises()
    vi.mocked(flushActiveChartWorkspace).mockClear()
    const page = wrapper.getComponent(PluginMarketplacePage)
    const intent = {
      pluginId: 'com.example.shortcuts',
      contributionId: 'workspace.notifications',
    }

    page.vm.$emit('open-page', intent)
    page.vm.$emit('open-page', intent)
    await flushPromises()

    expect(flushActiveChartWorkspace).toHaveBeenCalledTimes(1)
    expect(wrapper.get('[data-testid="settings-content"]').attributes('data-section'))
      .toBe('notifications')
    wrapper.unmount()
  })
})
