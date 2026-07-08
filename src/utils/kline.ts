import type { Kline } from '../types/models'
import { klineBarsEqual, toKLineData } from './klinecharts'

export {
  intervalToPeriod,
  KLINE_PERIODS,
  klineBarsEqual,
  periodToInterval,
  toKLineData,
  toSymbolInfo,
} from './klinecharts'

/** @deprecated Use toKLineData */
export function toCandlestickData(klines: Kline[]): Array<{
  time: number
  open: number
  high: number
  low: number
  close: number
}> {
  return toKLineData(klines).map((bar) => ({
    time: Math.floor(bar.timestamp / 1000),
    open: bar.open,
    high: bar.high,
    low: bar.low,
    close: bar.close,
  }))
}

/** @deprecated Use klineBarsEqual */
export function candlesEqual(
  a: { time: number; open: number; high: number; low: number; close: number },
  b: { time: number; open: number; high: number; low: number; close: number },
): boolean {
  return (
    a.time === b.time &&
    a.open === b.open &&
    a.high === b.high &&
    a.low === b.low &&
    a.close === b.close
  )
}

export function candlesEqualKline(a: ReturnType<typeof toKLineData>[number], b: ReturnType<typeof toKLineData>[number]): boolean {
  return klineBarsEqual(a, b)
}
