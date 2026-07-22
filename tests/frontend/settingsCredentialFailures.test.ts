import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import CredentialEditor from '../../src/components/account/CredentialEditor.vue'
import SettingsDialog from '../../src/components/settings/SettingsDialog.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useConfigStore } from '../../src/stores/config'
import type { AccountProfile, AppConfig } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

const config = {
  activeSymbol: 'BTCUSDT', activeAccountId: 'default', watchlistSymbols: ['BTCUSDT'],
  theme: 'dark', klineInterval: '15', useWebsocket: true,
  wsPublicUrl: '', wsPrivateUrl: '', tickerPollInterval: 1000,
  windowWidth: 1200, windowHeight: 800, accounts: ['default'], riskEnabled: true,
  riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5', riskMaxDailyOrders: 100,
  tradingDayTimezone: 'Asia/Shanghai',
} satisfies AppConfig

const profile: AccountProfile = {
  accountId: 'default', label: 'Main', baseUrl: 'https://api.easicoin.io',
  credentialState: 'present', active: true,
}

const dialogStub = {
  props: ['show'], template: '<section v-if="show"><slot/><slot name="footer"/></section>',
}

describe('CredentialEditor failures', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(tauriInvoke).mockReset()
  })

  function mountEditor() {
    return mount(CredentialEditor, {
      props: {
        show: true, mode: 'edit', accountId: 'primary', initialLabel: 'Main',
        initialBaseUrl: 'https://api.easicoin.io',
      },
      global: { plugins: [pinia], stubs: { AppDialog: dialogStub } },
    })
  }

  it('keeps the draft and dialog open when credential save fails', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('save credential failed'))
    const wrapper = mountEditor()
    await wrapper.findAll('input')[1].setValue('Changed label')
    const save = wrapper.findAll('button').find((button) => button.text() === 'Save')
    await save!.trigger('click')
    await flushPromises()

    expect(wrapper.get('[role="alert"]').text()).toContain('save credential failed')
    expect(wrapper.findAll('input')[1].element).toHaveProperty('value', 'Changed label')
    expect(wrapper.emitted('saved')).toBeUndefined()
    expect(wrapper.emitted('update:show')).toBeUndefined()
  })

  it('keeps entered credentials visible when connection testing fails', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('test connection failed'))
    const wrapper = mountEditor()
    await wrapper.findAll('input')[3].setValue('draft-key')
    await wrapper.findAll('input')[4].setValue('draft-secret')
    const test = wrapper.findAll('button').find((button) => button.text() === 'Test connection')
    await test!.trigger('click')
    await flushPromises()

    expect(wrapper.get('[role="alert"]').text()).toContain('test connection failed')
    expect(wrapper.findAll('input')[3].element).toHaveProperty('value', 'draft-key')
    expect(wrapper.findAll('input')[4].element).toHaveProperty('value', 'draft-secret')
    expect(wrapper.props('show')).toBe(true)
  })

  it('closes as committed when only the background profile refresh fails', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'list_account_profiles') {
        return Promise.reject(new Error('profile refresh failed'))
      }
      return Promise.resolve(undefined)
    })
    const wrapper = mountEditor()
    const save = wrapper.findAll('button').find((button) => button.text() === 'Save')

    await save!.trigger('click')
    await flushPromises()

    expect(wrapper.emitted('saved')).toEqual([['primary']])
    expect(wrapper.emitted('update:show')).toContainEqual([false])
    expect(useAccountProfilesStore().saveError).toBeNull()
    expect(useAccountProfilesStore().listError).toBe('profile refresh failed')
  })
})

describe('settings config failures', () => {
  let pinia: Pinia

  beforeEach(async () => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(tauriInvoke).mockResolvedValueOnce([profile])
    await useAccountProfilesStore().refreshProfiles()
  })

  function mountSettings() {
    return mount(SettingsDialog, {
      props: { show: true },
      global: { plugins: [pinia], stubs: { AppDialog: dialogStub } },
    })
  }

  async function saveThroughEditor(wrapper: ReturnType<typeof mountSettings>) {
    await flushPromises()
    const edit = wrapper.findAll('button').find((button) => button.text() === 'Edit credentials')
    await edit!.trigger('click')
    const save = wrapper.findAll('button').find((button) => button.text() === 'Save')
    await save!.trigger('click')
    await flushPromises()
  }

  it('does not connect when missing config cannot be fetched', async () => {
    useConfigStore().config = null
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'get_config') return Promise.reject(new Error('config fetch failed'))
      return Promise.resolve(undefined)
    })
    const wrapper = mountSettings()
    await saveThroughEditor(wrapper)

    expect(wrapper.get('[role="alert"]').text()).toContain('config fetch failed')
    expect(tauriInvoke).not.toHaveBeenCalledWith('connect', expect.anything())
  })

  it('does not connect when general settings persistence fails', async () => {
    useConfigStore().config = config
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'save_config') return Promise.reject(new Error('config save failed'))
      return Promise.resolve(undefined)
    })
    const wrapper = mountSettings()
    await saveThroughEditor(wrapper)

    expect(wrapper.get('[role="alert"]').text()).toContain('config save failed')
    expect(tauriInvoke).not.toHaveBeenCalledWith('connect', expect.anything())
  })

  it('preserves the user general-settings draft when missing config is fetched', async () => {
    useConfigStore().config = null
    vi.mocked(tauriInvoke).mockImplementation((command, args) => {
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'get_config') {
        return Promise.resolve({ ...config, useWebsocket: true, tickerPollInterval: 1000 })
      }
      if (command === 'save_config') {
        return Promise.resolve((args as { config: AppConfig }).config)
      }
      if (command === 'get_connection_status') return Promise.resolve('disconnected')
      return Promise.resolve(undefined)
    })
    const wrapper = mountSettings()
    await flushPromises()
    await wrapper.get('[role="switch"]').trigger('click')
    await wrapper.get('.n-input-number input').setValue('2500')
    await saveThroughEditor(wrapper)

    expect(tauriInvoke).toHaveBeenCalledWith('save_config', {
      config: expect.objectContaining({ useWebsocket: false, tickerPollInterval: 2500 }),
    })
    expect(tauriInvoke).toHaveBeenCalledWith('connect', {
      startRealtime: false, credential: undefined,
    })
  })
})
