import { describe, expect, it } from 'vitest'
import {
  calculateQuickQty,
  findClosablePosition,
  validateOrderDraft,
} from '../../src/utils/orderForm'
import type { Position } from '../../src/types/models'

function position(side: string, size: string, positionIdx: number): Position {
  return {
    symbol: 'ETHUSDT',
    side,
    size,
    entryPrice: '1800',
    leverage: '20',
    unrealisedPnl: '0',
    positionIdx,
  }
}

describe('order form helpers', () => {
  it('matches buy-close with short and sell-close with long positions', () => {
    const positions = [position('Buy', '0.5', 1), position('Sell', '0.3', 2)]

    expect(findClosablePosition(positions, 'ETHUSDT', 'Buy')?.positionIdx).toBe(2)
    expect(findClosablePosition(positions, 'ETHUSDT', 'Sell')?.positionIdx).toBe(1)
  })

  it('calculates close percentage from closeable quantity', () => {
    expect(
      calculateQuickQty({
        mode: 'close',
        percent: 50,
        closeableQty: 0.4,
        availableBalance: 0,
        referencePrice: 0,
        leverage: 1,
      }),
    ).toBeCloseTo(0.2)
  })

  it('rejects over-closing and invalid limit prices', () => {
    expect(
      validateOrderDraft({
        connected: true,
        mode: 'close',
        orderType: 'Market',
        qty: 0.5,
        price: Number.NaN,
        closeableQty: 0.4,
      }),
    ).toBe('平仓数量不能超过当前可平数量')
    expect(
      validateOrderDraft({
        connected: true,
        mode: 'open',
        orderType: 'Limit',
        qty: 0.1,
        price: Number.NaN,
        closeableQty: 0,
      }),
    ).toBe('请输入有效价格')
  })
})
