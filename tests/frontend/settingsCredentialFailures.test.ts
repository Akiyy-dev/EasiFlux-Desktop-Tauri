import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import CredentialEditor from '../../src/components/account/CredentialEditor.vue'
import QuickSetupDialog from '../../src/components/settings/QuickSetupDialog.vue'
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
  props: ['show'],
  emits: ['update:show'],
  template: `<section v-if="show">
    <button data-testid="dialog-close" @click="$emit('update:show', false)">close</button>
    <slot/><slot name="footer"/>
  </section>`,
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
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
    const save = wrapper.findAll('button').find((button) => button.text() === '保存')
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
    const test = wrapper.findAll('button').find((button) => button.text() === '测试连接')
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
    const save = wrapper.findAll('button').find((button) => button.text() === '保存')

    await save!.trigger('click')
    await flushPromises()

    expect(wrapper.emitted('saved')).toEqual([['primary']])
    expect(wrapper.emitted('update:show')).toContainEqual([false])
    expect(useAccountProfilesStore().saveError).toBeNull()
    expect(useAccountProfilesStore().listError).toBe('profile refresh failed')
  })
})

describe('QuickSetup connection failures', () => {
  let pinia: Pinia

  beforeEach(async () => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(tauriInvoke).mockResolvedValueOnce([profile])
    await useAccountProfilesStore().refreshProfiles()
  })

  function mountQuickSetup() {
    return mount(QuickSetupDialog, {
      props: { show: true },
      global: { plugins: [pinia], stubs: { AppDialog: dialogStub } },
    })
  }

  async function saveThroughEditor(wrapper: ReturnType<typeof mountQuickSetup>) {
    await flushPromises()
    const edit = wrapper.findAll('button').find((button) => button.text() === '编辑凭据')
    await edit!.trigger('click')
    const save = wrapper.findAll('button').find((button) => button.text() === '保存')
    await save!.trigger('click')
    await flushPromises()
  }

  it('keeps saved credentials committed when authoritative config cannot be fetched', async () => {
    useConfigStore().config = null
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'get_config') return Promise.reject(new Error('config fetch failed'))
      return Promise.resolve(undefined)
    })
    const wrapper = mountQuickSetup()
    await saveThroughEditor(wrapper)

    expect(tauriInvoke).toHaveBeenCalledWith('save_credentials', expect.anything())
    expect(wrapper.get('[role="alert"]').text()).toContain('config fetch failed')
    expect(tauriInvoke).not.toHaveBeenCalledWith('connect', expect.anything())
    expect(wrapper.findComponent(CredentialEditor).exists()).toBe(false)
    expect(wrapper.get('[data-testid="quick-setup-retry"]').exists()).toBe(true)
  })

  it('resets a saved/error/retry session after close and reopen', async () => {
    useConfigStore().config = config
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'connect') return Promise.reject(new Error('connect failed'))
      return Promise.resolve(undefined)
    })
    const wrapper = mountQuickSetup()
    await saveThroughEditor(wrapper)

    expect(wrapper.text()).toContain('凭据已保存，连接失败')
    expect(wrapper.get('[data-testid="quick-setup-retry"]').exists()).toBe(true)

    await wrapper.get('[data-testid="dialog-close"]').trigger('click')
    await wrapper.setProps({ show: false })
    await wrapper.setProps({ show: true })
    await flushPromises()

    expect(wrapper.text()).not.toContain('connect failed')
    expect(wrapper.find('[data-testid="quick-setup-retry"]').exists()).toBe(false)
    expect(wrapper.getComponent(CredentialEditor).props('show')).toBe(false)
    expect(wrapper.findAll('button').some((button) => button.text() === '编辑凭据')).toBe(true)
  })

  it('ignores a connection success that resolves after a newer dialog session opens', async () => {
    const pending = deferred<void>()
    useConfigStore().config = config
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'connect') return pending.promise
      return Promise.resolve(undefined)
    })
    const wrapper = mountQuickSetup()
    await saveThroughEditor(wrapper)

    await wrapper.get('[data-testid="dialog-close"]').trigger('click')
    await wrapper.setProps({ show: false })
    await wrapper.setProps({ show: true })
    await flushPromises()
    const closesBeforeOldCompletion = (wrapper.emitted('update:show') ?? [])
      .filter(([show]) => show === false).length

    pending.resolve()
    await flushPromises()

    const closesAfterOldCompletion = (wrapper.emitted('update:show') ?? [])
      .filter(([show]) => show === false).length
    expect(closesAfterOldCompletion).toBe(closesBeforeOldCompletion)
    expect(wrapper.find('[data-testid="quick-setup-retry"]').exists()).toBe(false)
    expect(wrapper.getComponent(CredentialEditor).props('show')).toBe(false)
  })

  it('ignores a connection rejection that settles after a newer dialog session opens', async () => {
    const pending = deferred<void>()
    useConfigStore().config = config
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'connect') return pending.promise
      return Promise.resolve(undefined)
    })
    const wrapper = mountQuickSetup()
    await saveThroughEditor(wrapper)

    await wrapper.get('[data-testid="dialog-close"]').trigger('click')
    await wrapper.setProps({ show: false })
    await wrapper.setProps({ show: true })
    await flushPromises()

    pending.reject(new Error('late apiKey=raw-key apiSecret=raw-secret'))
    await flushPromises()

    expect(wrapper.text()).not.toContain('late')
    expect(wrapper.text()).not.toContain('raw-key')
    expect(wrapper.text()).not.toContain('raw-secret')
    expect(wrapper.find('[data-testid="quick-setup-retry"]').exists()).toBe(false)
    expect(wrapper.getComponent(CredentialEditor).props('show')).toBe(false)
  })
})
