import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AppShell from '../../src/components/layout/AppShell.vue'
import NavigationRail from '../../src/components/layout/NavigationRail.vue'
import Sidebar from '../../src/components/layout/Sidebar.vue'
import TopBar from '../../src/components/layout/TopBar.vue'
import { flushActiveChartWorkspace } from '../../src/services/chartWorkspaceFlushRegistry'
import { reportError } from '../../src/services/errorService'
import type { NavKey } from '../../src/types/navigation'

const deferred = <T,>() => {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

vi.mock('../../src/services/chartWorkspaceFlushRegistry', () => ({
  flushActiveChartWorkspace: vi.fn().mockResolvedValue(undefined),
  registerChartWorkspaceFlusher: vi.fn(() => ({
    setActive: vi.fn(),
    unregister: vi.fn(),
  })),
}))

vi.mock('../../src/services/errorService', () => ({ reportError: vi.fn() }))
vi.mock('../../src/composables/useNewsRuntimeHost', () => ({ useNewsRuntimeHost: vi.fn() }))

vi.mock('../../src/components/layout/TradingLayout.vue', () => ({
  default: {
    name: 'TradingLayout',
    props: { active: Boolean },
    template: '<div data-testid="trading-layout" />',
  },
}))

vi.mock('../../src/components/market/KlineChart.vue', () => ({
  default: {
    name: 'KlineChart',
    props: { mode: String, active: Boolean },
    template: '<div data-testid="kline-chart" />',
  },
}))

async function selectPage(wrapper: ReturnType<typeof mount>, page: NavKey): Promise<void> {
  wrapper.findComponent(NavigationRail).vm.$emit('select', page)
  await flushPromises()
}

describe('chart workspace page integration', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(flushActiveChartWorkspace).mockReset().mockResolvedValue(undefined)
    vi.mocked(reportError).mockReset()
  })

  function mountShell() {
    return mount(AppShell, {
      global: {
        plugins: [pinia],
        stubs: {
          DashboardPage: { template: '<div data-testid="dashboard-page" />' },
          AccountCenterPage: { template: '<div data-testid="account-page" />' },
        },
      },
    })
  }

  it('shows only the full-size chart page and hides the real Sidebar', async () => {
    const wrapper = mountShell()

    await selectPage(wrapper, 'charts')

    const page = wrapper.get('[data-testid="chart-workspace-page"]')
    expect(wrapper.findComponent(Sidebar).exists()).toBe(false)
    expect(wrapper.find('[data-testid="trading-layout"]').exists()).toBe(false)
    expect(page.find('.ef-card').exists()).toBe(false)
    expect(wrapper.find('[data-testid="account-page"]').exists()).toBe(false)
  })

  it('keeps the same workspace instance across account navigation', async () => {
    const wrapper = mountShell()
    await selectPage(wrapper, 'charts')
    const first = wrapper.get('[data-testid="chart-workspace-page"]').element

    await selectPage(wrapper, 'account')
    await selectPage(wrapper, 'charts')

    expect(wrapper.get('[data-testid="chart-workspace-page"]').element).toBe(first)
  })

  it('keeps the same trading chart after it has been visited', async () => {
    const wrapper = mountShell()
    await selectPage(wrapper, 'trading')
    const first = wrapper.get('[data-testid="trading-layout"]').element

    await selectPage(wrapper, 'charts')
    await selectPage(wrapper, 'trading')

    expect(wrapper.get('[data-testid="trading-layout"]').element).toBe(first)
  })

  it('navigates immediately while the previous chart flush is still pending', async () => {
    const wrapper = mountShell()
    await selectPage(wrapper, 'charts')
    const pending = deferred<void>()
    vi.mocked(flushActiveChartWorkspace).mockReturnValueOnce(pending.promise)

    wrapper.findComponent(NavigationRail).vm.$emit('select', 'account')
    await wrapper.vm.$nextTick()

    expect(wrapper.find('[data-testid="account-page"]').exists()).toBe(true)
    expect(wrapper.get('[data-testid="chart-workspace-page"]').isVisible()).toBe(false)
    expect(wrapper.findComponent(NavigationRail).props('active')).toBe('account')

    pending.resolve()
    await flushPromises()
    expect(wrapper.find('[data-testid="account-page"]').exists()).toBe(true)
    expect(wrapper.findComponent(NavigationRail).props('active')).toBe('account')
  })

  it('reports a page flush failure and still navigates', async () => {
    vi.mocked(flushActiveChartWorkspace).mockRejectedValueOnce(new Error('disk full'))
    const wrapper = mountShell()

    await selectPage(wrapper, 'charts')

    expect(wrapper.find('[data-testid="chart-workspace-page"]').exists()).toBe(true)
    expect(reportError).toHaveBeenCalledOnce()
  })

  it('keeps the primary chrome mounted on every route', async () => {
    const wrapper = mountShell()
    const topBar = wrapper.findComponent(TopBar).element
    const rail = wrapper.findComponent(NavigationRail).element

    await selectPage(wrapper, 'charts')
    await selectPage(wrapper, 'account')

    expect(wrapper.findComponent(TopBar).element).toBe(topBar)
    expect(wrapper.findComponent(NavigationRail).element).toBe(rail)
    expect(wrapper.findComponent(Sidebar).exists()).toBe(true)
  })
})
