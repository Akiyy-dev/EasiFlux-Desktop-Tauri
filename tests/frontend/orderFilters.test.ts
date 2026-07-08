import { describe, expect, it } from 'vitest'
import { classifyOpenOrder, filterOpenOrders } from '../../src/utils/orderFilters'
import type { Order } from '../../src/types/models'

function order(orderType: string): Order {
  return {
    orderId: `id-${orderType}`,
    symbol: 'BTCUSDT',
    side: 'Buy',
    orderType,
    price: '1',
    qty: '1',
    status: 'New',
    filledQty: '0',
    avgPrice: '0',
  }
}

describe('orderFilters', () => {
  it('classifies tpsl and plan orders', () => {
    expect(classifyOpenOrder(order('Limit'))).toBe('limit')
    expect(classifyOpenOrder(order('Trigger'))).toBe('plan')
    expect(classifyOpenOrder(order('TakeProfit'))).toBe('tpsl')
  })

  it('filters open orders by scope', () => {
    const orders = [order('Limit'), order('Trigger'), order('TakeProfit')]
    expect(filterOpenOrders(orders, 'limit')).toHaveLength(1)
    expect(filterOpenOrders(orders, 'plan')).toHaveLength(1)
    expect(filterOpenOrders(orders, 'tpsl')).toHaveLength(1)
  })
})
