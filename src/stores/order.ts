import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { CancelAllOrdersRequest, CancelOrderRequest, Order, PlaceOrderRequest } from '../types/models'
import { isTerminalOrderStatus, normalizeOrder, normalizeOrders } from '../utils/order'
import { useAsyncState } from '../composables/useAsyncState'
import { assertTradingMutationAllowed } from '../utils/tradingMutationGuard'
import { useAccountProfilesStore } from './accountProfiles'
import { useConnectionStore } from './connection'
import { decodeCommandError } from '../services/notificationService'
import type { PendingOrderSubmission } from '../types/orderSubmission'

const PENDING_MESSAGE = '订单提交结果待确认，请查询原订单，勿重复提交'
const ACK_PENDING_MESSAGE = '原订单结果已确认，但确认记录尚未保存，请再次查询原订单，勿重复提交'
const CERTAIN_REJECTIONS = new Set([
  'ORDER_SUBMISSION_REJECTED', 'ORDER_REJECTED', 'RISK_ORDER_BLOCKED',
  'AUTH_SESSION_EXPIRED', 'ACCOUNT_SESSION_EXPIRED',
])

function isPendingSubmission(value: unknown): value is PendingOrderSubmission {
  if (typeof value !== 'object' || value === null) return false
  const row = value as Record<string, unknown>
  return ['orderLinkId', 'symbol', 'side', 'orderType', 'qty']
    .every((key) => typeof row[key] === 'string' && row[key] !== '')
    && typeof row.createdAtMs === 'number' && Number.isSafeInteger(row.createdAtMs)
    && (row.reduceOnly == null || typeof row.reduceOnly === 'boolean')
    && (row.price == null || typeof row.price === 'string')
}

function requireSubmissionReceipt(value: unknown, orderLinkId: string): Order {
  if (typeof value !== 'object' || value === null) throw new Error('订单回执无效')
  const receipt = value as Record<string, unknown>
  if (receipt.orderLinkId !== orderLinkId
    || !['orderId', 'symbol', 'side', 'orderType', 'qty', 'status']
      .every((key) => typeof receipt[key] === 'string' && String(receipt[key]).trim() !== '')
    || !['price', 'filledQty', 'avgPrice'].every((key) => typeof receipt[key] === 'string')) {
    throw new Error('订单回执与原提交标识不匹配')
  }
  return normalizeOrder(value as Order)
}

