import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import AccountAssetsPanel from '../../src/components/account/AccountAssetsPanel.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useAccountStore } from '../../src/stores/account'
import { useConnectionStore } from '../../src/stores/connection'
import { usePositionStore } from '../../src/stores/position'
import type {
  AccountSummary,
  DailyPnlSnapshot,
  FundingBalance,
  Position,
} from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

const funding: FundingBalance = {
  asset: 'USDT',
  available: '8',
  frozen: '2',
  total: '10',
}

const account: AccountSummary = {
  accountId: 'primary',
  balances: [{ asset: 'USDT', available: '80', frozen: '20', total: '100' }],
  totalEquity: '100',
}

const dailyPnl: DailyPnlSnapshot = {
  value: '3.5',
  serverTime: 1_784_692_800_000,
  updatedAt: 1_784_692_800_000,
  recordCount: 2,
  dayStart: 1_784_649_600_000,
  dayEnd: 1_784_736_000_000,
  timezone: 'Asia/Shanghai',
}

const position: Position = {
  symbol: 'BTCUSDT',
  side: 'Buy',
  size: '0.1',
  entryPrice: '60000',
  leverage: '10',
  unrealisedPnl: '1',
  positionIdx: 1,
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

async function flushMicrotasks(): Promise<void> {
  for (let index = 0; index < 8; index += 1) {
    await Promise.resolve()
  }
}

describe('account asset state', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  it('refreshes normalized funding balances', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce([funding])
    const store = useAccountStore()

    await store.refreshFundingBalances()

    expect(tauriInvoke).toHaveBeenCalledWith('fetch_funding_balances')
    expect(store.fundingBalances).toEqual([funding])
    expect(store.fundingError).toBeNull()
  })

  it('keeps contract data when the funding request fails', async () => {
    const store = useAccountStore()
    store.applySnapshot({
      accountId: 'primary',
      balances: [{ asset: 'USDT', available: '80', frozen: '20', total: '100' }],
      totalEquity: '100',
    })
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('funding unavailable'))

    await expect(store.refreshFundingBalances()).rejects.toThrow('funding unavailable')

    expect(store.summary?.totalEquity).toBe('100')
    expect(store.balances).toHaveLength(1)
    expect(store.fundingError).toBe('funding unavailable')
  })

  it('clears funding balances with the rest of account-bound data', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce([funding])
    const store = useAccountStore()
    await store.refreshFundingBalances()

    store.clearAccountData()

    expect(store.fundingBalances).toEqual([])
    expect(store.fundingStatus).toBe('idle')
  })

  it('lets successful snapshots clear request errors and publish updated timestamps', async () => {
    const accountStore = useAccountStore()
    const positionStore = usePositionStore()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'scheduler_run_task') return Promise.reject(new Error(`${String(command)} failed`))
      if (command === 'refresh_positions') return Promise.reject(new Error('positions failed'))
      if (command === 'fetch_funding_balances') return Promise.reject(new Error('funding failed'))
      return Promise.resolve(undefined)
    })

    await expect(accountStore.refreshAccount(true)).rejects.toThrow('scheduler_run_task failed')
    await expect(accountStore.refreshDailyPnl(true)).rejects.toThrow('scheduler_run_task failed')
    await expect(positionStore.refreshPositions()).rejects.toThrow('positions failed')
    await expect(accountStore.refreshFundingBalances()).rejects.toThrow('funding failed')

    expect(accountStore.status).toBe('error')
    expect(accountStore.dailyPnlStatus).toBe('error')
    expect(positionStore.status).toBe('error')
    expect(accountStore.fundingStatus).toBe('error')

    accountStore.applySnapshot(account)
    accountStore.applyDailyPnlSnapshot(dailyPnl)
    positionStore.setPositions([position])
    vi.mocked(tauriInvoke).mockResolvedValueOnce([funding])
    await accountStore.refreshFundingBalances()

    expect(accountStore.error).toBeNull()
    expect(accountStore.status).toBe('success')
    expect(accountStore.updatedAt).not.toBeNull()
    expect(accountStore.dailyPnlError).toBeNull()
    expect(accountStore.dailyPnlStatus).toBe('success')
    expect(accountStore.dailyPnlUpdatedAt).not.toBeNull()
    expect(positionStore.error).toBeNull()
    expect(positionStore.status).toBe('success')
    expect(positionStore.updatedAt).not.toBeNull()
    expect(accountStore.fundingError).toBeNull()
    expect(accountStore.fundingStatus).toBe('success')
    expect(accountStore.fundingUpdatedAt).not.toBeNull()
  })

  it('keeps initial refreshes loading until their delayed snapshots arrive', async () => {
    const accountGate = deferred<void>()
    const dailyGate = deferred<void>()
    vi.mocked(tauriInvoke).mockImplementation((command, args) => {
      if (command === 'scheduler_run_task' && args?.task === 'account') return accountGate.promise
      if (command === 'scheduler_run_task' && args?.task === 'dailyPnl') return dailyGate.promise
      return Promise.resolve(undefined)
    })
    const store = useAccountStore()

    const accountOutcome = store.refreshAccount(true).then(() => 'fulfilled', () => 'rejected')
    const dailyOutcome = store.refreshDailyPnl(true).then(() => 'fulfilled', () => 'rejected')

    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledTimes(2))
    accountGate.resolve()
    dailyGate.resolve()
    await flushMicrotasks()
    expect(store.status).toBe('loading')
    expect(store.dailyPnlStatus).toBe('loading')

    store.applySnapshot(account)
    store.applyDailyPnlSnapshot(dailyPnl)

    await expect(accountOutcome).resolves.toBe('fulfilled')
    await expect(dailyOutcome).resolves.toBe('fulfilled')
    expect(store.summary).toEqual(account)
    expect(store.error).toBeNull()
    expect(store.status).toBe('success')
    expect(store.dailyPnl.data).toEqual(dailyPnl)
    expect(store.dailyPnlError).toBeNull()
    expect(store.dailyPnlStatus).toBe('success')
  })

  it('does not mark seeded old snapshots fresh before delayed events arrive', async () => {
    const now = vi.spyOn(Date, 'now')
      .mockReturnValueOnce(100)
      .mockReturnValueOnce(200)
      .mockReturnValueOnce(300)
      .mockReturnValueOnce(400)
    const store = useAccountStore()
    const oldAccount = { ...account, totalEquity: '90' }
    const oldDaily = { ...dailyPnl, value: '1' }
    store.applySnapshot(oldAccount)
    store.applyDailyPnlSnapshot(oldDaily)
    const oldAccountUpdatedAt = store.updatedAt
    const oldDailyUpdatedAt = store.dailyPnlUpdatedAt
    const accountGate = deferred<void>()
    const dailyGate = deferred<void>()
    vi.mocked(tauriInvoke).mockImplementation((command, args) => {
      if (command === 'scheduler_run_task' && args?.task === 'account') return accountGate.promise
      if (command === 'scheduler_run_task' && args?.task === 'dailyPnl') return dailyGate.promise
      return Promise.resolve(undefined)
    })

    const accountOutcome = store.refreshAccount(true).then(() => 'fulfilled', () => 'rejected')
    const dailyOutcome = store.refreshDailyPnl(true).then(() => 'fulfilled', () => 'rejected')

    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledTimes(2))
    accountGate.resolve()
    dailyGate.resolve()
    await flushMicrotasks()
    expect(store.summary).toEqual(oldAccount)
    expect(store.dailyPnl.data).toEqual(oldDaily)
    expect(store.status).toBe('loading')
    expect(store.dailyPnlStatus).toBe('loading')
    expect(store.updatedAt).toBe(oldAccountUpdatedAt)
    expect(store.dailyPnlUpdatedAt).toBe(oldDailyUpdatedAt)

    store.applySnapshot(account)
    store.applyDailyPnlSnapshot(dailyPnl)

    await expect(accountOutcome).resolves.toBe('fulfilled')
    await expect(dailyOutcome).resolves.toBe('fulfilled')
    expect(store.summary).toEqual(account)
    expect(store.dailyPnl.data).toEqual(dailyPnl)
    expect(store.status).toBe('success')
    expect(store.dailyPnlStatus).toBe('success')
    expect(store.updatedAt).toBe(300)
    expect(store.dailyPnlUpdatedAt).toBe(400)
  })

  it('wakes pending snapshot refreshes on clear without restoring cleared state', async () => {
    const accountGate = deferred<void>()
    const dailyGate = deferred<void>()
    vi.mocked(tauriInvoke).mockImplementation((command, args) => {
      if (command === 'scheduler_run_task' && args?.task === 'account') return accountGate.promise
      if (command === 'scheduler_run_task' && args?.task === 'dailyPnl') return dailyGate.promise
      return Promise.resolve(undefined)
    })
    const store = useAccountStore()
    const accountOutcome = store.refreshAccount(true).then(() => 'fulfilled', () => 'rejected')
    const dailyOutcome = store.refreshDailyPnl(true).then(() => 'fulfilled', () => 'rejected')

    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledTimes(2))
    accountGate.resolve()
    dailyGate.resolve()
    await flushMicrotasks()
    expect(store.status).toBe('loading')
    expect(store.dailyPnlStatus).toBe('loading')
    store.clearAccountData()

    await expect(accountOutcome).resolves.toBe('rejected')
    await expect(dailyOutcome).resolves.toBe('rejected')
    expect(store.summary).toBeNull()
    expect(store.dailyPnl.data).toBeNull()
    expect(store.status).toBe('idle')
    expect(store.dailyPnlStatus).toBe('idle')
  })

  it('bounds event waits, shares concurrent refreshes, and accepts late authoritative events', async () => {
    vi.useFakeTimers()
    vi.mocked(tauriInvoke).mockResolvedValue(undefined)
    const store = useAccountStore()

    const swallowedAccount = store.refreshAccount(false)
    const thrownAccount = store.refreshAccount(true).then(
      () => null,
      (error: unknown) => error,
    )
    const thrownDaily = store.refreshDailyPnl(true).then(
      () => null,
      (error: unknown) => error,
    )
    await flushMicrotasks()

    expect(tauriInvoke).toHaveBeenCalledTimes(2)
    expect(tauriInvoke).toHaveBeenCalledWith('scheduler_run_task', { task: 'account', force: true })
    expect(tauriInvoke).toHaveBeenCalledWith('scheduler_run_task', { task: 'dailyPnl', force: true })
    expect(store.status).toBe('loading')
    expect(store.dailyPnlStatus).toBe('loading')
    expect(vi.getTimerCount()).toBe(2)

    await vi.runAllTimersAsync()

    await expect(swallowedAccount).resolves.toBeUndefined()
    await expect(thrownAccount).resolves.toEqual(new Error('账户快照等待超时'))
    await expect(thrownDaily).resolves.toEqual(new Error('每日盈亏快照等待超时'))
    expect(store.status).toBe('error')
    expect(store.dailyPnlStatus).toBe('error')
    expect(vi.getTimerCount()).toBe(0)

    store.applySnapshot(account)
    store.applyDailyPnlSnapshot(dailyPnl)

    expect(store.summary).toEqual(account)
    expect(store.dailyPnl.data).toEqual(dailyPnl)
    expect(store.status).toBe('success')
    expect(store.dailyPnlStatus).toBe('success')
  })

  it('disconnects cleared operations without letting old cleanup replace a new refresh', async () => {
    vi.useFakeTimers()
    vi.mocked(tauriInvoke).mockResolvedValue(undefined)
    const store = useAccountStore()
    const oldOutcome = store.refreshAccount(true).then(() => 'fulfilled', () => 'rejected')
    await flushMicrotasks()
    expect(tauriInvoke).toHaveBeenCalledTimes(1)
    expect(vi.getTimerCount()).toBe(1)

    store.clearAccountData()
    expect(vi.getTimerCount()).toBe(0)
    const newOutcome = store.refreshAccount(true).then(() => 'fulfilled', () => 'rejected')
    const reusedOutcome = store.refreshAccount(true).then(() => 'fulfilled', () => 'rejected')
    await flushMicrotasks()

    expect(tauriInvoke).toHaveBeenCalledTimes(2)
    expect(store.status).toBe('loading')
    expect(vi.getTimerCount()).toBe(1)
    store.applySnapshot(account)

    await expect(oldOutcome).resolves.toBe('rejected')
    await expect(newOutcome).resolves.toBe('fulfilled')
    await expect(reusedOutcome).resolves.toBe('fulfilled')
    expect(store.summary).toEqual(account)
    expect(store.status).toBe('success')
    expect(vi.getTimerCount()).toBe(0)
  })

  it('treats normalized zero positions as empty and lets incremental events clear errors', async () => {
    const accountStore = useAccountStore()
    const positionStore = usePositionStore()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'scheduler_run_task') return Promise.reject(new Error('account failed'))
      if (command === 'refresh_positions') return Promise.reject(new Error('positions failed'))
      return Promise.resolve(undefined)
    })

    await expect(accountStore.refreshAccount(true)).rejects.toThrow('account failed')
    await expect(positionStore.refreshPositions()).rejects.toThrow('positions failed')
    accountStore.setBalance(account.balances[0])
    positionStore.upsertPosition(position)

    expect(accountStore.status).toBe('success')
    expect(accountStore.error).toBeNull()
    expect(positionStore.status).toBe('success')
    expect(positionStore.error).toBeNull()

    positionStore.setPositions([{ ...position, size: '0' }])

    expect(positionStore.positions).toEqual([])
    expect(positionStore.status).toBe('empty')

    accountStore.applyDailyPnlSnapshot({ ...dailyPnl, value: '0', recordCount: 0 })
    expect(accountStore.dailyPnlStatus).toBe('empty')
  })

  it('keeps later account, daily PnL, and position events authoritative over old requests', async () => {
    const accountGate = deferred<void>()
    const dailyGate = deferred<void>()
    const positionGate = deferred<Position[]>()
    vi.mocked(tauriInvoke).mockImplementation((command, args) => {
      if (command === 'scheduler_run_task' && args?.task === 'account') return accountGate.promise
      if (command === 'scheduler_run_task' && args?.task === 'dailyPnl') return dailyGate.promise
      if (command === 'refresh_positions') return positionGate.promise
      return Promise.resolve(undefined)
    })
    const accountStore = useAccountStore()
    const positionStore = usePositionStore()

    const oldAccount = accountStore.refreshAccount(true)
    const oldDaily = accountStore.refreshDailyPnl(true)
    const oldPositions = positionStore.refreshPositions()
    expect(accountStore.status).toBe('loading')
    expect(accountStore.dailyPnlStatus).toBe('loading')
    expect(positionStore.status).toBe('loading')

    accountStore.applySnapshot(account)
    accountStore.applyDailyPnlSnapshot(dailyPnl)
    positionStore.setPositions([position])
    accountGate.reject(new Error('old account failed'))
    dailyGate.reject(new Error('old daily PnL failed'))
    positionGate.resolve([{ ...position, symbol: 'ETHUSDT' }])
    const results = await Promise.allSettled([oldAccount, oldDaily, oldPositions])

    expect(results.map((result) => result.status)).toEqual(['rejected', 'rejected', 'fulfilled'])
    expect(accountStore.summary).toEqual(account)
    expect(accountStore.status).toBe('success')
    expect(accountStore.error).toBeNull()
    expect(accountStore.dailyPnl.data).toEqual(dailyPnl)
    expect(accountStore.dailyPnlStatus).toBe('success')
    expect(accountStore.dailyPnlError).toBeNull()
    expect(positionStore.positions).toEqual([position])
    expect(positionStore.status).toBe('success')
    expect(positionStore.error).toBeNull()
  })
})

