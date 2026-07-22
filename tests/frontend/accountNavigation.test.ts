import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AccountAssetsPanel from '../../src/components/account/AccountAssetsPanel.vue'
import AccountProfilesPanel from '../../src/components/account/AccountProfilesPanel.vue'
import RiskControlPanel from '../../src/components/account/RiskControlPanel.vue'
import DashboardPage from '../../src/components/dashboard/DashboardPage.vue'
import DashboardQuickActions from '../../src/components/dashboard/DashboardQuickActions.vue'
import AppShell from '../../src/components/layout/AppShell.vue'
import NavigationRail from '../../src/components/layout/NavigationRail.vue'
import Sidebar from '../../src/components/layout/Sidebar.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useConnectionStore } from '../../src/stores/connection'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))
vi.mock('../../src/components/layout/TradingLayout.vue', () => ({
  default: { template: '<div data-testid="trading-layout" />' },
}))

describe('account navigation', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve([])
      if (command === 'get_risk_status') return Promise.resolve(null)
      if (command === 'refresh_positions' || command === 'fetch_funding_balances') {
        return Promise.resolve([])
      }
      return Promise.resolve(undefined)
    })
  })

  function mountShell() {
    return mount(AppShell, {
      global: { plugins: [pinia], stubs: { DashboardMarketOverview: true } },
    })
  }

  it('opens account on API and keeps the selected section when returning', async () => {
    const wrapper = mountShell()
    await wrapper.getComponent(NavigationRail).get('button[aria-label="账户"]').trigger('click')
    await flushPromises()

    expect(wrapper.findComponent(AccountProfilesPanel).exists()).toBe(true)
    const sidebar = wrapper.getComponent(Sidebar)
    await sidebar.findAll('.item-btn')[2].trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(RiskControlPanel).exists()).toBe(true)

    await wrapper.getComponent(NavigationRail).get('button[aria-label="交易"]').trigger('click')
    await flushPromises()
    await wrapper.getComponent(NavigationRail).get('button[aria-label="账户"]').trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(RiskControlPanel).exists()).toBe(true)
  })

  it('routes sidebar sections and does not request hidden asset or risk data', async () => {
    useConnectionStore().setStatus('connected')
    const wrapper = mountShell()
    await wrapper.getComponent(NavigationRail).get('button[aria-label="账户"]').trigger('click')
    await flushPromises()

    expect(tauriInvoke).toHaveBeenCalledWith('list_account_profiles')
    expect(tauriInvoke).not.toHaveBeenCalledWith('get_risk_status')
    expect(tauriInvoke).not.toHaveBeenCalledWith('fetch_funding_balances')

    const sidebar = wrapper.getComponent(Sidebar)
    await sidebar.findAll('.item-btn')[1].trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(AccountAssetsPanel).exists()).toBe(true)
    expect(tauriInvoke).toHaveBeenCalledWith('fetch_funding_balances')
    expect(tauriInvoke).not.toHaveBeenCalledWith('get_risk_status')

    await sidebar.findAll('.item-btn')[2].trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(RiskControlPanel).exists()).toBe(true)
    expect(tauriInvoke).toHaveBeenCalledWith('get_risk_status')
  })

  it('defers connected asset requests until the panel becomes active', async () => {
    useConnectionStore().setStatus('connected')
    const wrapper = mount(AccountAssetsPanel, {
      props: { active: false }, global: { plugins: [pinia] },
    })
    await flushPromises()
    expect(tauriInvoke).not.toHaveBeenCalled()

    await wrapper.setProps({ active: true })
    await flushPromises()
    expect(tauriInvoke).toHaveBeenCalledWith('fetch_funding_balances')
  })

  it('maps the dashboard assets action to the account assets section', async () => {
    const shell = mountShell()
    const quickActions = shell.getComponent(DashboardPage).getComponent(DashboardQuickActions)
    await quickActions.findAll('button')[2].trigger('click')
    await flushPromises()
    expect(shell.findComponent(AccountAssetsPanel).exists()).toBe(true)
  })

  it('keeps the settings gear action separate from page navigation', async () => {
    const wrapper = mountShell()
    await wrapper.getComponent(NavigationRail).get('.rail-bottom button').trigger('click')

    expect(wrapper.emitted('openSettings')).toHaveLength(1)
  })
})
