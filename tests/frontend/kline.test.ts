import { describe, expect, it } from 'vitest'
import { klineBarsEqual, toKLineData } from '../../src/utils/klinecharts'
import type { Kline } from '../../src/types/models'

describe('toKLineData', () => {
  it('keeps millisecond timestamp and volume', () => {
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

    expect(toKLineData(klines)).toEqual([
      {
        timestamp: 1_700_000_000_000,
        open: 100,
        high: 110,
        low: 90,
        close: 105,
        volume: 12,
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

    const data = toKLineData(klines)
    expect(data).toHaveLength(2)
    expect(data[0]?.timestamp).toBe(1_700_000_000_000)
    expect(data[1]?.timestamp).toBe(1_700_000_002_000)
  })
})

describe('klineBarsEqual', () => {
  it('compares OHLC timestamp and volume', () => {
    const a = {
      timestamp: 1,
      open: 1,
      high: 2,
      low: 0.5,
      close: 1.5,
      volume: 3,
    }
    const b = { ...a }
    const c = { ...a, close: 2 }
    expect(klineBarsEqual(a, b)).toBe(true)
    expect(klineBarsEqual(a, c)).toBe(false)
  })
})
