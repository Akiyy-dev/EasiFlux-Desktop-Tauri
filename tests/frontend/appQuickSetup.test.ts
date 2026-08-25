import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from '../../src/App.vue'
import AppShell from '../../src/components/layout/AppShell.vue'
import type { AccountProfile, AppConfig } from '../../src/types/models'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  reportError: vi.fn((error: unknown, context?: string) => {
    const detail = error instanceof Error ? error.message : String(error)
    return context ? `${context}: ${detail}` : detail
  }),
}))

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: mocks.invoke }))
vi.mock('../../src/composables/useTauriEvent', () => ({
  whenTauriListenersReady: () => Promise.resolve(),
  useTauriEvent: vi.fn(),
}))
vi.mock('../../src/composables/useAccountSessionEvent', () => ({
  useAccountSessionEvent: vi.fn(),
}))
vi.mock('../../src/composables/useChartWorkspaceCloseGuard', () => ({
  useChartWorkspaceCloseGuard: vi.fn(),
}))
vi.mock('../../src/composables/useChartWorkspaceAutosaveHost', () => ({
  useChartWorkspaceAutosaveHost: vi.fn(),
}))
vi.mock('../../src/components/dashboard/DashboardPage.vue', () => ({
  default: { name: 'DashboardPage', template: '<div />' },
}))
vi.mock('../../src/components/layout/TradingLayout.vue', () => ({
  default: { name: 'TradingLayout', template: '<div />' },
}))
vi.mock('../../src/components/chart/ChartWorkspacePage.vue', () => ({
  default: { name: 'ChartWorkspacePage', template: '<div />' },
}))
vi.mock('../../src/components/settings/SettingsCenterPage.vue', () => ({
  default: {
    name: 'SettingsCenterPage',
    template: '<section data-testid="settings-center-stub">Settings Center</section>',
  },
}))
vi.mock('../../src/services/errorService', () => ({
  reportError: mocks.reportError,
  installMessageApi: vi.fn(),
  notifyInfo: vi.fn(),
  notifySuccess: vi.fn(),
  notifyWarning: vi.fn(),
}))

const config: AppConfig = {
  activeSymbol: 'BTCUSDT', activeAccountId: 'primary', watchlistSymbols: ['BTCUSDT'],
  theme: 'dark', klineInterval: '15', useWebsocket: false,
  wsPublicUrl: '', wsPrivateUrl: '', tickerPollInterval: 1000,
  windowWidth: 1200, windowHeight: 800, accounts: ['primary'],
  riskEnabled: true, riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5',
  riskMaxDailyOrders: 100, tradingDayTimezone: 'Asia/Shanghai',
}

function profile(
  credentialState: AccountProfile['credentialState'],
  accountId = 'primary',
): AccountProfile {
  return {
    accountId,
    label: accountId === 'primary' ? 'Primary account' : 'Other account',
    baseUrl: 'https://trade.example',
    credentialState,
    active: accountId === 'primary',
  }
}

function installStartupResponses(options: {
  configError?: Error
  profiles?: AccountProfile[]
  profileError?: Error
  connectError?: unknown
} = {}): void {
  mocks.invoke.mockImplementation((command: string) => {
    if (command === 'get_version') return Promise.resolve('test-version')
    if (command === 'get_config') {
      return options.configError ? Promise.reject(options.configError) : Promise.resolve(config)
    }
    if (command === 'list_account_profiles') {
      return options.profileError
        ? Promise.reject(options.profileError)
        : Promise.resolve(options.profiles ?? [profile('present')])
    }
    if (command === 'connect') {
      return options.connectError ? Promise.reject(options.connectError) : Promise.resolve()
    }
    if (command === 'get_connection_status') return Promise.resolve('disconnected')
    if (command === 'fetch_instruments') return Promise.resolve([])
    if (command === 'get_environment_status') return Promise.resolve({})
    return Promise.resolve()
  })
}

function mountStartupApp(pinia: Pinia) {
  return mount(App, {
    global: {
      plugins: [pinia],
      stubs: {
        AppShell: true,
        ErrorToastBridge: true,
        QuickSetupDialog: {
          name: 'QuickSetupDialog',
          props: ['show'],
          template: '<div data-testid="quick-setup-stub" :data-show="String(show)" />',
        },
      },
    },
  })
}

async function settleStartup(): Promise<void> {
  await flushPromises()
  await flushPromises()
}

function quickSetupIsVisible(wrapper: ReturnType<typeof mountStartupApp>): boolean {
  return wrapper.get('[data-testid="quick-setup-stub"]').attributes('data-show') === 'true'
}

