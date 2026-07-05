import { describe, expect, it } from 'vitest'
import { normalizeOrder, normalizeOrders } from '../../src/utils/order'

describe('normalizeOrder', () => {
  it('maps snake_case api fields to frontend order shape', () => {
    const order = normalizeOrder({
      order_id: 'abc123',
      symbol: 'BTCUSDT',
      side: 'Buy',
      order_type: 'Limit',
      price: '60000',
      qty: '0.01',
      order_status: 'New',
      cum_exec_qty: '0',
      avg_price: '0',
    })

    expect(order).toEqual({
      orderId: 'abc123',
      symbol: 'BTCUSDT',
      side: 'Buy',
      orderType: 'Limit',
      price: '60000',
      qty: '0.01',
      status: 'New',
      orderLinkId: undefined,
      filledQty: '0',
      avgPrice: '0',
    })
  })

  it('drops rows without order id', () => {
    expect(
      normalizeOrders([
        { order_id: '1', symbol: 'BTCUSDT' },
        { symbol: 'ETHUSDT' },
      ]),
    ).toHaveLength(1)
  })
})
