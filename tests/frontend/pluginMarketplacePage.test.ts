import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import AppShell from '../../src/components/layout/AppShell.vue'
import NavigationRail from '../../src/components/layout/NavigationRail.vue'
import PluginMarketplacePage from '../../src/components/plugins/PluginMarketplacePage.vue'
import Sidebar from '../../src/components/layout/Sidebar.vue'
import { usePluginStore } from '../../src/stores/plugin'
import type {
  PluginAvailabilityReason,
  PluginCatalogItem,
  PluginCatalogMutationResult,
  PluginCatalogSnapshot,
  PluginStatus,
} from '../../src/types/plugin'
import type { PluginSection } from '../../src/types/navigation'

const serviceMocks = vi.hoisted(() => ({
  getCatalog: vi.fn(),
  setEnabled: vi.fn(),
}))

vi.mock('../../src/services/pluginService', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../src/services/pluginService')>(),
  getPluginCatalog: serviceMocks.getCatalog,
  setPluginEnabled: serviceMocks.setEnabled,
}))

vi.mock('../../src/composables/useChartWorkspaceAutosaveHost', () => ({
  useChartWorkspaceAutosaveHost: vi.fn(),
}))

vi.mock('../../src/services/chartWorkspaceFlushRegistry', () => ({
  flushActiveChartWorkspace: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('../../src/components/market/KlineChart.vue', () => ({
  default: { template: '<div />' },
}))

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

function pluginItem(
  id: string,
  name: string,
  status: PluginStatus = 'disabled',
  reason: PluginAvailabilityReason | null = status === 'blocked' ? 'stateUnavailable' : null,
): PluginCatalogItem {
  return {
    manifest: {
      schemaVersion: 1,
      id,
      name,
      version: '1.0.0',
      description: `${name} 的可信描述`,
      publisherId: 'com.easiflux',
      publisher: 'EasiFlux 团队',
      contributions: [],
      requestedCapabilities: [],
    },
    source: 'builtIn',
    grantedCapabilities: [],
    status,
    canToggle: status !== 'blocked',
    statusReasonCode: reason,
  }
}

function availableSnapshot(
  plugins: PluginCatalogItem[] = [
    pluginItem('com.easiflux.alpha', 'Alpha 研究', 'enabled'),
    pluginItem('com.easiflux.beta', 'Beta 交易', 'disabled'),
  ],
  revision = '1',
): PluginCatalogSnapshot {
  return {
    schemaVersion: 1,
    revision,
    availability: 'available',
    availabilityReasonCode: null,
    plugins,
  }
}

function unavailableSnapshot(
  plugins: PluginCatalogItem[] = [],
  reason: PluginAvailabilityReason = 'stateUnavailable',
): PluginCatalogSnapshot {
  return {
    schemaVersion: 1,
    revision: '3',
    availability: 'unavailable',
    availabilityReasonCode: reason,
    plugins,
  }
}

function mutation(
  id: string,
  enabled: boolean,
): PluginCatalogMutationResult {
  return {
    revision: '2',
    plugin: pluginItem(id, 'Beta 交易', enabled ? 'enabled' : 'disabled'),
  }
}

let pinia: Pinia

function mountPage(section: PluginSection = 'installed', attachTo?: Element) {
  return mount(PluginMarketplacePage, {
    props: { section },
    attachTo,
    global: { plugins: [pinia] },
  })
}

describe('PluginMarketplacePage loading and section lifetime', () => {
  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    serviceMocks.getCatalog.mockReset()
    serviceMocks.setEnabled.mockReset()
  })

  afterEach(() => {
    document.body.replaceChildren()
  })

  it('loads once and preserves store-owned query and filter across section changes', async () => {
    serviceMocks.getCatalog.mockResolvedValueOnce(availableSnapshot())
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('#plugin-search').setValue('beta')
    await wrapper.get('#plugin-status-filter').setValue('disabled')
    await wrapper.setProps({ section: 'market' })
    await wrapper.setProps({ section: 'manage' })
    await wrapper.setProps({ section: 'installed' })

    expect(serviceMocks.getCatalog).toHaveBeenCalledTimes(1)
    expect(wrapper.get<HTMLInputElement>('#plugin-search').element.value).toBe('beta')
    expect(wrapper.get<HTMLSelectElement>('#plugin-status-filter').element.value).toBe('disabled')
    expect(wrapper.findAllComponents({ name: 'PluginCard' })).toHaveLength(1)
    expect(wrapper.text()).toContain('Beta 交易')
  })

  it('focuses the page heading once after an attached mount', async () => {
    serviceMocks.getCatalog.mockResolvedValueOnce(availableSnapshot())
    const wrapper = mountPage('installed', document.body)
    await flushPromises()
    await nextTick()

    const heading = wrapper.get('h1')
    expect(heading.attributes('tabindex')).toBe('-1')
    expect(document.activeElement).toBe(heading.element)

    await wrapper.setProps({ section: 'market' })
    expect(document.activeElement).toBe(heading.element)
    wrapper.unmount()
  })

  it('announces initial loading and offers retry after a sanitized first-load failure', async () => {
    const first = deferred<PluginCatalogSnapshot>()
    serviceMocks.getCatalog.mockReturnValueOnce(first.promise)
    const wrapper = mountPage()

    expect(wrapper.get('[role="status"]').text()).toContain('正在加载插件目录')

    first.reject({ code: 'plugin_state_unavailable', message: 'D:\\private\\state.json' })
    await flushPromises()
    const alert = wrapper.get('[role="alert"]')
    expect(alert.text()).toContain('插件状态暂不可用，请重试。')
    expect(alert.text()).not.toContain('private')

    serviceMocks.getCatalog.mockResolvedValueOnce(availableSnapshot([], '2'))
    await alert.get('button').trigger('click')
    await flushPromises()
    expect(serviceMocks.getCatalog).toHaveBeenCalledTimes(2)
    expect(wrapper.find('[data-testid="plugin-load-error"]').exists()).toBe(false)
  })

  it('keeps confirmed cards visible when an explicit refresh fails', async () => {
    serviceMocks.getCatalog.mockResolvedValueOnce(availableSnapshot())
    const wrapper = mountPage()
    await flushPromises()
    const store = usePluginStore()

    serviceMocks.getCatalog.mockRejectedValueOnce({
      code: 'plugin_catalog_invalid',
      message: 'D:\\private\\catalog.json',
    })
    await store.retry()
    await nextTick()

    expect(wrapper.get('[data-testid="plugin-refresh-error"]').text())
      .toContain('插件目录不可用，请稍后重试。')
    expect(wrapper.findAllComponents({ name: 'PluginCard' })).toHaveLength(2)
    expect(wrapper.text()).not.toContain('private')
  })
})

describe('PluginMarketplacePage catalog views', () => {
  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    serviceMocks.getCatalog.mockReset()
    serviceMocks.setEnabled.mockReset()
  })

  it('shows an unavailable subsystem alert for an empty successful catalog', async () => {
    serviceMocks.getCatalog.mockResolvedValueOnce(unavailableSnapshot())
    const wrapper = mountPage()
    await flushPromises()

    const alert = wrapper.get('[data-testid="plugin-availability-alert"]')
    expect(alert.attributes('role')).toBe('alert')
    expect(alert.text()).toContain('插件状态子系统暂时不可用')
    expect(alert.text()).not.toContain('stateUnavailable')
    expect(wrapper.get('[data-testid="plugin-catalog-empty"]').text())
      .toContain('当前没有已安装插件')
  })

  it('keeps unavailable catalog items visible as blocked cards', async () => {
    const blocked = pluginItem(
      'com.easiflux.alpha',
      'Alpha 研究',
      'blocked',
      'catalogInvalid',
    )
    serviceMocks.getCatalog.mockResolvedValueOnce(unavailableSnapshot([blocked], 'catalogInvalid'))
    const wrapper = mountPage()
    await flushPromises()

    expect(wrapper.get('[data-testid="plugin-availability-alert"]').text())
      .toContain('插件目录校验失败')
    expect(wrapper.findAllComponents({ name: 'PluginCard' })).toHaveLength(1)
    expect(wrapper.get('[data-testid="plugin-status"]').text()).toContain('已阻止')
  })

  it('intersects installed search and status filter and distinguishes no matches', async () => {
    serviceMocks.getCatalog.mockResolvedValueOnce(availableSnapshot())
    const wrapper = mountPage()
    await flushPromises()

    expect(wrapper.findAllComponents({ name: 'PluginCard' })).toHaveLength(2)
    await wrapper.get('#plugin-search').setValue('alpha')
    await wrapper.get('#plugin-status-filter').setValue('disabled')

    expect(wrapper.findAllComponents({ name: 'PluginCard' })).toHaveLength(0)
    expect(wrapper.get('[data-testid="plugin-no-match"]').text())
      .toContain('没有符合当前条件的插件')
    expect(wrapper.find('[data-testid="plugin-catalog-empty"]').exists()).toBe(false)
  })

  it('renders only built-in entries in a read-only Phase 0 marketplace', async () => {
    const untrusted = {
      ...pluginItem('com.example.local', '本地未受信条目'),
      source: 'local',
    } as unknown as PluginCatalogItem
    serviceMocks.getCatalog.mockResolvedValueOnce(availableSnapshot([
      pluginItem('com.easiflux.alpha', 'Alpha 研究'),
      untrusted,
    ]))
    const wrapper = mountPage('market')
    await flushPromises()

    expect(wrapper.text()).toContain('Alpha 研究')
    expect(wrapper.text()).toContain('随应用提供')
    expect(wrapper.text()).not.toContain('本地未受信条目')
    expect(wrapper.find('[role="switch"]').exists()).toBe(false)
    const prohibitedActions = wrapper.findAll('button, a[href], [role="button"]')
      .filter((control) => /安装|下载|更新/.test(
        `${control.text()} ${control.attributes('aria-label') ?? ''}`,
      ))
    expect(prohibitedActions).toHaveLength(0)
  })

  it('summarizes the complete catalog independently of installed filters', async () => {
    serviceMocks.getCatalog.mockResolvedValueOnce(availableSnapshot([
      pluginItem('com.easiflux.alpha', 'Alpha 研究', 'enabled'),
      pluginItem('com.easiflux.beta', 'Beta 交易', 'disabled'),
      pluginItem('com.easiflux.gamma', 'Gamma 风控', 'blocked'),
    ]))
    const wrapper = mountPage()
    await flushPromises()
    const store = usePluginStore()
    store.setQuery('不存在')
    store.setStatusFilter('enabled')

    await wrapper.setProps({ section: 'manage' })

    expect(wrapper.get('[data-testid="enabled-count"]').text()).toContain('1')
    expect(wrapper.get('[data-testid="disabled-count"]').text()).toContain('1')
    expect(wrapper.get('[data-testid="blocked-count"]').text()).toContain('1')
    expect(wrapper.findAll('[data-testid="plugin-management-item"]')).toHaveLength(3)
    expect(wrapper.findAll('[data-testid="management-requested-capabilities"]'))
      .toHaveLength(3)
    expect(wrapper.findAll('[data-testid="management-granted-capabilities"]'))
      .toHaveLength(3)
    expect(wrapper.text()).toContain('无需额外权限')
  })

  it('delegates a toggle without changing the confirmed status optimistically', async () => {
    serviceMocks.getCatalog.mockResolvedValueOnce(availableSnapshot([
      pluginItem('com.easiflux.beta', 'Beta 交易', 'disabled'),
    ]))
    const response = deferred<PluginCatalogMutationResult>()
    serviceMocks.setEnabled.mockReturnValueOnce(response.promise)
    const wrapper = mountPage()
    await flushPromises()

    await wrapper.get('[role="switch"]').trigger('change')

    expect(serviceMocks.setEnabled).toHaveBeenCalledWith('com.easiflux.beta', true)
    expect(wrapper.get('[data-testid="plugin-status"]').text()).toContain('已停用')
    expect(wrapper.get<HTMLInputElement>('[role="switch"]').element.disabled).toBe(true)

    response.resolve(mutation('com.easiflux.beta', true))
    await flushPromises()
    expect(wrapper.get('[data-testid="plugin-status"]').text()).toContain('已启用')
  })
})

