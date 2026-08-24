import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import AppShell from '../../src/components/layout/AppShell.vue'
import TopBar from '../../src/components/layout/TopBar.vue'
import { flushActiveChartWorkspace } from '../../src/services/chartWorkspaceFlushRegistry'

vi.mock('../../src/composables/useChartWorkspaceAutosaveHost', () => ({ useChartWorkspaceAutosaveHost: vi.fn() }))
vi.mock('../../src/services/chartWorkspaceFlushRegistry', () => ({ flushActiveChartWorkspace: vi.fn().mockResolvedValue(undefined) }))
vi.mock('../../src/services/errorService', () => ({ reportError: vi.fn() }))
vi.mock('../../src/components/market/KlineChart.vue', () => ({
  default: { template: '<div />' },
}))

function mountShell() {
  return mount(AppShell, {
    attachTo: document.body,
    global: {
      plugins: [createPinia()],
      stubs: {
        TradingLayout: { template: '<div />' },
        ChartWorkspacePage: { template: '<div />' },
        DashboardPage: { template: '<div />' },
        SettingsCenterPage: {
          props: ['initialSection', 'initialAccountSection'],
          template: '<section data-testid="settings-content" :data-account-section="initialAccountSection"><h1>{{ initialSection }}</h1></section>',
        },
      },
    },
  })
}

describe('notification action navigation', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(flushActiveChartWorkspace).mockReset().mockResolvedValue(undefined)
  })

  afterEach(() => {
    document.body.replaceChildren()
  })

  it.each([
    [{ type: 'openTrading' }, 'trading', undefined, undefined],
    [{ type: 'openAccountSettings', accountSection: 'api' }, 'settings', 'account', 'api'],
    [{ type: 'openAccountSettings', accountSection: 'risk' }, 'settings', 'account', 'risk'],
    [{ type: 'openGeneralSettings' }, 'settings', 'general', undefined],
    [{ type: 'openNotificationSettings' }, 'settings', 'notifications', undefined],
  ] as const)('maps %o through object navigation and keeps the chart flush', async (action, page, section, accountSection) => {
    const wrapper = mountShell()
    wrapper.getComponent(TopBar).vm.$emit('action', action)
    await flushPromises()

    expect(flushActiveChartWorkspace).toHaveBeenCalledWith('page')
    if (page === 'trading') {
      expect(wrapper.find('[data-testid="settings-content"]').exists()).toBe(false)
    } else {
      expect(wrapper.get('[data-testid="settings-content"]').text()).toContain(section)
      expect(wrapper.get('[data-testid="settings-content"]').attributes('data-account-section')).toBe(accountSection ?? 'api')
    }
  })

  it('focuses the rendered settings heading after notification-settings navigation without changing the placeholder markup', async () => {
    const wrapper = mountShell()
    wrapper.getComponent(TopBar).vm.$emit('action', { type: 'openNotificationSettings' })
    await flushPromises()
    await wrapper.vm.$nextTick()

    const heading = wrapper.get('[data-testid="settings-content"] h1')
    expect(heading.attributes('tabindex')).toBe('-1')
    expect(document.activeElement).toBe(heading.element)
    wrapper.unmount()
  })
})