export const useOrderStore = defineStore('order', () => {
  const openOrders = ref<Order[]>([])
  const orderHistory = ref<Order[]>([])
  const openOrdersRequest = useAsyncState<Order[]>((value) => value.length === 0)
  const historyRequest = useAsyncState<Order[]>((value) => value.length === 0)
  const profilesStore = useAccountProfilesStore()
  const connectionStore = useConnectionStore()
  const pendingSubmissions = ref<PendingOrderSubmission[]>([])
  const pendingLoading = ref(false)
  const pendingLoaded = ref(false)
  const pendingError = ref<string | null>(null)
  const pendingStatusMessage = ref<string | null>(null)
  const queryingOrderLinkId = ref<string | null>(null)
  const placing = ref(false)
  const submissionSession = ref(0)
  let pendingRefresh: Promise<void> | null = null

  function canReadSubmissions(): boolean {
    return connectionStore.connected && !profilesStore.switching
  }

  function submissionContext() {
    return {
      accountId: profilesStore.activeAccountId,
      epoch: profilesStore.sessionEpoch,
      generation: submissionSession.value,
    }
  }

  function ownsSubmission(context: ReturnType<typeof submissionContext>): boolean {
    return context.accountId === profilesStore.activeAccountId
      && context.epoch === profilesStore.sessionEpoch
      && context.generation === submissionSession.value
      && canReadSubmissions()
  }

  function submissionBlockedMessage(reduceOnly = false): string | null {
    if (placing.value) return '订单正在提交，请稍候'
    if (queryingOrderLinkId.value) return '正在查询原订单结果，请稍候'
    if (pendingLoading.value || !pendingLoaded.value) return '正在核对待确认订单，请稍候'
    if (pendingError.value) return pendingError.value
    if (pendingSubmissions.value.length > 0
      && (!reduceOnly || pendingSubmissions.value.some((order) => order.reduceOnly))) {
      return PENDING_MESSAGE
    }
    return null
  }

  function refreshPendingSubmissions(): Promise<void> {
    if (!canReadSubmissions() || queryingOrderLinkId.value) return Promise.resolve()
    if (pendingRefresh) return pendingRefresh
    const context = submissionContext()
    pendingLoading.value = true
    pendingError.value = null
    const operation = (async () => {
      try {
        const result = await tauriInvoke<unknown>('list_pending_order_submissions')
        if (!ownsSubmission(context)) return
        if (!Array.isArray(result) || !result.every(isPendingSubmission)) {
          throw new Error('待确认订单记录格式无效')
        }
        // Keep local intents until acknowledgement, even if a journal read
        // races with the create or the receipt acknowledgement.
        const merged = new Map(pendingSubmissions.value.map((entry) => [entry.orderLinkId, entry]))
        for (const entry of result) merged.set(entry.orderLinkId, entry)
        pendingSubmissions.value = [...merged.values()]
        pendingLoaded.value = true
      } catch {
        if (ownsSubmission(context)) {
          pendingError.value = '待确认订单读取失败，请重新查询后再提交'
        }
      } finally {
        if (ownsSubmission(context)) pendingLoading.value = false
      }
    })().finally(() => {
      if (pendingRefresh === operation) pendingRefresh = null
    })
    pendingRefresh = operation
    return operation
  }

  function resetSubmissionState(clearPending = true): void {
    submissionSession.value += 1
    pendingRefresh = null
    if (clearPending) pendingSubmissions.value = []
    pendingLoading.value = false
    pendingLoaded.value = false
    pendingError.value = null
    pendingStatusMessage.value = null
    queryingOrderLinkId.value = null
    placing.value = false
  }

  watch(
    [() => profilesStore.activeAccountId, () => profilesStore.sessionEpoch,
      () => connectionStore.connected, () => profilesStore.switching],
    (next, previous) => {
      resetSubmissionState(!previous || next[0] !== previous[0] || next[1] !== previous[1])
    },
    { immediate: true, flush: 'sync' },
  )
  watch(
    [() => profilesStore.activeAccountId, () => profilesStore.sessionEpoch,
      () => connectionStore.connected, () => profilesStore.switching],
    () => { void refreshPendingSubmissions() },
    { immediate: true },
  )

  function assertCanMutate(): void {
    assertTradingMutationAllowed(useAccountProfilesStore().tradingBlockedMessage)
  }

  function upsertOpenOrder(order: Order): void {
    const normalized = normalizeOrder(order)
    if (!normalized.orderId) {
      return
    }
    if (isTerminalOrderStatus(normalized.status)) {
      openOrders.value = openOrders.value.filter((o) => o.orderId !== normalized.orderId)
      return
    }
    const idx = openOrders.value.findIndex((o) => o.orderId === normalized.orderId)
    if (idx >= 0) {
      openOrders.value[idx] = normalized
    } else {
      openOrders.value.unshift(normalized)
    }
  }

  function upsertOrder(order: Order): void {
    upsertOpenOrder(order)
  }

  async function acknowledgeReceipt(
    orderLinkId: string,
    context: ReturnType<typeof submissionContext>,
  ): Promise<boolean> {
    if (!ownsSubmission(context)) return false
    try {
      await tauriInvoke<void>('acknowledge_order_submission', { orderLinkId })
    } catch {
      if (!ownsSubmission(context)) return false
      throw { code: 'ORDER_SUBMISSION_ACK_PENDING', message: ACK_PENDING_MESSAGE, orderLinkId }
    }
    return ownsSubmission(context)
  }

  async function placeOrder(request: PlaceOrderRequest): Promise<Order> {
    assertCanMutate()
    if (placing.value || queryingOrderLinkId.value) {
      throw new Error(submissionBlockedMessage(request.reduceOnly)!)
    }
    const context = submissionContext()
    placing.value = true
    try {
      if (!pendingLoaded.value || pendingLoading.value) await refreshPendingSubmissions()
      assertCanMutate()
      if (!ownsSubmission(context)) throw new Error('账户会话已变更，请重新确认订单')
      if (!pendingLoaded.value || pendingError.value) {
        throw new Error(pendingError.value ?? '待确认订单尚未完成核对')
      }
      if (pendingSubmissions.value.length > 0
        && (!request.reduceOnly || pendingSubmissions.value.some((entry) => entry.reduceOnly))) {
        throw new Error(PENDING_MESSAGE)
      }
      const intent = { ...request, orderLinkId: request.orderLinkId ?? crypto.randomUUID() }
      pendingSubmissions.value.push({ ...intent, createdAtMs: Date.now() })
      try {
        const order = requireSubmissionReceipt(
          await tauriInvoke<unknown>('place_order', { request: intent }), intent.orderLinkId,
        )
        if (await acknowledgeReceipt(intent.orderLinkId, context)) {
          pendingSubmissions.value = pendingSubmissions.value.filter((entry) => entry.orderLinkId !== intent.orderLinkId)
          upsertOpenOrder(order)
        }
        return order
      } catch (error) {
        if (!ownsSubmission(context)) throw error
        const decoded = decodeCommandError(error)
        if (CERTAIN_REJECTIONS.has(decoded.code ?? '') || decoded.code === 'ORDER_SUBMISSION_BLOCKED') {
          pendingSubmissions.value = pendingSubmissions.value.filter((entry) => entry.orderLinkId !== intent.orderLinkId)
        }
        if (!CERTAIN_REJECTIONS.has(decoded.code ?? '')) {
          pendingStatusMessage.value = decoded.code === 'ORDER_SUBMISSION_ACK_PENDING'
            ? ACK_PENDING_MESSAGE : PENDING_MESSAGE
          await refreshPendingSubmissions()
          throw {
            code: decoded.code === 'ORDER_SUBMISSION_ACK_PENDING'
              ? decoded.code : 'ORDER_SUBMISSION_UNKNOWN',
            message: pendingStatusMessage.value,
            orderLinkId: intent.orderLinkId,
          }
        }
        throw error
      }
    } finally {
      if (ownsSubmission(context)) placing.value = false
    }
  }

  async function reconcileSubmission(orderLinkId: string): Promise<Order | null> {
    if (!canReadSubmissions() || pendingLoading.value || placing.value || queryingOrderLinkId.value
      || !pendingSubmissions.value.some((entry) => entry.orderLinkId === orderLinkId)) return null
    const context = submissionContext()
    queryingOrderLinkId.value = orderLinkId
    pendingError.value = null
    pendingStatusMessage.value = null
    try {
      const result = await tauriInvoke<Order | null>('reconcile_order_submission', { orderLinkId })
      if (!ownsSubmission(context)) return null
      if (result === null) {
        pendingStatusMessage.value = '暂未确认原订单结果，请稍后再次查询，勿重复提交'
        return null
      }
      const order = requireSubmissionReceipt(result, orderLinkId)
      if (!await acknowledgeReceipt(orderLinkId, context)) return null
      pendingSubmissions.value = pendingSubmissions.value.filter((entry) => entry.orderLinkId !== orderLinkId)
      upsertOpenOrder(order)
      pendingStatusMessage.value = '原订单结果已确认'
      return order
    } catch (error) {
      if (!ownsSubmission(context)) return null
      const decoded = decodeCommandError(error)
      if (decoded.code === 'ORDER_SUBMISSION_REJECTED') {
        pendingSubmissions.value = pendingSubmissions.value.filter((entry) => entry.orderLinkId !== orderLinkId)
        pendingStatusMessage.value = '原订单已明确拒绝，可修改后重新提交'
      } else if (decoded.code === 'ORDER_SUBMISSION_ACK_PENDING') {
        pendingStatusMessage.value = ACK_PENDING_MESSAGE
      } else {
        pendingError.value = '原订单查询失败，请稍后再次查询，勿重复提交'
      }
      return null
    } finally {
      if (ownsSubmission(context)) queryingOrderLinkId.value = null
    }
  }

  async function cancelOrder(request: CancelOrderRequest): Promise<Order> {
    assertCanMutate()
    const order = normalizeOrder(await tauriInvoke<Order>('cancel_order', { request }))
    upsertOpenOrder(order)
    return order
  }

  async function cancelAllOrders(request: CancelAllOrdersRequest = {}): Promise<void> {
    assertCanMutate()
    await tauriInvoke('cancel_all_orders', { request })
  }

  async function refreshOrders(symbol?: string): Promise<void> {
    await openOrdersRequest.run(
      () => tauriInvoke<Order[]>('refresh_orders', { symbol: symbol ?? null }),
      (raw) => { openOrders.value = normalizeOrders(raw) },
    )
  }

  async function refreshOrderHistory(symbol?: string, limit = 50): Promise<void> {
    await historyRequest.run(
      () => tauriInvoke<Order[]>('refresh_order_history', {
        symbol: symbol ?? null,
        limit,
      }),
      (raw) => { orderHistory.value = normalizeOrders(raw) },
    )
  }

  async function refreshAll(symbol?: string): Promise<void> {
    await Promise.all([refreshOrders(symbol), refreshOrderHistory(symbol)])
  }

  function setOpenOrders(next: Order[]): void {
    openOrders.value = next
  }

  function setOrderHistory(next: Order[]): void {
    orderHistory.value = next
  }

  function clearOrders(): void {
    openOrders.value = []
    orderHistory.value = []
    openOrdersRequest.reset()
    historyRequest.reset()
    resetSubmissionState()
    void refreshPendingSubmissions()
  }

  return {
    openOrders,
    orderHistory,
    pendingSubmissions,
    pendingLoading,
    pendingError,
    pendingStatusMessage,
    queryingOrderLinkId,
    placing,
    submissionSession,
    submissionBlockedMessage,
    refreshPendingSubmissions,
    reconcileSubmission,
    openOrdersLoading: openOrdersRequest.loading,
    openOrdersError: openOrdersRequest.error,
    openOrdersStatus: openOrdersRequest.status,
    orderHistoryLoading: historyRequest.loading,
    orderHistoryError: historyRequest.error,
    orderHistoryStatus: historyRequest.status,
    upsertOrder,
    placeOrder,
    cancelOrder,
    cancelAllOrders,
    refreshOrders,
    refreshOrderHistory,
    refreshAll,
    setOpenOrders,
    setOrderHistory,
    clearOrders,
  }
})
