import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useAsyncState } from '../../src/composables/useAsyncState'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { clearAccountBoundState } from '../../src/services/accountSessionService'
import { useAccountStore } from '../../src/stores/account'
import { useOrderStore } from '../../src/stores/order'
import { usePositionStore } from '../../src/stores/position'
import { refreshPrivatePanels, usePrivatePanelsState } from '../../src/stores/privatePanels'
import type { FundingBalance, Order, Position } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((done, fail) => {
    resolve = done
    reject = fail
  })
  return { promise, resolve, reject }
}

const oldPosition: Position = {
  symbol: 'BTCUSDT', side: 'Buy', size: '1', entryPrice: '10', leverage: '1',
  unrealisedPnl: '2', positionIdx: 1,
}
const newPosition: Position = { ...oldPosition, symbol: 'ETHUSDT' }
const oldOrder: Order = {
  orderId: 'old', symbol: 'BTCUSDT', side: 'Buy', orderType: 'Limit', price: '10',
  qty: '1', status: 'New', filledQty: '0', avgPrice: '0',
}
const newOrder: Order = { ...oldOrder, orderId: 'new', symbol: 'ETHUSDT' }
const oldFunding: FundingBalance = { asset: 'USDT', available: '1', frozen: '0', total: '1' }
const newFunding: FundingBalance = { asset: 'USDC', available: '2', frozen: '0', total: '2' }

describe('account session request isolation', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  it('keeps async state idle when a pre-clear request later resolves or rejects', async () => {
    const success = deferred<string>()
    const state = useAsyncState<string>()
    const staleSuccess = state.run(() => success.promise)
    state.reset()
    success.resolve('old account')
    await staleSuccess
    expect(state.state.value).toMatchObject({ status: 'idle', data: null, error: null })

    const failure = deferred<string>()
    const staleFailure = state.run(() => failure.promise)
    state.reset()
    failure.reject(new Error('old account failed'))
    await expect(staleFailure).rejects.toThrow('old account failed')
    expect(state.state.value).toMatchObject({ status: 'idle', data: null, error: null })
  })

  it('does not let an older async request overwrite the newest result', async () => {
    const oldRequest = deferred<string>()
    const newRequest = deferred<string>()
    const state = useAsyncState<string>()
    const oldRun = state.run(() => oldRequest.promise)
    const newRun = state.run(() => newRequest.promise)

    newRequest.resolve('new account')
    await newRun
    oldRequest.resolve('old account')
    await oldRun

    expect(state.state.value.data).toBe('new account')
    expect(state.status.value).toBe('success')
  })

  it('does not restore cleared positions, orders, funding data, or request state', async () => {
    const positions = deferred<Position[]>()
    const orders = deferred<Order[]>()
    const funding = deferred<FundingBalance[]>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'refresh_positions') return positions.promise
      if (command === 'refresh_orders') return orders.promise
      if (command === 'fetch_funding_balances') return funding.promise
      return Promise.resolve(undefined)
    })
    const positionStore = usePositionStore()
    const orderStore = useOrderStore()
    const accountStore = useAccountStore()
    const pending = [
      positionStore.refreshPositions(),
      orderStore.refreshOrders(),
      accountStore.refreshFundingBalances(),
    ]

    clearAccountBoundState()
    positions.resolve([oldPosition])
    orders.resolve([oldOrder])
    funding.resolve([oldFunding])
    await Promise.all(pending)

    expect(positionStore.positions).toEqual([])
    expect(positionStore.status).toBe('idle')
    expect(orderStore.openOrders).toEqual([])
    expect(orderStore.openOrdersStatus).toBe('idle')
    expect(accountStore.fundingBalances).toEqual([])
    expect(accountStore.fundingStatus).toBe('idle')
  })

  it('does not restore stale errors after clearing account-bound state', async () => {
    const positions = deferred<Position[]>()
    const orders = deferred<Order[]>()
    const funding = deferred<FundingBalance[]>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'refresh_positions') return positions.promise
      if (command === 'refresh_orders') return orders.promise
      if (command === 'fetch_funding_balances') return funding.promise
      return Promise.resolve(undefined)
    })
    const positionStore = usePositionStore()
    const orderStore = useOrderStore()
    const accountStore = useAccountStore()
    const pending = [
      positionStore.refreshPositions(),
      orderStore.refreshOrders(),
      accountStore.refreshFundingBalances(),
    ]
    clearAccountBoundState()
    positions.reject(new Error('old positions failed'))
    orders.reject(new Error('old orders failed'))
    funding.reject(new Error('old funding failed'))
    const results = await Promise.allSettled(pending)

    expect(results.every((result) => result.status === 'rejected')).toBe(true)
    expect(positionStore.status).toBe('idle')
    expect(positionStore.error).toBeNull()
    expect(orderStore.openOrdersStatus).toBe('idle')
    expect(orderStore.openOrdersError).toBeNull()
    expect(accountStore.fundingStatus).toBe('idle')
    expect(accountStore.fundingError).toBeNull()
  })

  it('keeps private panel request state idle when its pre-clear task finishes late', async () => {
    const scheduler = deferred<void>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'scheduler_run_task') return scheduler.promise
      return Promise.resolve(undefined)
    })
    const pending = refreshPrivatePanels()

    clearAccountBoundState()
    scheduler.resolve()
    await pending

    expect(usePrivatePanelsState().status.value).toBe('idle')
    expect(usePrivatePanelsState().state.value.data).toBeNull()
  })

  it('keeps the newest position, order, and funding responses', async () => {
    const oldPositionRequest = deferred<Position[]>()
    const newPositionRequest = deferred<Position[]>()
    const oldOrderRequest = deferred<Order[]>()
    const newOrderRequest = deferred<Order[]>()
    const oldFundingRequest = deferred<FundingBalance[]>()
    const newFundingRequest = deferred<FundingBalance[]>()
    const responses = new Map<string, Array<Promise<unknown>>>([
      ['refresh_positions', [oldPositionRequest.promise, newPositionRequest.promise]],
      ['refresh_orders', [oldOrderRequest.promise, newOrderRequest.promise]],
      ['fetch_funding_balances', [oldFundingRequest.promise, newFundingRequest.promise]],
    ])
    vi.mocked(tauriInvoke).mockImplementation((command) =>
      responses.get(command)!.shift() as Promise<never>,
    )
    const positionStore = usePositionStore()
    const orderStore = useOrderStore()
    const accountStore = useAccountStore()
    const oldRuns = [
      positionStore.refreshPositions(), orderStore.refreshOrders(), accountStore.refreshFundingBalances(),
    ]
    const newRuns = [
      positionStore.refreshPositions(), orderStore.refreshOrders(), accountStore.refreshFundingBalances(),
    ]

    newPositionRequest.resolve([newPosition])
    newOrderRequest.resolve([newOrder])
    newFundingRequest.resolve([newFunding])
    await Promise.all(newRuns)
    oldPositionRequest.resolve([oldPosition])
    oldOrderRequest.resolve([oldOrder])
    oldFundingRequest.resolve([oldFunding])
    await Promise.all(oldRuns)

    expect(positionStore.positions.map((position) => position.symbol)).toEqual(['ETHUSDT'])
    expect(orderStore.openOrders.map((order) => order.orderId)).toEqual(['new'])
    expect(accountStore.fundingBalances.map((balance) => balance.asset)).toEqual(['USDC'])
  })
})