describe('App credential-aware startup', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    mocks.invoke.mockReset()
    mocks.reportError.mockClear()
  })

  it('opens QuickSetup only for an authoritative active profile with missing credentials', async () => {
    installStartupResponses({ profiles: [profile('missing')] })
    const wrapper = mountStartupApp(pinia)

    await settleStartup()

    expect(quickSetupIsVisible(wrapper)).toBe(true)
    expect(mocks.invoke).not.toHaveBeenCalledWith('connect', expect.anything())
    expect(mocks.invoke.mock.calls.map(([command]) => command)).not.toContain('has_credentials')
  })

  it('connects a present profile with the saved WebSocket preference', async () => {
    installStartupResponses({ profiles: [profile('present')] })
    const wrapper = mountStartupApp(pinia)

    await settleStartup()

    expect(quickSetupIsVisible(wrapper)).toBe(false)
    expect(mocks.invoke).toHaveBeenCalledWith('connect', {
      startRealtime: false,
      credential: undefined,
    })
    const commands = mocks.invoke.mock.calls.map(([command]) => command)
    expect(commands.indexOf('get_config')).toBeLessThan(commands.indexOf('list_account_profiles'))
    expect(commands.indexOf('list_account_profiles')).toBeLessThan(commands.indexOf('connect'))
    expect(commands).not.toContain('has_credentials')
  })

  it('reports unavailable credential storage without opening QuickSetup or connecting', async () => {
    installStartupResponses({ profiles: [profile('unavailable')] })
    const wrapper = mountStartupApp(pinia)

    await settleStartup()

    expect(quickSetupIsVisible(wrapper)).toBe(false)
    expect(mocks.invoke).not.toHaveBeenCalledWith('connect', expect.anything())
    expect(mocks.reportError).toHaveBeenCalledWith(
      expect.objectContaining({ message: '凭据存储不可用' }),
      '读取账户凭据失败',
    )
  })

  it('reports config load failure without opening QuickSetup', async () => {
    const error = new Error('config failed')
    installStartupResponses({ configError: error })
    const wrapper = mountStartupApp(pinia)

    await settleStartup()

    expect(quickSetupIsVisible(wrapper)).toBe(false)
    expect(mocks.reportError).toHaveBeenCalledWith(error, '加载配置失败')
    expect(mocks.invoke).not.toHaveBeenCalledWith('connect', expect.anything())
  })

  it('reports profile-list failure without opening QuickSetup', async () => {
    const error = new Error('profiles failed')
    installStartupResponses({ profileError: error })
    const wrapper = mountStartupApp(pinia)

    await settleStartup()

    expect(quickSetupIsVisible(wrapper)).toBe(false)
    expect(mocks.reportError).toHaveBeenCalledWith(error, '加载账户配置失败')
    expect(mocks.invoke).not.toHaveBeenCalledWith('connect', expect.anything())
  })

  it('reports an absent configured active account without opening QuickSetup', async () => {
    installStartupResponses({ profiles: [profile('missing', 'other')] })
    const wrapper = mountStartupApp(pinia)

    await settleStartup()

    expect(quickSetupIsVisible(wrapper)).toBe(false)
    expect(mocks.reportError).toHaveBeenCalledWith(
      expect.objectContaining({ message: '活动账户不在账户列表中' }),
      '加载活动账户失败',
    )
    expect(mocks.invoke).not.toHaveBeenCalledWith('connect', expect.anything())
  })

  it('reports automatic connection failure without opening QuickSetup', async () => {
    const error = new Error('connect failed')
    installStartupResponses({ profiles: [profile('present')], connectError: error })
    const wrapper = mountStartupApp(pinia)

    await settleStartup()

    expect(quickSetupIsVisible(wrapper)).toBe(false)
    expect(mocks.reportError).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'connect failed' }),
      '自动连接失败',
    )
    expect(mocks.reportError).toHaveBeenCalledTimes(1)
  })

  it('leaves notification-aware automatic connection failure Rust-owned', async () => {
    installStartupResponses({
      profiles: [profile('present')],
      connectError: {
        code: 'CONNECTION_UNAVAILABLE',
        message: '连接服务暂时不可用',
        notificationId: 'notification-auto-connect-1',
      },
    })
    mountStartupApp(pinia)

    await settleStartup()

    expect(mocks.reportError).not.toHaveBeenCalled()
  })

  it('keeps QuickSetup closed when the normal settings gear opens Settings Center', async () => {
    installStartupResponses({ profiles: [profile('present')] })
    const wrapper = mount(App, {
      global: {
        plugins: [pinia],
        stubs: {
          ErrorToastBridge: true,
          QuickSetupDialog: {
            props: ['show'],
            template: '<div data-testid="quick-setup-stub" :data-show="String(show)" />',
          },
        },
      },
    })
    await settleStartup()

    await wrapper.get('button[aria-label="设置"]').trigger('click')

    expect(wrapper.getComponent(AppShell).exists()).toBe(true)
    expect(wrapper.get('[data-testid="settings-center-stub"]').exists()).toBe(true)
    expect(wrapper.get('[data-testid="quick-setup-stub"]').attributes('data-show')).toBe('false')
  })
})
