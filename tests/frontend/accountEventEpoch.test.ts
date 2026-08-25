import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { decideAccountSessionEpoch } from '../../src/services/accountSessionEpochService'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useAccountStore } from '../../src/stores/account'
import { useConfigStore } from '../../src/stores/config'
import { useConnectionStore } from '../../src/stores/connection'
import { useOrderStore } from '../../src/stores/order'
import type { AccountSessionEvent, AccountSwitchResult, AppConfig } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

const config: AppConfig = {
  activeSymbol: 'BTCUSDT', activeAccountId: 'primary', watchlistSymbols: ['BTCUSDT'],
  theme: 'dark', klineInterval: '15', useWebsocket: false,
  wsPublicUrl: '', wsPrivateUrl: '', tickerPollInterval: 1000,
  windowWidth: 1200, windowHeight: 800, accounts: ['primary', 'backup'],
  riskEnabled: true, riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5',
  riskMaxDailyOrders: 100, tradingDayTimezone: 'Asia/Shanghai',
}

describe('account session epoch decisions', () => {
  it('rejects lower epochs, accepts equal epochs, and advances for higher epochs', () => {
    expect(decideAccountSessionEpoch(3, 2)).toBe('reject')
    expect(decideAccountSessionEpoch(3, 3)).toBe('accept')
    expect(decideAccountSessionEpoch(3, 4)).toBe('advance')
  })
})

