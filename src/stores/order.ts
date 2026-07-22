import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { CancelAllOrdersRequest, CancelOrderRequest, Order, PlaceOrderRequest } from '../types/models'
import { isTerminalOrderStatus, normalizeOrder, normalizeOrders } from '../utils/order'
import { useAsyncState } from '../composables/useAsyncState'
import { assertTradingMutationAllowed } from '../utils/tradingMutationGuard'
import { useAccountProfilesStore } from './accountProfiles'

export const useOrderStore = defineStore('order', () => {
  const openOrders = ref<Order[]>([])
  const orderHistory = ref<Order[]>([])
  const openOrdersRequest = useAsyncState<Order[]>((value) => value.length === 0)
  const historyRequest = useAsyncState<Order[]>((value) => value.length === 0)

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

  async function placeOrder(request: PlaceOrderRequest): Promise<Order> {
    assertCanMutate()
    const order = normalizeOrder(await tauriInvoke<Order>('place_order', { request }))
    upsertOpenOrder(order)
    return order
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
  }

  return {
    openOrders,
    orderHistory,
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
