import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AccountSettingsPage from '../../src/components/settings/AccountSettingsPage.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useConnectionStore } from '../../src/stores/connection'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

describe('AccountSettingsPage', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    useConnectionStore().setStatus('connected')
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve([])
      if (command === 'refresh_positions') return Promise.resolve([])
      if (command === 'fetch_funding_balances') return Promise.resolve([])
      if (command === 'get_risk_status') return Promise.resolve(null)
      return Promise.resolve(undefined)
    })
  })

  it('mounts only the API panel by default', async () => {
    const wrapper = mount(AccountSettingsPage, {
      global: { plugins: [pinia] },
    })
    await flushPromises()

    expect(wrapper.text()).toContain('账户管理')
    expect(tauriInvoke).toHaveBeenCalledWith('list_account_profiles')
    expect(tauriInvoke).not.toHaveBeenCalledWith('fetch_funding_balances')
    expect(tauriInvoke).not.toHaveBeenCalledWith('get_risk_status')
  })

  it('mounts only the assets panel for an assets deep link', async () => {
    const wrapper = mount(AccountSettingsPage, {
      props: { initialSection: 'assets' },
      global: { plugins: [pinia] },
    })
    await flushPromises()

    expect(wrapper.text()).toContain('资产概览')
    expect(tauriInvoke).toHaveBeenCalledWith('fetch_funding_balances')
    expect(tauriInvoke).not.toHaveBeenCalledWith('list_account_profiles')
    expect(tauriInvoke).not.toHaveBeenCalledWith('get_risk_status')
  })

  it('mounts only the risk panel for a risk deep link', async () => {
    const wrapper = mount(AccountSettingsPage, {
      props: { initialSection: 'risk' },
      global: { plugins: [pinia] },
    })
    await flushPromises()

    expect(wrapper.text()).toContain('每日风控')
    expect(tauriInvoke).toHaveBeenCalledWith('get_risk_status')
    expect(tauriInvoke).not.toHaveBeenCalledWith('list_account_profiles')
    expect(tauriInvoke).not.toHaveBeenCalledWith('fetch_funding_balances')
  })

  it('replaces the active panel when the parent deep link changes', async () => {
    const wrapper = mount(AccountSettingsPage, {
      props: { initialSection: 'assets' },
      global: { plugins: [pinia] },
    })
    await flushPromises()
    vi.mocked(tauriInvoke).mockClear()

    await wrapper.setProps({ initialSection: 'api' })
    await flushPromises()

    expect(wrapper.text()).toContain('账户管理')
    expect(wrapper.text()).not.toContain('资产概览')
    expect(tauriInvoke).toHaveBeenCalledWith('list_account_profiles')
    expect(tauriInvoke).not.toHaveBeenCalledWith('fetch_funding_balances')
  })

  it('mounts only the risk panel after clicking the third tab', async () => {
    const wrapper = mount(AccountSettingsPage, {
      props: { initialSection: 'api' },
      global: { plugins: [pinia] },
    })
    await flushPromises()
    vi.mocked(tauriInvoke).mockClear()

    await wrapper.findAll('[role="tab"]')[2].trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('每日风控')
    expect(wrapper.text()).not.toContain('账户管理')
    expect(wrapper.text()).not.toContain('资产概览')
    expect(tauriInvoke).toHaveBeenCalledWith('get_risk_status')
    expect(tauriInvoke).not.toHaveBeenCalledWith('fetch_funding_balances')
  })
})
