import type { Order } from '../types/models'

export type OpenOrderScope = 'limit' | 'plan' | 'tpsl'

export function classifyOpenOrder(order: Order): OpenOrderScope {
  const type = order.orderType.toLowerCase()
  if (
    type.includes('tpsl') ||
    type.includes('takeprofit') ||
    type.includes('take_profit') ||
    type.includes('stoploss') ||
    type.includes('stop_loss')
  ) {
    return 'tpsl'
  }
  if (
    type.includes('plan') ||
    type.includes('trigger') ||
    type.includes('conditional') ||
    type.includes('stoporder')
  ) {
    return 'plan'
  }
  return 'limit'
}

export function filterOpenOrders(orders: Order[], scope: OpenOrderScope): Order[] {
  return orders.filter((order) => classifyOpenOrder(order) === scope)
}
