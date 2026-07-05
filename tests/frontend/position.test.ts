import { describe, expect, it } from 'vitest'
import { normalizePositions } from '../../src/utils/position'

describe('normalizePositions', () => {
  it('maps snake_case position fields and filters zero size', () => {
    const positions = normalizePositions([
      {
        symbol: 'BTCUSDT',
        position_side: 'Buy',
        position_amt: '0.02',
        entry_price: '60000',
        leverage: '10',
        unrealised_pnl: '12.5',
        position_idx: 1,
      },
      {
        symbol: 'ETHUSDT',
        position_side: 'Sell',
        position_amt: '0',
      },
    ])

    expect(positions).toHaveLength(1)
    expect(positions[0]).toEqual({
      symbol: 'BTCUSDT',
      side: 'Buy',
      size: '0.02',
      entryPrice: '60000',
      leverage: '10',
      unrealisedPnl: '12.5',
      positionIdx: 1,
    })
  })
})
