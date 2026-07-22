import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useConfigStore } from '../../src/stores/config'
import { useConnectionStore } from '../../src/stores/connection'
import type { AccountSwitchResult, AppConfig } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

const config: AppConfig = {
  activeSymbol: 'BTCUSDT', activeAccountId: 'primary', watchlistSymbols: ['BTCUSDT'],
  theme: 'dark', klineInterval: '15', useWebsocket: false,
  wsPublicUrl: '', wsPrivateUrl: '', tickerPollInterval: 1000,
  windowWidth: 1200, windowHeight: 800, accounts: ['primary', 'backup', 'tertiary'],
  riskEnabled: true, riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5',
  riskMaxDailyOrders: 100, tradingDayTimezone: 'Asia/Shanghai',
}

describe('account reconciliation serialization', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    useConfigStore().config = config
    vi.mocked(tauriInvoke).mockReset()
  })

  it('requires the old failed context to reconcile before a new backend switch', async () => {
    const secondSwitch = deferred<AccountSwitchResult>()
    let switchCalls = 0
    let configCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        switchCalls += 1
        return switchCalls === 1
          ? Promise.resolve({ activeAccountId: 'backup', connected: false, sessionEpoch: 1 })
          : secondSwitch.promise
      }
      if (command === 'get_config') {
        configCalls += 1
        if (configCalls === 1) return Promise.reject(new Error('first reconciliation failed'))
        return Promise.resolve({
          ...config,
          activeAccountId: configCalls === 2 ? 'backup' : 'tertiary',
        })
      }
      if (command === 'list_account_profiles') return Promise.resolve([])
      if (command === 'get_connection_status') return Promise.resolve('disconnected')
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    await store.switchAccount('backup')
    expect(store.reconciliationError).not.toBeNull()

    await expect(store.switchAccount('tertiary')).rejects.toThrow('temporarily unavailable')
    expect(switchCalls).toBe(1)
    await store.retryReconciliation()
    expect(store.reconciliationError).toBeNull()

    const switching = store.switchAccount('tertiary')
    await store.retryReconciliation()
    expect(configCalls).toBe(2)

    secondSwitch.resolve({ activeAccountId: 'tertiary', connected: false, sessionEpoch: 2 })
    await switching
    expect(configCalls).toBe(3)
    expect(store.activeAccountId).toBe('tertiary')
    expect(store.reconciliationError).toBeNull()
  })

  it('prevents an older connection-status response from overwriting switch reconciliation', async () => {
    const staleStatus = deferred<'disconnected'>()
    let statusCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'get_connection_status') {
        statusCalls += 1
        return statusCalls === 1 ? staleStatus.promise : Promise.resolve('connected')
      }
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 1 })
      }
      if (command === 'get_config') return Promise.resolve({
        ...config, activeAccountId: 'backup',
      })
      if (command === 'list_account_profiles') return Promise.resolve([])
      return Promise.resolve(undefined)
    })
    const connectionStore = useConnectionStore()
    const staleRefresh = connectionStore.refreshStatus()
    await useAccountProfilesStore().switchAccount('backup')
    staleStatus.resolve('disconnected')
    await staleRefresh

    expect(connectionStore.status).toBe('connected')
  })
})
