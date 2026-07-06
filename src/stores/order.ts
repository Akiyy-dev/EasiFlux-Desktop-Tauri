import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { CancelAllOrdersRequest, CancelOrderRequest, Order, PlaceOrderRequest } from '../types/models'
import { isTerminalOrderStatus, normalizeOrder, normalizeOrders } from '../utils/order'

export const useOrderStore = defineStore('order', () => {
  const openOrders = ref<Order[]>([])
  const orderHistory = ref<Order[]>([])

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
    const order = normalizeOrder(await tauriInvoke<Order>('place_order', { request }))
    upsertOpenOrder(order)
    return order
  }

  async function cancelOrder(request: CancelOrderRequest): Promise<Order> {
    const order = normalizeOrder(await tauriInvoke<Order>('cancel_order', { request }))
    upsertOpenOrder(order)
    return order
  }

  async function cancelAllOrders(request: CancelAllOrdersRequest = {}): Promise<void> {
    await tauriInvoke('cancel_all_orders', { request })
    await refreshOrders(request.symbol)
  }

  async function refreshOrders(symbol?: string): Promise<void> {
    const raw = await tauriInvoke<Order[]>('refresh_orders', { symbol: symbol ?? null })
    openOrders.value = normalizeOrders(raw)
  }

  async function refreshOrderHistory(symbol?: string, limit = 50): Promise<void> {
    const raw = await tauriInvoke<Order[]>('refresh_order_history', {
      symbol: symbol ?? null,
      limit,
    })
    orderHistory.value = normalizeOrders(raw)
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

  return {
    openOrders,
    orderHistory,
    upsertOrder,
    placeOrder,
    cancelOrder,
    cancelAllOrders,
    refreshOrders,
    refreshOrderHistory,
    refreshAll,
    setOpenOrders,
    setOrderHistory,
  }
})
