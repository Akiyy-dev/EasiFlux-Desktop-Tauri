import type { CandlestickData, UTCTimestamp } from 'lightweight-charts'
import type { Kline } from '../types/models'

function isValidPrice(value: number): boolean {
  return Number.isFinite(value)
}

export function toCandlestickData(klines: Kline[]): CandlestickData[] {
  const byTime = new Map<number, CandlestickData>()

  for (const kline of klines) {
    if (kline.openTime <= 0) {
      continue
    }
    const open = parseFloat(kline.open)
    const high = parseFloat(kline.high)
    const low = parseFloat(kline.low)
    const close = parseFloat(kline.close)
    if (!isValidPrice(open) || !isValidPrice(high) || !isValidPrice(low) || !isValidPrice(close)) {
      continue
    }
    const time = Math.floor(kline.openTime / 1000)
    byTime.set(time, {
      time: time as UTCTimestamp,
      open,
      high,
      low,
      close,
    })
  }

  return Array.from(byTime.entries())
    .sort(([a], [b]) => a - b)
    .map(([, candle]) => candle)
}

export function candlesEqual(a: CandlestickData, b: CandlestickData): boolean {
  return (
    a.time === b.time &&
    a.open === b.open &&
    a.high === b.high &&
    a.low === b.low &&
    a.close === b.close
  )
}
