import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { defineComponent, nextTick, ref } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { MessageApi } from 'naive-ui'
import CredentialEditor from '../../src/components/account/CredentialEditor.vue'
import ConnectionStatus from '../../src/components/common/ConnectionStatus.vue'
import QuickSetupDialog from '../../src/components/settings/QuickSetupDialog.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { installMessageApi } from '../../src/services/errorService'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useConfigStore } from '../../src/stores/config'
import { useConnectionStore } from '../../src/stores/connection'
import { useLogStore } from '../../src/stores/log'
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

  function mountQuickSetupWithConnectionStatus() {
    return mount(defineComponent({
      components: { ConnectionStatus, QuickSetupDialog },
      setup: () => ({ show: ref(true) }),
      template: '<QuickSetupDialog v-model:show="show"/><ConnectionStatus/>',
    }), {
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

  it('keeps a notification-aware connection failure inline without generic delivery', async () => {
    const toastError = vi.fn()
    installMessageApi({ error: toastError } as unknown as MessageApi)
    useLogStore().clear()
    useLogStore().clearError()
    useConfigStore().config = config
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'connect') {
        return Promise.reject({
          code: 'CONNECTION_UNAVAILABLE',
          message: '连接服务暂时不可用',
          notificationId: 'notification-quick-setup-1',
        })
      }
      return Promise.resolve(undefined)
    })
    const wrapper = mountQuickSetup()

    await saveThroughEditor(wrapper)

    expect(wrapper.get('[role="alert"]').text()).toContain('连接服务暂时不可用')
    expect(toastError).toHaveBeenCalledTimes(0)
    expect(useLogStore().entries).toHaveLength(0)
    expect(useLogStore().lastError).toBeNull()
  })

  it('delivers an ordinary connection failure through the generic owner exactly once', async () => {
    const toastError = vi.fn()
    installMessageApi({ error: toastError } as unknown as MessageApi)
    useLogStore().clear()
    useLogStore().clearError()
    useConfigStore().config = config
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'connect') return Promise.reject(new Error('ordinary connection failure'))
      return Promise.resolve(undefined)
    })
    const wrapper = mountQuickSetup()

    await saveThroughEditor(wrapper)

    expect(wrapper.get('[role="alert"]').text()).toContain('ordinary connection failure')
    expect(toastError).toHaveBeenCalledTimes(1)
    expect(useLogStore().entries).toHaveLength(1)
    expect(useLogStore().lastError).toContain('ordinary connection failure')
  })

  it('does not connect when a stale session config fetch resolves after reopen', async () => {
    const pendingConfig = deferred<AppConfig>()
    useConfigStore().config = null
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'get_config') return pendingConfig.promise
      if (command === 'connect') return Promise.resolve(undefined)
      if (command === 'get_connection_status') return Promise.resolve('disconnected')
      return Promise.resolve(undefined)
    })
    const wrapper = mountQuickSetup()
    await saveThroughEditor(wrapper)

    await wrapper.get('[data-testid="dialog-close"]').trigger('click')
    await wrapper.setProps({ show: false })
    await wrapper.setProps({ show: true })
    await flushPromises()
    const closesBeforeOldConfig = (wrapper.emitted('update:show') ?? [])
      .filter(([show]) => show === false).length

    pendingConfig.resolve(config)
    await flushPromises()

    expect(tauriInvoke).not.toHaveBeenCalledWith('connect', expect.anything())
    expect((wrapper.emitted('update:show') ?? [])
      .filter(([show]) => show === false)).toHaveLength(closesBeforeOldConfig)
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="quick-setup-retry"]').exists()).toBe(false)
    expect(wrapper.getComponent(CredentialEditor).props('show')).toBe(false)

    await wrapper.findAll('button').find((button) => button.text() === '编辑凭据')!.trigger('click')
    await wrapper.findAll('button').find((button) => button.text() === '保存')!.trigger('click')
    await flushPromises()

    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'connect')).toHaveLength(1)
    expect((wrapper.emitted('update:show') ?? [])
      .filter(([show]) => show === false)).toHaveLength(closesBeforeOldConfig + 1)
  })

  it('does not report a stale profile refresh rejection after reopen succeeds', async () => {
    const firstRefresh = deferred<AccountProfile[]>()
    const freshProfile = { ...profile, label: 'Fresh profile' }
    const toastError = vi.fn()
    installMessageApi({ error: toastError } as unknown as MessageApi)
    useLogStore().clear()
    useLogStore().clearError()
    let listCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') {
        listCalls += 1
        return listCalls === 1 ? firstRefresh.promise : Promise.resolve([freshProfile])
      }
      return Promise.resolve(undefined)
    })
    const wrapper = mountQuickSetup()

    await wrapper.get('[data-testid="dialog-close"]').trigger('click')
    await wrapper.setProps({ show: false })
    await wrapper.setProps({ show: true })
    await flushPromises()

    const store = useAccountProfilesStore()
    expect(store.profiles).toEqual([freshProfile])
    expect(store.listError).toBeNull()
    expect(store.loading).toBe(false)
    expect(wrapper.getComponent(CredentialEditor).props('initialLabel')).toBe('Fresh profile')
    const edit = wrapper.findAll('button').find((button) => button.text() === '编辑凭据')
    expect(edit?.attributes('disabled')).toBeUndefined()

    firstRefresh.reject(new Error('stale apiKey=raw-key apiSecret=raw-secret'))
    await flushPromises()

    expect(toastError).not.toHaveBeenCalled()
    expect(useLogStore().lastError).toBeNull()
    expect(useLogStore().entries).toEqual([])
    expect(store.profiles).toEqual([freshProfile])
    expect(store.listError).toBeNull()
    expect(wrapper.getComponent(CredentialEditor).props('initialLabel')).toBe('Fresh profile')
    expect(wrapper.text()).not.toContain('raw-key')
    expect(wrapper.text()).not.toContain('raw-secret')
  })

  it('does not apply an old connect success after close and reopen without a fresh connect', async () => {
    const pendingConnect = deferred<void>()
    const toastError = vi.fn()
    installMessageApi({ error: toastError } as unknown as MessageApi)
    useConfigStore().config = config
    useLogStore().clear()
    useLogStore().clearError()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'connect') return pendingConnect.promise
      if (command === 'get_connection_status') return Promise.resolve('connected')
      if (command === 'scheduler_run_task') return Promise.resolve(undefined)
      return Promise.resolve(undefined)
    })
    const connectionStore = useConnectionStore()
    connectionStore.setWsStatus('error')
    const host = mountQuickSetupWithConnectionStatus()
    const quickSetup = host.getComponent(QuickSetupDialog)
    await flushPromises()
    await quickSetup.findAll('button')
      .find((button) => button.text() === '编辑凭据')!.trigger('click')
    await quickSetup.findAll('button')
      .find((button) => button.text() === '保存')!.trigger('click')
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('connect', expect.anything()))

    await quickSetup.get('[data-testid="dialog-close"]').trigger('click')
    const hostState = host.vm as unknown as { show: boolean }
    expect(hostState.show).toBe(false)
    hostState.show = true
    await nextTick()
    await flushPromises()
    const closesAtReopen = (quickSetup.emitted('update:show') ?? [])
      .filter(([show]) => show === false).length
    const statusAtReopen = host.getComponent(ConnectionStatus).text()
    const baseline = {
      status: connectionStore.status,
      wsStatus: connectionStore.wsStatus,
      lastError: connectionStore.lastError,
      logLastError: useLogStore().lastError,
      logEntries: [...useLogStore().entries],
    }

    pendingConnect.resolve()
    await flushPromises()

    expect(connectionStore.status).toBe(baseline.status)
    expect(connectionStore.wsStatus).toBe(baseline.wsStatus)
    expect(connectionStore.lastError).toBe(baseline.lastError)
    expect(useLogStore().lastError).toBe(baseline.logLastError)
    expect(useLogStore().entries).toEqual(baseline.logEntries)
    expect(toastError).not.toHaveBeenCalled()
    expect(host.getComponent(ConnectionStatus).text()).toBe(statusAtReopen)
    expect(host.text()).not.toContain('API 已连接')
    expect(host.text()).not.toContain('raw-key')
    expect(host.text()).not.toContain('raw-secret')
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'get_connection_status')).toHaveLength(0)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(0)
    expect((quickSetup.emitted('update:show') ?? [])
      .filter(([show]) => show === false)).toHaveLength(closesAtReopen)
  })

  it('does not apply or expose an old connect failure after close and reopen', async () => {
    const pendingConnect = deferred<void>()
    const toastError = vi.fn()
    installMessageApi({ error: toastError } as unknown as MessageApi)
    useConfigStore().config = config
    useLogStore().clear()
    useLogStore().clearError()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'connect') return pendingConnect.promise
      return Promise.resolve(undefined)
    })
    const connectionStore = useConnectionStore()
    connectionStore.setWsStatus('error')
    const host = mountQuickSetupWithConnectionStatus()
    const quickSetup = host.getComponent(QuickSetupDialog)
    await flushPromises()
    await quickSetup.findAll('button')
      .find((button) => button.text() === '编辑凭据')!.trigger('click')
    await quickSetup.findAll('button')
      .find((button) => button.text() === '保存')!.trigger('click')
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('connect', expect.anything()))

    await quickSetup.get('[data-testid="dialog-close"]').trigger('click')
    const hostState = host.vm as unknown as { show: boolean }
    expect(hostState.show).toBe(false)
    hostState.show = true
    await nextTick()
    await flushPromises()
    const closesAtReopen = (quickSetup.emitted('update:show') ?? [])
      .filter(([show]) => show === false).length
    const statusAtReopen = host.getComponent(ConnectionStatus).text()
    const baseline = {
      status: connectionStore.status,
      wsStatus: connectionStore.wsStatus,
      lastError: connectionStore.lastError,
      logLastError: useLogStore().lastError,
      logEntries: [...useLogStore().entries],
    }

    pendingConnect.reject(new Error('apiKey=raw-key apiSecret=raw-secret'))
    await flushPromises()

    expect(connectionStore.status).toBe(baseline.status)
    expect(connectionStore.wsStatus).toBe(baseline.wsStatus)
    expect(connectionStore.lastError).toBe(baseline.lastError)
    expect(useLogStore().lastError).toBe(baseline.logLastError)
    expect(useLogStore().entries).toEqual(baseline.logEntries)
    expect(toastError).not.toHaveBeenCalled()
    expect(host.getComponent(ConnectionStatus).text()).toBe(statusAtReopen)
    expect(host.text()).not.toContain('raw-key')
    expect(host.text()).not.toContain('raw-secret')
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'get_connection_status')).toHaveLength(0)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(0)
    expect((quickSetup.emitted('update:show') ?? [])
      .filter(([show]) => show === false)).toHaveLength(closesAtReopen)
  })

  it('keeps fresh global connection state when an older session connect rejects late', async () => {
    const firstConnect = deferred<void>()
    let connectCalls = 0
    useConfigStore().config = config
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'connect') {
        connectCalls += 1
        return connectCalls === 1 ? firstConnect.promise : Promise.resolve(undefined)
      }
      if (command === 'get_connection_status') return Promise.resolve('connected')
      if (command === 'scheduler_run_task') return Promise.resolve(undefined)
      return Promise.resolve(undefined)
    })
    const host = mountQuickSetupWithConnectionStatus()
    const quickSetup = host.getComponent(QuickSetupDialog)
    await flushPromises()
    await quickSetup.findAll('button')
      .find((button) => button.text() === '编辑凭据')!.trigger('click')
    await quickSetup.findAll('button')
      .find((button) => button.text() === '保存')!.trigger('click')
    await flushPromises()

    await quickSetup.get('[data-testid="dialog-close"]').trigger('click')
    const hostState = host.vm as unknown as { show: boolean }
    hostState.show = true
    await nextTick()
    await flushPromises()
    await quickSetup.findAll('button')
      .find((button) => button.text() === '编辑凭据')!.trigger('click')
    await quickSetup.findAll('button')
      .find((button) => button.text() === '保存')!.trigger('click')
    await flushPromises()

    const store = useConnectionStore()
    const closesAfterFreshConnect = (quickSetup.emitted('update:show') ?? [])
      .filter(([show]) => show === false).length
    expect(connectCalls).toBe(2)
    expect(closesAfterFreshConnect).toBe(2)
    expect(store.status).toBe('connected')
    expect(store.lastError).toBeNull()
    expect(host.getComponent(ConnectionStatus).text()).toContain('API 已连接')

    firstConnect.reject(new Error('apiKey=raw-key apiSecret=raw-secret'))
    await flushPromises()

    expect(store.status).toBe('connected')
    expect(store.lastError).toBeNull()
    expect(host.getComponent(ConnectionStatus).text()).toContain('API 已连接')
    expect(host.text()).not.toContain('raw-key')
    expect(host.text()).not.toContain('raw-secret')
    expect((quickSetup.emitted('update:show') ?? [])
      .filter(([show]) => show === false)).toHaveLength(closesAfterFreshConnect)
  })

  it('handles duplicate saved events as one connection flow', async () => {
    const pendingConnect = deferred<void>()
    useConfigStore().config = null
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'get_config') return Promise.resolve(config)
      if (command === 'connect') return pendingConnect.promise
      if (command === 'get_connection_status') return Promise.resolve('connected')
      if (command === 'scheduler_run_task') return Promise.resolve(undefined)
      return Promise.resolve(undefined)
    })
    const wrapper = mountQuickSetup()
    await flushPromises()
    await wrapper.findAll('button').find((button) => button.text() === '编辑凭据')!.trigger('click')
    const editor = wrapper.getComponent(CredentialEditor)

    editor.vm.$emit('saved', 'default')
    editor.vm.$emit('saved', 'default')
    await flushPromises()

    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'get_config')).toHaveLength(1)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'connect')).toHaveLength(1)

    pendingConnect.resolve()
    await flushPromises()

    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(1)
    expect((wrapper.emitted('update:show') ?? [])
      .filter(([show]) => show === false)).toHaveLength(1)
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
