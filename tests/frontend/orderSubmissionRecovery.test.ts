import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, disposePinia, setActivePinia, type Pinia } from 'pinia'
import { effectScope, nextTick, type EffectScope } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import OrderPanel from '../../src/components/trading/OrderPanel.vue'
import { useOrderPanel } from '../../src/composables/useOrderPanel'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { refreshSyncTask } from '../../src/services/dataSyncService'
import { installMessageApi } from '../../src/services/errorService'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useConfigStore } from '../../src/stores/config'
import { useConnectionStore } from '../../src/stores/connection'
import { useOrderStore } from '../../src/stores/order'
import { usePositionStore } from '../../src/stores/position'
import type { Order, PlaceOrderRequest } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))
vi.mock('../../src/services/dataSyncService', () => ({ refreshSyncTask: vi.fn() }))

const recovered: Order = {
  orderId: 'exchange-original', orderLinkId: 'original-intent', symbol: 'BTCUSDT',
  side: 'Buy', orderType: 'Limit', qty: '0.1', price: '60000', status: 'Filled',
  filledQty: '0.1', avgPrice: '60000',
}
const original = {
  orderLinkId: 'original-intent', symbol: 'BTCUSDT', side: 'Buy',
  orderType: 'Limit', qty: '0.1', price: '60000', reduceOnly: false, createdAtMs: 1234,
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

describe('order submission recovery', () => {
  let pinia: Pinia
  const scopes: EffectScope[] = []
  let saved: Array<typeof original>
  let send: (request: PlaceOrderRequest) => Promise<Order>
  let lookup: (orderLinkId: string) => Promise<Order | null>
  let acknowledge: (orderLinkId: string) => Promise<void>
  const messages = { error: vi.fn(), warning: vi.fn(), success: vi.fn() }

  function panel() {
    const scope = effectScope()
    scopes.push(scope)
    return scope.run(() => useOrderPanel())!
  }

  async function draft() {
    const result = panel()
    await flushPromises()
    result.qty.value = '0.1'
    result.price.value = '60000'
    await nextTick()
    return result
  }

  function creates() {
    return vi.mocked(tauriInvoke).mock.calls.filter(([command]) => command === 'place_order')
  }

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    saved = []
    send = async (request) => ({ ...recovered, orderLinkId: request.orderLinkId })
    lookup = async () => null
    acknowledge = async (orderLinkId) => { saved = saved.filter((entry) => entry.orderLinkId !== orderLinkId) }
    Object.values(messages).forEach((message) => message.mockReset())
    installMessageApi(messages as never)
    vi.mocked(refreshSyncTask).mockReset().mockResolvedValue()
    vi.mocked(tauriInvoke).mockReset().mockImplementation(async (command, args) => {
      if (command === 'list_pending_order_submissions') return saved.map((entry) => ({ ...entry }))
      if (command === 'place_order') return send(args!.request as PlaceOrderRequest)
      if (command === 'reconcile_order_submission') return lookup(args!.orderLinkId as string)
      if (command === 'acknowledge_order_submission') return acknowledge(args!.orderLinkId as string)
      return undefined
    })
    useConnectionStore().setStatus('connected')
  })

  afterEach(() => {
    for (const scope of scopes.splice(0)) scope.stop()
    disposePinia(pinia)
  })

  it('keeps the original intent locked after timeout and an inconclusive lookup', async () => {
    send = async (request) => {
      saved = [{ ...original, ...request, orderLinkId: request.orderLinkId! }]
      throw { code: 'ORDER_SUBMISSION_UNKNOWN', message: '订单提交结果待确认', orderLinkId: request.orderLinkId }
    }
    const form = await draft()
    await form.submit()
    const request = creates()[0]![1]!.request as PlaceOrderRequest
    expect(request.orderLinkId).toMatch(/^[0-9a-f-]{36}$/i)
    expect(form.pendingSubmissions.value[0]?.orderLinkId).toBe(request.orderLinkId)
    expect(form.canSubmit.value).toBe(false)
    form.qty.value = '0.7'
    await nextTick()
    await form.submit()
    await form.reconcileSubmission(request.orderLinkId!)
    await form.submit()
    expect(creates()).toHaveLength(1)
    expect(form.canSubmit.value).toBe(false)
    expect(form.pendingSubmissions.value).toHaveLength(1)
    expect(tauriInvoke).toHaveBeenCalledWith('reconcile_order_submission', { orderLinkId: request.orderLinkId })
  })

  it('only permits one create when submit is invoked twice before the response', async () => {
    const response = deferred<Order>()
    send = () => response.promise
    const form = await draft()
    const first = form.submit()
    const second = form.submit()
    await flushPromises()
    expect(creates()).toHaveLength(1)
    response.resolve({ ...recovered, orderLinkId: (creates()[0]![1]!.request as PlaceOrderRequest).orderLinkId })
    await Promise.all([first, second])
    expect(form.qty.value).toBe('')
  })

  it('retains the original ID when transport loses a successful reply and the pending list is empty', async () => {
    send = async () => { throw new Error('IPC response timeout') }
    const form = await draft()
    await form.submit()
    expect(saved).toEqual([])
    expect(form.pendingSubmissions.value).toHaveLength(1)
    await form.submit()
    expect(creates()).toHaveLength(1)
    lookup = async (orderLinkId) => ({ ...recovered, orderLinkId })
    await form.reconcileSubmission(form.pendingSubmissions.value[0]!.orderLinkId)
    expect(form.pendingSubmissions.value).toEqual([])
    expect(form.qty.value).toBe('')
    expect(form.price.value).toBe('')
    expect(refreshSyncTask).toHaveBeenCalledWith('privatePanels', true)
    expect(refreshSyncTask).toHaveBeenCalledWith('account', true)
    send = async (request) => ({ ...recovered, orderLinkId: request.orderLinkId })
    form.qty.value = '0.2'
    form.price.value = '59000'
    await nextTick()
    expect(form.canSubmit.value).toBe(true)
    await form.submit()
    const requests = creates().map(([, args]) => args!.request as PlaceOrderRequest)
    expect(requests).toHaveLength(2)
    expect(requests[1]!.orderLinkId).not.toBe(requests[0]!.orderLinkId)
  })

  it('restores pending orders from the backend in a fresh app and renders the original query action', async () => {
    saved = [original]
    const form = await draft()
    expect(form.pendingSubmissions.value).toEqual([original])
    expect(form.canSubmit.value).toBe(false)
    scopes.splice(0).forEach((scope) => scope.stop())
    disposePinia(pinia)
    pinia = createPinia()
    setActivePinia(pinia)
    useConnectionStore().setStatus('connected')
    const wrapper = mount(OrderPanel, {
      global: { plugins: [pinia], stubs: { TradingAssetPanel: true } },
    })
    await flushPromises()
    const pending = wrapper.get('[data-testid="pending-order-submissions"]')
    expect(pending.text()).toContain('BTCUSDT')
    expect(pending.text()).toContain('0.1')
    expect(pending.text()).toContain('original-intent')
    const query = pending.findAll('button').find((button) => button.text() === '查询订单结果')!
    await query.trigger('click')
    await flushPromises()
    expect(tauriInvoke).toHaveBeenCalledWith('reconcile_order_submission', { orderLinkId: 'original-intent' })
    expect(wrapper.get('.submit-button').attributes('disabled')).toBeDefined()
    expect(creates()).toEqual([])
    wrapper.unmount()
  })

  it('discards delayed pending lists across account and epoch changes', async () => {
    const oldList = deferred<Array<typeof original>>()
    const invoke = vi.mocked(tauriInvoke).getMockImplementation()!
    vi.mocked(tauriInvoke).mockImplementation((command, args) => (
      command === 'list_pending_order_submissions' ? oldList.promise : invoke(command, args)
    ))
    const form = panel()
    await nextTick()
    vi.mocked(tauriInvoke).mockImplementation(invoke)
    useConfigStore().adoptActiveAccountId('backup')
    useAccountProfilesStore().adoptSessionEpoch(2)
    await flushPromises()
    oldList.resolve([original])
    await flushPromises()
    expect(form.pendingSubmissions.value).toEqual([])
    expect(useOrderStore().pendingLoading).toBe(false)
  })

  it('does not apply an old-account query result or clear the new account draft', async () => {
    saved = [original]
    const result = deferred<Order | null>()
    lookup = () => result.promise
    const form = await draft()
    const querying = form.reconcileSubmission(original.orderLinkId)
    saved = []
    useConfigStore().adoptActiveAccountId('backup')
    useAccountProfilesStore().adoptSessionEpoch(2)
    await flushPromises()
    form.qty.value = '0.8'
    form.price.value = '50000'
    result.resolve({ ...recovered, status: 'New' })
    await querying
    expect(form.pendingSubmissions.value).toEqual([])
    expect(form.qty.value).toBe('0.8')
    expect(useOrderStore().openOrders).toEqual([])
    expect(refreshSyncTask).not.toHaveBeenCalled()
  })

  it('allows a reduce-only close for an uncertain opening but blocks it during lookup and after an uncertain close', async () => {
    saved = [original]
    usePositionStore().setPositions([{
      symbol: 'BTCUSDT', side: 'Sell', size: '1', entryPrice: '60000', leverage: '1', unrealisedPnl: '0',
    }])
    const form = await draft()
    form.tradeMode.value = 'close'
    await nextTick()
    expect(form.canSubmit.value).toBe(true)
    const result = deferred<Order | null>()
    lookup = () => result.promise
    const querying = form.reconcileSubmission(original.orderLinkId)
    expect(form.canSubmit.value).toBe(false)
    await form.submit()
    expect(creates()).toHaveLength(0)
    result.resolve(null)
    await querying
    send = async (request) => {
      saved.push({ ...original, ...request, orderLinkId: request.orderLinkId! })
      throw { code: 'ORDER_SUBMISSION_UNKNOWN', message: '待确认', orderLinkId: request.orderLinkId }
    }
    await form.submit()
    expect(creates()).toHaveLength(1)
    expect(form.canSubmit.value).toBe(false)
    await form.submit()
    expect(creates()).toHaveLength(1)
  })

  it('keeps trading locked if restoring the pending journal fails, and lets the user query again', async () => {
    const invoke = vi.mocked(tauriInvoke).getMockImplementation()!
    vi.mocked(tauriInvoke).mockImplementation((command, args) => (
      command === 'list_pending_order_submissions'
        ? Promise.reject(new Error('journal unavailable')) : invoke(command, args)
    ))
    const form = await draft()
    expect(form.canSubmit.value).toBe(false)
    expect(form.pendingError.value).toBeTruthy()
    await form.submit()
    expect(creates()).toHaveLength(0)
    vi.mocked(tauriInvoke).mockImplementation(invoke)
    await form.refreshPendingSubmissions()
    expect(form.canSubmit.value).toBe(true)
  })

  it('does not list pending submissions until the connection is ready and account switching has ended', async () => {
    useConnectionStore().setStatus('disconnected')
    const form = panel()
    await flushPromises()
    expect(tauriInvoke).not.toHaveBeenCalledWith('list_pending_order_submissions')
    useAccountProfilesStore().reconciliationLoading = true
    useConnectionStore().setStatus('connected')
    await flushPromises()
    expect(tauriInvoke).not.toHaveBeenCalledWith('list_pending_order_submissions')
    useAccountProfilesStore().reconciliationLoading = false
    await flushPromises()
    expect(tauriInvoke).toHaveBeenCalledWith('list_pending_order_submissions')
    expect(form.pendingSubmissions.value).toEqual([])
  })

  it.each(['ORDER_SUBMISSION_REJECTED', 'ORDER_REJECTED', 'RISK_ORDER_BLOCKED', 'AUTH_SESSION_EXPIRED'])(
    'allows a corrected new intent after a certain %s rejection', async (code) => {
      send = async () => { throw { code, message: '确定未提交', notificationId: code === 'ORDER_SUBMISSION_REJECTED' ? undefined : 'notice-1' } }
      const form = await draft()
      await form.submit()
      expect(form.pendingSubmissions.value).toEqual([])
      form.qty.value = '0.2'
      await nextTick()
      send = async (request) => ({ ...recovered, orderLinkId: request.orderLinkId })
      await form.submit()
      expect(creates()).toHaveLength(2)
    },
  )

  it('unlocks a recovered pending intent only when lookup confirms a definite rejection', async () => {
    saved = [original]
    const form = await draft()
    lookup = async () => { throw { code: 'ORDER_SUBMISSION_REJECTED', message: '订单已被明确拒绝' } }
    await form.reconcileSubmission(original.orderLinkId)
    expect(form.pendingSubmissions.value).toEqual([])
    form.qty.value = '0.2'
    await nextTick()
    expect(form.canSubmit.value).toBe(true)
  })

  it('waits for durable receipt acknowledgement before clearing the draft or refreshing', async () => {
    const ack = deferred<void>()
    acknowledge = () => ack.promise
    const form = await draft()
    const submitting = form.submit()
    await flushPromises()
    expect(tauriInvoke).toHaveBeenCalledWith('acknowledge_order_submission', {
      orderLinkId: (creates()[0]![1]!.request as PlaceOrderRequest).orderLinkId,
    })
    expect(form.pendingSubmissions.value).toHaveLength(1)
    expect(form.qty.value).toBe('0.1')
    expect(refreshSyncTask).not.toHaveBeenCalled()
    await form.submit()
    expect(creates()).toHaveLength(1)
    ack.resolve()
    await submitting
    expect(form.pendingSubmissions.value).toEqual([])
    expect(form.qty.value).toBe('')
    expect(refreshSyncTask).toHaveBeenCalledWith('account', true)
  })

  it('keeps a received order pending when acknowledgement fails and only retries its lookup', async () => {
    acknowledge = async () => { throw new Error('journal write failed') }
    const form = await draft()
    await form.submit()
    expect(form.pendingSubmissions.value).toHaveLength(1)
    expect(form.pendingStatusMessage.value).toContain('已确认')
    expect(form.pendingStatusMessage.value).toContain('再次查询')
    expect(messages.error).not.toHaveBeenCalled()
    expect(messages.success).not.toHaveBeenCalled()
    expect(refreshSyncTask).not.toHaveBeenCalled()
    await form.submit()
    expect(creates()).toHaveLength(1)
    lookup = async (orderLinkId) => ({ ...recovered, orderLinkId })
    await form.reconcileSubmission(form.pendingSubmissions.value[0]!.orderLinkId)
    expect(form.pendingSubmissions.value).toHaveLength(1)
    acknowledge = async () => undefined
    await form.reconcileSubmission(form.pendingSubmissions.value[0]!.orderLinkId)
    expect(form.pendingSubmissions.value).toEqual([])
    expect(form.qty.value).toBe('')
    expect(creates()).toHaveLength(1)
  })

  it('restores an accepted but unacknowledged submission after a lost reply and restart', async () => {
    send = async (request) => {
      saved = [{ ...original, ...request, orderLinkId: request.orderLinkId! }]
      throw new Error('accepted response lost in transport')
    }
    const initial = await draft()
    await initial.submit()
    const originalId = saved[0]!.orderLinkId
    expect(messages.error).not.toHaveBeenCalled()
    scopes.splice(0).forEach((scope) => scope.stop())
    disposePinia(pinia)
    pinia = createPinia()
    setActivePinia(pinia)
    useConnectionStore().setStatus('connected')
    const restarted = await draft()
    expect(restarted.pendingSubmissions.value[0]?.orderLinkId).toBe(originalId)
    expect(restarted.canSubmit.value).toBe(false)
    await restarted.submit()
    expect(creates()).toHaveLength(1)
    lookup = async (orderLinkId) => ({ ...recovered, orderLinkId })
    await restarted.reconcileSubmission(originalId)
    expect(saved).toEqual([])
    expect(restarted.pendingSubmissions.value).toEqual([])
    expect(restarted.qty.value).toBe('')
    expect(creates()).toHaveLength(1)
  })

  it.each([undefined, 'different-intent'])(
    'does not acknowledge a receipt with missing or mismatched link ID (%s)', async (orderLinkId) => {
      send = async () => ({ ...recovered, orderLinkId })
      const form = await draft()
      await form.submit()
      expect(form.pendingSubmissions.value).toHaveLength(1)
      expect(tauriInvoke).not.toHaveBeenCalledWith('acknowledge_order_submission', expect.anything())
      lookup = async () => ({ ...recovered, orderLinkId })
      await form.reconcileSubmission(form.pendingSubmissions.value[0]!.orderLinkId)
      expect(form.pendingSubmissions.value).toHaveLength(1)
      expect(tauriInvoke).not.toHaveBeenCalledWith('acknowledge_order_submission', expect.anything())
      expect(refreshSyncTask).not.toHaveBeenCalled()
    },
  )

  it('does not clear the new account draft when an old acknowledgement completes', async () => {
    saved = [original]
    const ack = deferred<void>()
    acknowledge = () => ack.promise
    lookup = async () => recovered
    const form = await draft()
    const querying = form.reconcileSubmission(original.orderLinkId)
    await flushPromises()
    expect(tauriInvoke).toHaveBeenCalledWith('acknowledge_order_submission', { orderLinkId: original.orderLinkId })
    saved = []
    useConfigStore().adoptActiveAccountId('backup')
    useAccountProfilesStore().adoptSessionEpoch(2)
    await flushPromises()
    form.qty.value = '0.8'
    form.price.value = '50000'
    ack.resolve()
    await querying
    expect(form.qty.value).toBe('0.8')
    expect(form.price.value).toBe('50000')
    expect(refreshSyncTask).not.toHaveBeenCalled()
  })
})
