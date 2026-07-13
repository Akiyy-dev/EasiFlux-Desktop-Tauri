import { describe, expect, it } from 'vitest'
import { pnlToneClass, sumUnrealisedPnl } from '../../src/utils/dashboardAssets'
import type { Position } from '../../src/types/models'

const samplePositions: Position[] = [
  {
    symbol: 'BTCUSDT',
    side: 'Buy',
    size: '0.1',
    entryPrice: '60000',
    leverage: '10',
    unrealisedPnl: '12.5',
  },
  {
    symbol: 'ETHUSDT',
    side: 'Sell',
    size: '1',
    entryPrice: '3000',
    leverage: '5',
    unrealisedPnl: '-3.2',
  },
]

describe('dashboardAssets', () => {
  it('sums unrealised pnl across positions', () => {
    expect(sumUnrealisedPnl(samplePositions)).toBe('9.3000')
  })

  it('returns zero when no positions', () => {
    expect(sumUnrealisedPnl([])).toBe('0.0000')
  })

  it('deduplicates the same position before summing', () => {
    expect(
      sumUnrealisedPnl([
        { ...samplePositions[0], positionIdx: 1 },
        { ...samplePositions[0], unrealisedPnl: '13.5', positionIdx: 1 },
      ]),
    ).toBe('13.5000')
  })

  it('maps pnl tone class', () => {
    expect(pnlToneClass('1.2')).toBe('text-up')
    expect(pnlToneClass('-1.2')).toBe('text-down')
    expect(pnlToneClass('--')).toBe('')
  })
})
