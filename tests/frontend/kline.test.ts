import { describe, expect, it } from 'vitest'
import { candlesEqual, toCandlestickData } from '../../src/utils/kline'
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

  it('filters invalid openTime and sorts ascending', () => {
    const klines: Kline[] = [
      {
        symbol: 'BTCUSDT',
        interval: '1',
        openTime: 0,
        open: '1',
        high: '2',
        low: '0.5',
        close: '1.5',
        volume: '1',
      },
      {
        symbol: 'BTCUSDT',
        interval: '1',
        openTime: 1_700_000_002_000,
        open: '102',
        high: '112',
        low: '92',
        close: '107',
        volume: '12',
      },
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

    const data = toCandlestickData(klines)
    expect(data).toHaveLength(2)
    expect(data[0]?.time).toBe(1_700_000_000)
    expect(data[1]?.time).toBe(1_700_000_002)
  })

  it('deduplicates by time keeping the last entry', () => {
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
      {
        symbol: 'BTCUSDT',
        interval: '1',
        openTime: 1_700_000_000_000,
        open: '101',
        high: '111',
        low: '91',
        close: '106',
        volume: '13',
      },
    ]

    expect(toCandlestickData(klines)).toEqual([
      {
        time: 1_700_000_000,
        open: 101,
        high: 111,
        low: 91,
        close: 106,
      },
    ])
  })
})

describe('candlesEqual', () => {
  it('compares OHLC and time', () => {
    const a = { time: 1 as const, open: 1, high: 2, low: 0.5, close: 1.5 }
    const b = { time: 1 as const, open: 1, high: 2, low: 0.5, close: 1.5 }
    const c = { time: 1 as const, open: 1, high: 2, low: 0.5, close: 2 }
    expect(candlesEqual(a, b)).toBe(true)
    expect(candlesEqual(a, c)).toBe(false)
  })
})
