import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AccountProfilesPanel from '../../src/components/account/AccountProfilesPanel.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useAccountStore } from '../../src/stores/account'
import { useConfigStore } from '../../src/stores/config'
import { useOrderStore } from '../../src/stores/order'
import { usePositionStore } from '../../src/stores/position'
import { applyPrivatePanelsSnapshot, usePrivatePanelsState } from '../../src/stores/privatePanels'
import type { AccountProfile, AccountSwitchResult, AppConfig } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((ok, fail) => {
    resolve = ok
    reject = fail
  })
  return { promise, resolve, reject }
}

const profile: AccountProfile = {
  accountId: 'primary',
  label: 'Main',
  baseUrl: 'https://api.easicoin.io',
  credentialState: 'present',
  active: true,
}

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
  accounts: ['primary'],
  riskEnabled: true,
  riskMaxOrderQty: '10',
  riskMaxPriceDeviationPct: '5',
  riskMaxDailyOrders: 100,
  tradingDayTimezone: 'Asia/Shanghai',
}

describe('account profile store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  it('refreshes sanitized profiles without retaining secret-shaped fields', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce([
      { ...profile, apiKey: 'mock-key', apiSecret: 'mock-secret' },
    ])
    const store = useAccountProfilesStore()
    await store.refreshProfiles()
    expect(tauriInvoke).toHaveBeenCalledWith('list_account_profiles')
    expect(JSON.stringify(store.profiles)).not.toContain('mock-key')
    expect(JSON.stringify(store.profiles)).not.toContain('mock-secret')
  })

  it('adopts the authoritative config returned after a stale settings save', async () => {
    const authoritative = {
      ...appConfig,
      activeAccountId: 'backup',
      accounts: ['primary', 'backup'],
      windowWidth: 1440,
    }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(authoritative)
    const store = useConfigStore()

    await store.saveConfig({ ...appConfig, windowWidth: 1440 })

    expect(store.config).toEqual(authoritative)
  })

  it('tracks save independently and refreshes only after success', async () => {
    const pending = deferred<void>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'save_credentials') return pending.promise
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    const saving = store.saveCredentials({
      accountId: 'primary',
      apiKey: '',
      apiSecret: '',
      label: 'Main',
      baseUrl: 'https://api.easicoin.io',
    })
    expect(store.saving).toBe(true)
    expect(tauriInvoke).not.toHaveBeenCalledWith('list_account_profiles')
    pending.resolve()
    await saving
    expect(store.saving).toBe(false)
    expect(store.saveError).toBeNull()
    expect(tauriInvoke).toHaveBeenCalledWith('list_account_profiles')
  })

  it('clears account-bound state only after a successful switch resolves', async () => {
    const account = useAccountStore()
    const orders = useOrderStore()
    const positions = usePositionStore()
    account.applySnapshot({ accountId: 'primary', balances: [], totalEquity: '10' })
    account.applyDailyPnlSnapshot({
      value: '4', serverTime: 1, updatedAt: 1, recordCount: 1,
      dayStart: 0, dayEnd: 2, timezone: 'Asia/Shanghai',
    })
    orders.setOpenOrders([{ orderId: 'o1' } as never])
    orders.setOrderHistory([{ orderId: 'o2' } as never])
    positions.setPositions([{ symbol: 'BTCUSDT', size: '1' } as never])
    applyPrivatePanelsSnapshot({
      openOrders: orders.openOrders,
      orderHistory: orders.orderHistory,
      positions: positions.positions,
    })
    const pending = deferred<AccountSwitchResult>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') return pending.promise
      if (command === 'get_config') return Promise.resolve(null)
      if (command === 'list_account_profiles') return Promise.resolve([profile])
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    const switching = store.switchAccount('backup')
    expect(store.switching).toBe(true)
    expect(account.summary?.accountId).toBe('primary')
    expect(orders.openOrders).toHaveLength(1)
    pending.resolve({ activeAccountId: 'backup', connected: true })
    await switching
    expect(account.summary).toBeNull()
    expect(account.balances).toEqual([])
    expect(account.dailyPnl.data).toBeNull()
    expect(orders.openOrders).toEqual([])
    expect(orders.orderHistory).toEqual([])
    expect(positions.positions).toEqual([])
    expect(usePrivatePanelsState().state.value.data).toBeNull()
    expect(store.switching).toBe(false)
  })

  it('retains all former private state when switching fails', async () => {
    const account = useAccountStore()
    const orders = useOrderStore()
    const positions = usePositionStore()
    account.applySnapshot({ accountId: 'primary', balances: [], totalEquity: '10' })
    orders.setOpenOrders([{ orderId: 'o1' } as never])
    positions.setPositions([{ symbol: 'BTCUSDT', size: '1' } as never])
    applyPrivatePanelsSnapshot({ openOrders: [], orderHistory: [], positions: [] })
    vi.mocked(tauriInvoke).mockRejectedValueOnce('switch failed')
    const store = useAccountProfilesStore()
    await expect(store.switchAccount('backup')).rejects.toBe('switch failed')
    expect(account.summary?.accountId).toBe('primary')
    expect(orders.openOrders).toHaveLength(1)
    expect(positions.positions).toHaveLength(1)
    expect(usePrivatePanelsState().state.value.data).not.toBeNull()
    expect(store.switchError).toBe('switch failed')
  })

  it('refreshes delete only after success and keeps the row after failure', async () => {
    const store = useAccountProfilesStore()
    vi.mocked(tauriInvoke).mockResolvedValueOnce([profile])
    await store.refreshProfiles()
    vi.mocked(tauriInvoke).mockRejectedValueOnce('delete failed')
    await expect(store.deleteAccount('primary')).rejects.toBe('delete failed')
    expect(store.profiles).toEqual([profile])
    expect(tauriInvoke).toHaveBeenCalledTimes(2)

    vi.mocked(tauriInvoke)
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce([])
    await store.deleteAccount('primary')
    expect(store.profiles).toEqual([])
  })
})

describe('AccountProfilesPanel', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  it('never renders secrets and requires delete confirmation', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') {
        return Promise.resolve([
          { ...profile, accountId: 'backup', active: false, apiKey: 'mock-key', apiSecret: 'mock-secret' },
        ])
      }
      return Promise.resolve(undefined)
    })
    const wrapper = mount(AccountProfilesPanel, {
      global: {
        plugins: [createPinia()],
        stubs: {
          CredentialEditor: true,
          AppDialog: {
            props: ['show'],
            template: '<section v-if="show" data-testid="dialog"><slot/><slot name="footer"/></section>',
          },
        },
      },
    })
    await vi.waitFor(() => expect(wrapper.text()).toContain('backup'))
    expect(wrapper.text()).not.toContain('mock-key')
    expect(wrapper.text()).not.toContain('mock-secret')
    await wrapper.get('[data-testid="delete-backup"]').trigger('click')
    expect(wrapper.text()).toContain('backup')
    expect(tauriInvoke).not.toHaveBeenCalledWith('delete_account', { accountId: 'backup' })
    await wrapper.get('[data-testid="confirm-delete"]').trigger('click')
    expect(tauriInvoke).toHaveBeenCalledWith('delete_account', { accountId: 'backup' })
  })
})