describe('AppShell plugin integration', () => {
  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
  })

  function mountShell() {
    return mount(AppShell, {
      global: {
        plugins: [pinia],
        stubs: {
          TopBar: { template: '<div />' },
          DashboardPage: { template: '<div />' },
          TradingLayout: { props: ['active'], template: '<div />' },
          ChartWorkspacePage: { props: ['active'], template: '<div />' },
          SettingsCenterPage: { template: '<div />' },
          PluginMarketplacePage: {
            name: 'PluginMarketplacePage',
            props: ['section'],
            template: '<div data-testid="plugin-page-stub" :data-section="section" />',
          },
        },
      },
    })
  }

  it('replaces the placeholder, forwards all sections, and resets top-level plugin entry', async () => {
    const wrapper = mountShell()
    await wrapper.getComponent(NavigationRail).get('button[aria-label="插件"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="plugin-page-stub"]').attributes('data-section'))
      .toBe('installed')
    expect(wrapper.text()).not.toContain('该页面将在后续 PRD 中逐步迁移实现')

    const sidebar = wrapper.getComponent(Sidebar)
    await sidebar.findAll('button').find((button) => button.text() === '插件市场')!.trigger('click')
    expect(wrapper.get('[data-testid="plugin-page-stub"]').attributes('data-section'))
      .toBe('market')
    await sidebar.findAll('button').find((button) => button.text() === '插件管理')!.trigger('click')
    expect(wrapper.get('[data-testid="plugin-page-stub"]').attributes('data-section'))
      .toBe('manage')

    await wrapper.getComponent(NavigationRail).get('button[aria-label="首页"]').trigger('click')
    await wrapper.getComponent(NavigationRail).get('button[aria-label="插件"]').trigger('click')
    expect(wrapper.get('[data-testid="plugin-page-stub"]').attributes('data-section'))
      .toBe('installed')
  })
})
