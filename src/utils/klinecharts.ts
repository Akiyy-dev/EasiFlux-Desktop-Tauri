import type { KLineData } from 'klinecharts'
import type { Period, SymbolInfo } from '@klinecharts/pro'
import type { Kline } from '../types/models'

export const KLINE_PERIODS: Period[] = [
  { multiplier: 1, timespan: 'minute', text: '1m' },
  { multiplier: 5, timespan: 'minute', text: '5m' },
  { multiplier: 15, timespan: 'minute', text: '15m' },
  { multiplier: 1, timespan: 'hour', text: '1h' },
  { multiplier: 4, timespan: 'hour', text: '4h' },
  { multiplier: 1, timespan: 'day', text: '1D' },
]

const INTERVAL_BY_PERIOD_KEY: Record<string, string> = {
  '1-minute': '1',
  '5-minute': '5',
  '15-minute': '15',
  '1-hour': '60',
  '4-hour': '240',
  '1-day': 'D',
}

const PERIOD_BY_INTERVAL: Record<string, Period> = {
  '1': KLINE_PERIODS[0]!,
  '5': KLINE_PERIODS[1]!,
  '15': KLINE_PERIODS[2]!,
  '60': KLINE_PERIODS[3]!,
  '240': KLINE_PERIODS[4]!,
  D: KLINE_PERIODS[5]!,
}

function periodKey(period: Period): string {
  return `${period.multiplier}-${period.timespan}`
}

export function intervalToPeriod(interval: string): Period {
  return PERIOD_BY_INTERVAL[interval] ?? KLINE_PERIODS[0]!
}

export function periodToInterval(period: Period): string {
  return INTERVAL_BY_PERIOD_KEY[periodKey(period)] ?? '1'
}

export function toSymbolInfo(symbol: string): SymbolInfo {
  return {
    ticker: symbol,
    shortName: symbol,
    name: symbol,
    exchange: 'EasiCoin',
    market: 'futures',
    priceCurrency: 'USDT',
    type: 'crypto',
  }
}

function isValidPrice(value: number): boolean {
  return Number.isFinite(value)
}

export function toKLineData(klines: Kline[]): KLineData[] {
  const byTime = new Map<number, KLineData>()

  for (const kline of klines) {
    if (kline.openTime <= 0) {
      continue
    }
    const open = Number.parseFloat(kline.open)
    const high = Number.parseFloat(kline.high)
    const low = Number.parseFloat(kline.low)
    const close = Number.parseFloat(kline.close)
    const volume = Number.parseFloat(kline.volume)
    if (!isValidPrice(open) || !isValidPrice(high) || !isValidPrice(low) || !isValidPrice(close)) {
      continue
    }
    byTime.set(kline.openTime, {
      timestamp: kline.openTime,
      open,
      high,
      low,
      close,
      volume: Number.isFinite(volume) ? volume : 0,
    })
  }

  return Array.from(byTime.values()).sort((a, b) => a.timestamp - b.timestamp)
}

export function klineBarsEqual(a: KLineData, b: KLineData): boolean {
  return (
    a.timestamp === b.timestamp &&
    a.open === b.open &&
    a.high === b.high &&
    a.low === b.low &&
    a.close === b.close &&
    (a.volume ?? 0) === (b.volume ?? 0)
  )
}
