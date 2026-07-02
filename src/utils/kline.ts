import type { CandlestickData, UTCTimestamp } from 'lightweight-charts'
import type { Kline } from '../types/models'

export function toCandlestickData(klines: Kline[]): CandlestickData[] {
  return klines.map((k) => ({
    time: Math.floor(k.openTime / 1000) as UTCTimestamp,
    open: parseFloat(k.open),
    high: parseFloat(k.high),
    low: parseFloat(k.low),
    close: parseFloat(k.close),
  }))
}
