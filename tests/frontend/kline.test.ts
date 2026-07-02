import { describe, expect, it } from 'vitest'
import { toCandlestickData } from '../../src/utils/kline'
import type { Kline } from '../../src/types/models'

describe('toCandlestickData', () => {
  it('converts millisecond openTime to UTC seconds', () => {
    const klines: Kline[] = [
      {
        symbol: 'BTCUSDT',
        interval: '1',
        openTime: 1_700_000_000_000,
        open: '100',
        high: '110',
        low: '90',
        close: '105',
        volume: '12',
      },
    ]

    expect(toCandlestickData(klines)).toEqual([
      {
        time: 1_700_000_000,
        open: 100,
        high: 110,
        low: 90,
        close: 105,
      },
    ])
  })
})
