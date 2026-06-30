import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { CancelOrderRequest, Order, PlaceOrderRequest } from '../types/models'

export const useOrderStore = defineStore('order', () => {
  const orders = ref<Order[]>([])

  function upsertOrder(order: Order): void {
    const idx = orders.value.findIndex((o) => o.orderId === order.orderId)
    if (idx >= 0) {
      orders.value[idx] = order
    } else {
      orders.value.unshift(order)
    }
  }

  async function placeOrder(request: PlaceOrderRequest): Promise<Order> {
    const order = await tauriInvoke<Order>('place_order', { request })
    upsertOrder(order)
    return order
  }

  async function cancelOrder(request: CancelOrderRequest): Promise<Order> {
    const order = await tauriInvoke<Order>('cancel_order', { request })
    upsertOrder(order)
    return order
  }

  async function refreshOrders(symbol?: string): Promise<void> {
    orders.value = await tauriInvoke<Order[]>('refresh_orders', { symbol: symbol ?? null })
  }

  return { orders, upsertOrder, placeOrder, cancelOrder, refreshOrders }
})