describe('account-bound event routing', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    useConfigStore().config = config
    vi.mocked(tauriInvoke).mockReset()
  })

  it('does not call a listener for a stale lower-epoch event', () => {
    const profiles = useAccountProfilesStore()
    profiles.adoptSessionEpoch(2)
    const listener = vi.fn()

    const accepted = profiles.handleSessionEvent(
      { accountId: 'primary', sessionEpoch: 1, payload: 'stale' },
      listener,
    )

    expect(accepted).toBe(false)
    expect(listener).not.toHaveBeenCalled()
    expect(profiles.sessionEpoch).toBe(2)
  })

  it('advances and clears account-bound state before accepting a higher epoch', () => {
    const profiles = useAccountProfilesStore()
    const account = useAccountStore()
    const orders = useOrderStore()
    account.applySnapshot({ accountId: 'primary', balances: [], totalEquity: '10' })
    orders.setOpenOrders([{ orderId: 'old' } as never])
    const observedAtHandler: Array<{ summary: unknown; openOrders: number }> = []

    const accepted = profiles.handleSessionEvent(
      { accountId: 'primary', sessionEpoch: 1, payload: 'new-session' },
      () => {
        observedAtHandler.push({
          summary: account.summary,
          openOrders: orders.openOrders.length,
        })
      },
    )

    expect(accepted).toBe(true)
    expect(profiles.sessionEpoch).toBe(1)
    expect(observedAtHandler).toEqual([{
      summary: null,
      openOrders: 0,
    }])
  })

  it('fails closed on malformed account ownership before an epoch can advance', () => {
    useConfigStore().config = { ...config, activeAccountId: 'default' }
    const profiles = useAccountProfilesStore()
    const account = useAccountStore()
    const clear = vi.spyOn(account, 'clearAccountData')
    const handler = vi.fn()

    for (const accountId of [undefined, null, '', '   ', ' default ', 17]) {
      const accepted = profiles.handleSessionEvent(
        { accountId, sessionEpoch: 9, payload: 'malformed' } as never,
        handler,
      )
      expect(accepted).toBe(false)
    }

    expect(handler).not.toHaveBeenCalled()
    expect(clear).not.toHaveBeenCalled()
    expect(profiles.sessionEpoch).toBe(0)
  })

  it('rejects wrong-account equal and higher epochs before clearing or advancing', () => {
    const profiles = useAccountProfilesStore()
    const account = useAccountStore()
    const clear = vi.spyOn(account, 'clearAccountData')
    const handler = vi.fn()

    for (const sessionEpoch of [0, 4]) {
      expect(profiles.handleSessionEvent(
        { accountId: 'backup', sessionEpoch, payload: 'wrong-owner' },
        handler,
      )).toBe(false)
    }

    expect(handler).not.toHaveBeenCalled()
    expect(clear).not.toHaveBeenCalled()
    expect(profiles.sessionEpoch).toBe(0)
  })

  it('rejects malformed epochs before clearing or advancing', () => {
    const profiles = useAccountProfilesStore()
    const account = useAccountStore()
    const clear = vi.spyOn(account, 'clearAccountData')
    const handler = vi.fn()

    for (const sessionEpoch of [
      undefined, null, '1', Number.NaN, Number.POSITIVE_INFINITY, -1, 0.5,
      Number.MAX_SAFE_INTEGER + 1,
    ]) {
      expect(profiles.handleSessionEvent(
        { accountId: 'primary', sessionEpoch, payload: 'malformed-epoch' } as never,
        handler,
      )).toBe(false)
      expect(profiles.handleSessionEvent(
        { accountId: 'backup', sessionEpoch, payload: 'wrong-and-malformed' } as never,
        handler,
      )).toBe(false)
    }

    expect(handler).not.toHaveBeenCalled()
    expect(clear).not.toHaveBeenCalled()
    expect(profiles.sessionEpoch).toBe(0)
  })

  it('rejects a delayed old-A event after a rapid A to B to A cycle', () => {
    const profiles = useAccountProfilesStore()
    const received: string[] = []
    const route = (event: AccountSessionEvent<string>) => {
      profiles.handleSessionEvent(event, (payload) => received.push(payload))
    }

    route({ accountId: 'primary', sessionEpoch: 0, payload: 'A-before-switch' })
    route({ accountId: 'primary', sessionEpoch: 1, payload: 'B' })
    route({ accountId: 'primary', sessionEpoch: 2, payload: 'A-after-switch' })
    route({ accountId: 'primary', sessionEpoch: 0, payload: 'A-delayed-old-session' })

    expect(received).toEqual(['A-before-switch', 'B', 'A-after-switch'])
    expect(profiles.sessionEpoch).toBe(2)
  })

  it('rejects equal and higher epoch events until a successful switch commits once', async () => {
    const pendingSwitch = deferred<AccountSwitchResult>()
    const barrierWsStatus = deferred<'connecting'>()
    const postBarrierWsStatus = deferred<'connecting'>()
    let wsStatusCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') return pendingSwitch.promise
      if (command === 'get_websocket_status') {
        wsStatusCalls += 1
        return wsStatusCalls === 1 ? barrierWsStatus.promise : postBarrierWsStatus.promise
      }
      if (command === 'get_config') return Promise.resolve({ ...config, activeAccountId: 'backup' })
      if (command === 'list_account_profiles') return Promise.resolve([])
      if (command === 'get_connection_status') return Promise.resolve('disconnected')
      return Promise.resolve(undefined)
    })
    const profiles = useAccountProfilesStore()
    const account = useAccountStore()
    const clear = vi.spyOn(account, 'clearAccountData')
    account.applySnapshot({ accountId: 'primary', balances: [], totalEquity: '10' })
    const switching = profiles.switchAccount('backup')
    const listener = vi.fn((snapshot) => account.applySnapshot(snapshot))

    const equalAccepted = profiles.handleSessionEvent(
      {
        accountId: 'primary',
        sessionEpoch: 0,
        payload: { accountId: 'primary', balances: [], totalEquity: '11' },
      },
      listener,
    )
    const higherAccepted = profiles.handleSessionEvent(
      {
        accountId: 'backup',
        sessionEpoch: 1,
        payload: { accountId: 'backup', balances: [], totalEquity: '99' },
      },
      listener,
    )

    expect(equalAccepted).toBe(false)
    expect(higherAccepted).toBe(false)
    expect(listener).not.toHaveBeenCalled()
    expect(profiles.sessionEpoch).toBe(0)
    expect(account.summary?.totalEquity).toBe('10')
    expect(clear).not.toHaveBeenCalled()

    pendingSwitch.resolve({ activeAccountId: 'backup', connected: false, sessionEpoch: 1 })
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('get_websocket_status'))
    expect(profiles.handleSessionEvent(
      { accountId: 'backup', sessionEpoch: 1, payload: 'target-during-websocket-status-refresh' },
      listener,
    )).toBe(false)
    barrierWsStatus.resolve('connecting')
    await vi.waitFor(() => expect(wsStatusCalls).toBe(2))
    const connection = useConnectionStore()
    expect(profiles.handleSessionEvent(
      { accountId: 'backup', sessionEpoch: 1, payload: 'connected' },
      (status) => connection.setWsStatus(status),
    )).toBe(true)
    postBarrierWsStatus.resolve('connecting')
    await switching

    expect(profiles.sessionEpoch).toBe(1)
    expect(account.summary).toBeNull()
    expect(clear).toHaveBeenCalledTimes(1)
    expect(connection.wsStatus).toBe('connected')
    expect(profiles.handleSessionEvent(
      {
        accountId: 'backup',
        sessionEpoch: 1,
        payload: { accountId: 'backup', balances: [], totalEquity: '12' },
      },
      listener,
    )).toBe(true)
    expect(account.summary?.totalEquity).toBe('12')
  })

  it('keeps the former epoch and private state when a pending switch fails', async () => {
    const pendingSwitch = deferred<AccountSwitchResult>()
    const pendingStatus = deferred<'connected'>()
    const barrierWsStatus = deferred<'connecting'>()
    const postBarrierWsStatus = deferred<'connecting'>()
    let wsStatusCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') return pendingSwitch.promise
      if (command === 'get_connection_status') return pendingStatus.promise
      if (command === 'get_websocket_status') {
        wsStatusCalls += 1
        return wsStatusCalls === 1 ? barrierWsStatus.promise : postBarrierWsStatus.promise
      }
      return Promise.resolve(undefined)
    })
    const profiles = useAccountProfilesStore()
    const account = useAccountStore()
    const orders = useOrderStore()
    account.applySnapshot({ accountId: 'primary', balances: [], totalEquity: '10' })
    orders.setOpenOrders([{ orderId: 'old' } as never])
    const listener = vi.fn()
    const switching = profiles.switchAccount('backup')

    expect(profiles.handleSessionEvent(
      { accountId: 'backup', sessionEpoch: 1, payload: 'target-before-failure' },
      listener,
    )).toBe(false)
    pendingSwitch.reject(new Error('target failed and rolled back'))
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('get_connection_status'))
    expect(profiles.handleSessionEvent(
      { accountId: 'primary', sessionEpoch: 0, payload: 'former-during-status-refresh' },
      listener,
    )).toBe(false)
    pendingStatus.resolve('connected')
    barrierWsStatus.resolve('connecting')
    await vi.waitFor(() => expect(wsStatusCalls).toBe(2))
    const connection = useConnectionStore()
    expect(profiles.handleSessionEvent(
      { accountId: 'primary', sessionEpoch: 0, payload: 'connected' },
      (status) => connection.setWsStatus(status),
    )).toBe(true)
    postBarrierWsStatus.resolve('connecting')
    await expect(switching).rejects.toThrow('target failed and rolled back')

    expect(profiles.sessionEpoch).toBe(0)
    expect(account.summary?.accountId).toBe('primary')
    expect(orders.openOrders).toHaveLength(1)
    expect(connection.wsStatus).toBe('connected')
    expect(listener).not.toHaveBeenCalled()
    expect(profiles.handleSessionEvent(
      { accountId: 'primary', sessionEpoch: 0, payload: 'former-after-rollback' },
      listener,
    )).toBe(true)
    expect(listener).toHaveBeenCalledWith('former-after-rollback')
  })

  it('releases the transition barrier but freezes session events after local commit failure', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: false, sessionEpoch: 1 })
      }
      return Promise.resolve(undefined)
    })
    const profiles = useAccountProfilesStore()
    const account = useAccountStore()
    vi.spyOn(account, 'clearAccountData').mockImplementationOnce(() => {
      throw new Error('local clear failed')
    })

    await expect(profiles.switchAccount('backup')).rejects.toThrow('local clear failed')

    expect(profiles.switching).toBe(false)
    expect(profiles.recoveryRequired).toBe(true)
    const listener = vi.fn()
    expect(profiles.handleSessionEvent(
      { accountId: 'backup', sessionEpoch: 1, payload: 'after-local-failure' },
      listener,
    )).toBe(false)
    expect(listener).not.toHaveBeenCalled()
  })
})
