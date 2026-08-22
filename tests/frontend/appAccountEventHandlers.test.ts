import { shallowMount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import App from '../../src/App.vue'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useAccountStore } from '../../src/stores/account'
import type { AppConfig, Balance } from '../../src/types/models'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listeners: new Map<string, (payload: unknown) => void>(),
  closeGuard: vi.fn(),
}))

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: mocks.invoke }))
vi.mock('../../src/composables/useTauriEvent', () => ({
  whenTauriListenersReady: () => Promise.resolve(),
  useTauriEvent: (event: string, handler: (payload: unknown) => void) => {
    mocks.listeners.set(event, handler)
  },
}))
vi.mock('../../src/composables/useChartWorkspaceCloseGuard', () => ({
  useChartWorkspaceCloseGuard: mocks.closeGuard,
}))
vi.mock('../../src/components/layout/AppShell.vue', () => ({
  default: { template: '<div />' },
}))
vi.mock('../../src/components/common/ErrorToastBridge.vue', () => ({
  default: { template: '<div />' },
}))
vi.mock('../../src/components/settings/QuickSetupDialog.vue', () => ({
  default: { template: '<div />' },
}))

const config: AppConfig = {
  activeSymbol: 'BTCUSDT', activeAccountId: 'primary', watchlistSymbols: ['BTCUSDT'],
  theme: 'dark', klineInterval: '15', useWebsocket: false,
  wsPublicUrl: '', wsPrivateUrl: '', tickerPollInterval: 1000,
  windowWidth: 1200, windowHeight: 800, accounts: ['primary'],
  riskEnabled: true, riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5',
  riskMaxDailyOrders: 100, tradingDayTimezone: 'Asia/Shanghai',
}

describe('App account-bound event handlers', () => {
  beforeEach(() => {
    const pinia = createPinia()
    setActivePinia(pinia)
    mocks.listeners.clear()
    mocks.closeGuard.mockReset()
    mocks.invoke.mockReset()
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_config') return Promise.resolve(config)
      if (command === 'list_account_profiles') {
        return Promise.resolve([{
          accountId: 'primary', label: 'Primary', baseUrl: 'https://trade.example',
          credentialState: 'missing', active: true,
        }])
      }
      if (command === 'get_version') return Promise.resolve('test')
      return Promise.resolve(undefined)
    })
    shallowMount(App, { global: { plugins: [pinia] } })
  })

  it('installs the application close guard exactly once', () => {
    expect(mocks.closeGuard).toHaveBeenCalledOnce()
  })

  it('retains every application event subscription', () => {
    expect([...mocks.listeners.keys()]).toEqual([
      'app:ready',
      'connection:status',
      'websocket:status',
      'market:ticker',
      'market:depth',
      'market:kline',
      'order:updated',
      'position:updated',
      'balance:updated',
      'account:snapshot',
      'private-panels:snapshot',
      'daily-pnl:updated',
      'environment:updated',
      'time:updated',
      'error:occurred',
      'log:entry',
    ])
  })

  it('does not write a stale balance event into the account store', () => {
    useAccountProfilesStore().adoptSessionEpoch(2)
    const balance: Balance = {
      asset: 'USDT', available: '10', frozen: '0', total: '10',
    }

    mocks.listeners.get('balance:updated')?.({ sessionEpoch: 1, payload: balance })

    expect(useAccountStore().balances).toEqual([])
  })

  it('accepts the current epoch balance payload', () => {
    useAccountProfilesStore().adoptSessionEpoch(2)
    const balance: Balance = {
      asset: 'USDT', available: '10', frozen: '0', total: '10',
    }

    mocks.listeners.get('balance:updated')?.({ sessionEpoch: 2, payload: balance })

    expect(useAccountStore().balances).toEqual([balance])
  })
})