describe('AccountAssetsPanel', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  it('does not invoke private commands while disconnected', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const wrapper = mount(AccountAssetsPanel, { global: { plugins: [pinia] } })

    await wrapper.vm.$nextTick()

    expect(wrapper.text()).toContain('连接状态：已断开')
    expect(wrapper.text()).toContain('连接账户后刷新')
    expect(tauriInvoke).not.toHaveBeenCalled()
  })

  it('refreshes every section independently when connected', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    useConnectionStore().setStatus('connected')
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'refresh_positions') return Promise.resolve([])
      if (command === 'fetch_funding_balances') return Promise.reject(new Error('funding unavailable'))
      return Promise.resolve(undefined)
    })

    mount(AccountAssetsPanel, { global: { plugins: [pinia] } })

    await vi.waitFor(() => {
      expect(tauriInvoke).toHaveBeenCalledWith('scheduler_run_task', { task: 'account', force: true })
      expect(tauriInvoke).toHaveBeenCalledWith('refresh_positions', { symbol: null })
      expect(tauriInvoke).toHaveBeenCalledWith('scheduler_run_task', { task: 'dailyPnl', force: true })
      expect(tauriInvoke).toHaveBeenCalledWith('fetch_funding_balances')
    })
    expect(useAccountStore().fundingError).toBe('funding unavailable')
  })

  it('keeps scheduler failures isolated from successful positions and funding', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    useConnectionStore().setStatus('connected')
    vi.mocked(tauriInvoke).mockImplementation((command, args) => {
      if (command === 'scheduler_run_task' && args?.task === 'account') {
        return Promise.reject(new Error('account unavailable'))
      }
      if (command === 'scheduler_run_task' && args?.task === 'dailyPnl') {
        return Promise.reject(new Error('daily PnL unavailable'))
      }
      if (command === 'refresh_positions') {
        return Promise.resolve([{
          symbol: 'BTCUSDT', side: 'Buy', size: '0.1', entryPrice: '60000',
          leverage: '10', unrealisedPnl: '1', positionIdx: 1,
        }])
      }
      if (command === 'fetch_funding_balances') return Promise.resolve([funding])
      return Promise.resolve(undefined)
    })

    const wrapper = mount(AccountAssetsPanel, { global: { plugins: [pinia] } })

    await vi.waitFor(() => {
      expect(wrapper.text()).toContain('account unavailable')
      expect(wrapper.text()).toContain('daily PnL unavailable')
      expect(wrapper.text()).toContain('BTCUSDT')
      expect(wrapper.text()).toContain('买入')
      expect(wrapper.text()).not.toContain(' Buy ')
      expect(wrapper.text()).toContain('USDT')
    })
    expect(wrapper.get('[data-testid="asset-contract"]').attributes('data-state')).toBe('error')
    expect(wrapper.get('[data-testid="asset-positions"]').attributes('data-state')).toBe('success')
    expect(wrapper.get('[data-testid="asset-daily-pnl"]').attributes('data-state')).toBe('error')
    expect(wrapper.get('[data-testid="asset-funding"]').attributes('data-state')).toBe('success')
  })

  it('publishes completed sections while funding remains permanently pending', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const accountStore = useAccountStore()
    const fundingGate = deferred<FundingBalance[]>()
    useConnectionStore().setStatus('connected')
    vi.mocked(tauriInvoke).mockImplementation((command, args) => {
      if (command === 'scheduler_run_task' && args?.task === 'account') {
        accountStore.applySnapshot(account)
        return Promise.resolve(undefined)
      }
      if (command === 'scheduler_run_task' && args?.task === 'dailyPnl') {
        accountStore.applyDailyPnlSnapshot(dailyPnl)
        return Promise.resolve(undefined)
      }
      if (command === 'refresh_positions') return Promise.resolve([])
      if (command === 'fetch_funding_balances') return fundingGate.promise
      return Promise.resolve(undefined)
    })

    const wrapper = mount(AccountAssetsPanel, { global: { plugins: [pinia] } })

    await vi.waitFor(() => {
      expect(wrapper.get('[data-testid="asset-contract"]').attributes('data-state')).toBe('success')
      expect(wrapper.get('[data-testid="asset-positions"]').attributes('data-state')).toBe('empty')
      expect(wrapper.get('[data-testid="asset-daily-pnl"]').attributes('data-state')).toBe('success')
      expect(wrapper.get('[data-testid="asset-funding"]').attributes('data-state')).toBe('loading')
    })
    expect(wrapper.get('[data-testid="asset-contract"]').text()).toContain('100')
    expect(wrapper.get('[data-testid="asset-positions"]').text()).toContain('暂无')
    expect(wrapper.get('[data-testid="asset-daily-pnl"]').text()).toContain('3.5')
    expect(wrapper.get('[data-testid="asset-funding"]').text()).toContain('加载中')
    expect(accountStore.updatedAt).not.toBeNull()
    expect(accountStore.dailyPnlUpdatedAt).not.toBeNull()
    expect(accountStore.fundingUpdatedAt).toBeNull()
  })

  it('renders store-owned section errors without waiting for other sections', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const fundingGate = deferred<FundingBalance[]>()
    useConnectionStore().setStatus('connected')
    vi.mocked(tauriInvoke).mockImplementation((command, args) => {
      if (command === 'scheduler_run_task' && args?.task === 'account') {
        return Promise.reject(new Error('account unavailable'))
      }
      if (command === 'scheduler_run_task' && args?.task === 'dailyPnl') {
        return Promise.reject(new Error('daily PnL unavailable'))
      }
      if (command === 'refresh_positions') return Promise.reject(new Error('positions unavailable'))
      if (command === 'fetch_funding_balances') return fundingGate.promise
      return Promise.resolve(undefined)
    })

    const wrapper = mount(AccountAssetsPanel, { global: { plugins: [pinia] } })

    await vi.waitFor(() => {
      expect(wrapper.get('[data-testid="asset-contract"]').attributes('data-state')).toBe('error')
      expect(wrapper.get('[data-testid="asset-positions"]').attributes('data-state')).toBe('error')
      expect(wrapper.get('[data-testid="asset-daily-pnl"]').attributes('data-state')).toBe('error')
      expect(wrapper.get('[data-testid="asset-funding"]').attributes('data-state')).toBe('loading')
    })
    expect(wrapper.text()).toContain('account unavailable')
    expect(wrapper.text()).toContain('positions unavailable')
    expect(wrapper.text()).toContain('daily PnL unavailable')
  })
})
