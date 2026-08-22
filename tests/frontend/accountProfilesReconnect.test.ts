import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AccountProfilesPanel from '../../src/components/account/AccountProfilesPanel.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useConfigStore } from '../../src/stores/config'
import { useConnectionStore } from '../../src/stores/connection'
import type { AccountProfile, AppConfig } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

const profiles: AccountProfile[] = [
  {
    accountId: 'primary', label: 'Primary', baseUrl: 'https://primary.example',
    credentialState: 'present', active: true,
  },
  {
    accountId: 'backup', label: 'Backup', baseUrl: 'https://backup.example',
    credentialState: 'present', active: false,
  },
]

const appConfig: AppConfig = {
  activeSymbol: 'BTCUSDT',
  activeAccountId: 'primary',
  watchlistSymbols: ['BTCUSDT'],
  theme: 'dark',
  klineInterval: '15',
  useWebsocket: true,
  wsPublicUrl: 'wss://example.test/public',
  wsPrivateUrl: 'wss://example.test/private',
  tickerPollInterval: 1000,
  windowWidth: 1200,
  windowHeight: 800,
  accounts: ['primary', 'backup'],
  riskEnabled: true,
  riskMaxOrderQty: '10',
  riskMaxPriceDeviationPct: '5',
  riskMaxDailyOrders: 100,
  tradingDayTimezone: 'Asia/Shanghai',
}

const credentialEditorStub = {
  name: 'CredentialEditor',
  props: [
    'show', 'mode', 'accountId', 'initialLabel', 'initialBaseUrl', 'requireCredentials',
  ],
  emits: ['saved', 'update:show'],
  template: '<div data-testid="credential-editor" />',
}

describe('AccountProfilesPanel credential reconnect', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    useConfigStore().config = { ...appConfig }
    useConnectionStore().setStatus('connected')
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve(profiles)
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
  })

  function mountPanel(): VueWrapper {
    return mount(AccountProfilesPanel, {
      global: {
        plugins: [pinia],
        stubs: { CredentialEditor: credentialEditorStub },
      },
    })
  }

  async function emitSaved(wrapper: VueWrapper, accountId: string): Promise<void> {
    wrapper.findComponent({ name: 'CredentialEditor' }).vm.$emit('saved', accountId)
    await wrapper.vm.$nextTick()
  }

  it('waits for an explicit action before reconnecting with authoritative websocket mode', async () => {
    const wrapper = mountPanel()
    await flushPromises()
    vi.mocked(tauriInvoke).mockClear()

    await emitSaved(wrapper, 'primary')

    expect(wrapper.text()).toContain('凭据已保存，重新连接后生效')
    expect(tauriInvoke).not.toHaveBeenCalledWith('disconnect')
    expect(tauriInvoke).not.toHaveBeenCalledWith('connect', expect.anything())

    await wrapper.get('[data-testid="account-reconnect"]').trigger('click')
    await flushPromises()

    const commands = vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)
    expect(commands.indexOf('disconnect')).toBeGreaterThanOrEqual(0)
    expect(commands.indexOf('connect')).toBeGreaterThan(commands.indexOf('disconnect'))
    expect(tauriInvoke).toHaveBeenCalledWith('connect', {
      startRealtime: appConfig.useWebsocket,
      credential: undefined,
    })
    expect(tauriInvoke).not.toHaveBeenCalledWith('save_credentials', expect.anything())
    expect(wrapper.find('[data-testid="account-reconnect"]').exists()).toBe(false)
  })

  it('keeps saved status, connection error, and retry action after reconnect failure', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve(profiles)
      if (command === 'connect') return Promise.reject(new Error('socket refused'))
      return Promise.resolve(undefined)
    })
    const wrapper = mountPanel()
    await flushPromises()
    vi.mocked(tauriInvoke).mockClear()
    await emitSaved(wrapper, 'primary')

    await wrapper.get('[data-testid="account-reconnect"]').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('凭据已保存，重新连接后生效')
    expect(wrapper.get('[role="alert"]').text()).toContain('账户重新连接失败')
    expect(wrapper.get('[role="alert"]').text()).toContain('socket refused')
    expect(wrapper.get('[data-testid="account-reconnect"]').exists()).toBe(true)
    expect(tauriInvoke).not.toHaveBeenCalledWith('save_credentials', expect.anything())

    await wrapper.get('[data-testid="account-reconnect"]').trigger('click')
    await flushPromises()
    expect(tauriInvoke).not.toHaveBeenCalledWith('save_credentials', expect.anything())
  })

  it('does not offer reconnect for credentials saved on a non-active account', async () => {
    const wrapper = mountPanel()
    await flushPromises()

    await emitSaved(wrapper, 'backup')

    expect(wrapper.text()).not.toContain('凭据已保存，重新连接后生效')
    expect(wrapper.find('[data-testid="account-reconnect"]').exists()).toBe(false)
  })

  it('fetches missing config before reconnecting and uses returned false websocket mode', async () => {
    useConfigStore().config = null
    const wrapper = mountPanel()
    await flushPromises()
    await emitSaved(wrapper, 'default')
    vi.mocked(tauriInvoke).mockClear()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'get_config') {
        return Promise.resolve({
          ...appConfig,
          activeAccountId: 'default',
          useWebsocket: false,
        })
      }
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })

    await wrapper.get('[data-testid="account-reconnect"]').trigger('click')
    await flushPromises()

    const commands = vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)
    expect(commands.indexOf('get_config')).toBeGreaterThanOrEqual(0)
    expect(commands.indexOf('disconnect')).toBeGreaterThan(commands.indexOf('get_config'))
    expect(tauriInvoke).toHaveBeenCalledWith('connect', {
      startRealtime: false,
      credential: undefined,
    })
  })

  it('clears the action when the authoritative active account changes', async () => {
    const wrapper = mountPanel()
    await flushPromises()
    await emitSaved(wrapper, 'primary')
    vi.mocked(tauriInvoke).mockClear()

    useConfigStore().adoptActiveAccountId('backup')
    await flushPromises()

    expect(wrapper.find('[data-testid="account-reconnect"]').exists()).toBe(false)
    expect(tauriInvoke).not.toHaveBeenCalledWith('disconnect')
    expect(tauriInvoke).not.toHaveBeenCalledWith('connect', expect.anything())
  })

  it('rechecks the bound account after an in-flight config fetch', async () => {
    const configGate = deferred<AppConfig>()
    useConfigStore().config = null
    const wrapper = mountPanel()
    await flushPromises()
    await emitSaved(wrapper, 'default')
    vi.mocked(tauriInvoke).mockClear()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'get_config') return configGate.promise
      return Promise.resolve(undefined)
    })

    await wrapper.get('[data-testid="account-reconnect"]').trigger('click')
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('get_config'))
    useConfigStore().config = { ...appConfig, activeAccountId: 'backup' }
    await flushPromises()
    configGate.resolve({
      ...appConfig,
      activeAccountId: 'default',
      useWebsocket: false,
    })
    await flushPromises()

    expect(wrapper.find('[data-testid="account-reconnect"]').exists()).toBe(false)
    expect(tauriInvoke).not.toHaveBeenCalledWith('disconnect')
    expect(tauriInvoke).not.toHaveBeenCalledWith('connect', expect.anything())
  })

  it('clears pending reconnect after an external disconnect', async () => {
    const wrapper = mountPanel()
    await flushPromises()
    await emitSaved(wrapper, 'primary')

    useConnectionStore().setStatus('disconnected')
    await flushPromises()

    expect(wrapper.find('[data-testid="account-reconnect"]').exists()).toBe(false)
  })
})
