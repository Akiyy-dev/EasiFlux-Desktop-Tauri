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
          template: '<section data-testid="settings-content" :data-account-section="initialAccountSection"><h2 v-if="initialSection === \'notifications\'" id="notification-settings-title">通知设置</h2><h1 v-else>{{ initialSection }}</h1></section>',
        },
      },
    },
  })
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
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
      if (section === 'notifications') {
        expect(wrapper.get('#notification-settings-title').text()).toBe('通知设置')
      } else {
        expect(wrapper.get('[data-testid="settings-content"]').text()).toContain(section)
      }
      expect(wrapper.get('[data-testid="settings-content"]').attributes('data-account-section')).toBe(accountSection ?? 'api')
    }
  })

  it('focuses the stable notification settings heading after notification-settings navigation', async () => {
    const wrapper = mountShell()
    wrapper.getComponent(TopBar).vm.$emit('action', { type: 'openNotificationSettings' })
    await flushPromises()
    await wrapper.vm.$nextTick()

    const heading = wrapper.get('#notification-settings-title')
    expect(heading.attributes('tabindex')).toBe('-1')
    expect(document.activeElement).toBe(heading.element)
    wrapper.unmount()
  })

  it('focuses notification settings before an unrelated pending chart flush resolves', async () => {
    const pendingFlush = deferred<void>()
    vi.mocked(flushActiveChartWorkspace).mockReturnValueOnce(pendingFlush.promise)
    const wrapper = mountShell()

    wrapper.getComponent(TopBar).vm.$emit('action', { type: 'openNotificationSettings' })
    await flushPromises()
    await wrapper.vm.$nextTick()

    const heading = wrapper.get('#notification-settings-title')
    expect(heading.attributes('tabindex')).toBe('-1')
    expect(document.activeElement).toBe(heading.element)
    pendingFlush.resolve()
    await flushPromises()
    wrapper.unmount()
  })
})
