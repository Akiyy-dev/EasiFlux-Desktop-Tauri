import type {
  ChartPreferencesV1,
  ChartViewStateV1,
  ChartWorkspaceKey,
  JsonValue,
} from '../types/chartWorkspace'
import type { Kline } from '../types/models'

export const SUPPORTED_CHART_INTERVALS = new Set(['1', '5', '15', '60', '240', 'D'])

export function normalizeChartWorkspaceKey(key: ChartWorkspaceKey): ChartWorkspaceKey {
  const rawSymbol = key.symbol.trim()
  const symbol = rawSymbol.toUpperCase()
  const interval = key.interval.trim()
  const isAscii = [...rawSymbol].every((character) => character.charCodeAt(0) <= 0x7f)
  if (!isAscii || !/^[A-Z0-9_-]{1,64}$/.test(symbol)) {
    throw new Error('Chart workspace symbol must be 1-64 uppercase ASCII letters, digits, underscores, or hyphens')
  }
  if (!SUPPORTED_CHART_INTERVALS.has(interval)) {
    throw new Error('Unsupported chart interval')
  }
  return { symbol, interval }
}

export function defaultChartViewState(key: ChartWorkspaceKey): ChartViewStateV1 {
  const normalized = normalizeChartWorkspaceKey(key)
  return {
    schemaVersion: 1,
    symbol: normalized.symbol,
    interval: normalized.interval,
    revision: 0,
    savedAtMs: 0,
    overlays: [],
    viewport: {},
  }
}

export function defaultChartPreferences(): ChartPreferencesV1 {
  return {
    schemaVersion: 1,
    revision: 0,
    savedAtMs: 0,
    mainIndicators: ['MA', 'EMA'],
    subIndicators: ['VOL', 'MACD'],
  }
}

export function toJsonValue(value: unknown): JsonValue {
  return toJsonValueInternal(value, new WeakSet<object>())
}

function toJsonValueInternal(value: unknown, ancestors: WeakSet<object>): JsonValue {
  if (value === null || typeof value === 'boolean' || typeof value === 'string') {
    return value
  }
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value : null
  }
  if (typeof value !== 'object') {
    return null
  }
  if (ancestors.has(value)) {
    return null
  }

  ancestors.add(value)
  try {
    if (Array.isArray(value)) {
      return value.map((item) => toJsonValueInternal(item, ancestors))
    }
    if (Object.getPrototypeOf(value) !== Object.prototype && Object.getPrototypeOf(value) !== null) {
      return null
    }
    const result: { [key: string]: JsonValue } = {}
    for (const name of Object.keys(value)) {
      try {
        result[name] = toJsonValueInternal((value as Record<string, unknown>)[name], ancestors)
      } catch {
        result[name] = null
      }
    }
    return result
  } catch {
    return null
  } finally {
    ancestors.delete(value)
  }
}

export function stableChartFingerprint(value: unknown): string {
  return JSON.stringify(sortJsonValue(toJsonValue(value)))
}

function sortJsonValue(value: JsonValue): JsonValue {
  if (Array.isArray(value)) {
    return value.map(sortJsonValue)
  }
  if (value !== null && typeof value === 'object') {
    return Object.keys(value).sort().reduce<{ [key: string]: JsonValue }>((sorted, key) => {
      sorted[key] = sortJsonValue(value[key])
      return sorted
    }, {})
  }
  return value
}

export function chartViewContentFingerprint(view: ChartViewStateV1): string {
  return stableChartFingerprint({
    schemaVersion: view.schemaVersion,
    symbol: view.symbol,
    interval: view.interval,
    overlays: view.overlays,
    viewport: view.viewport,
  })
}

export function chartPreferencesContentFingerprint(preferences: ChartPreferencesV1): string {
  return stableChartFingerprint({
    schemaVersion: preferences.schemaVersion,
    mainIndicators: preferences.mainIndicators,
    subIndicators: preferences.subIndicators,
  })
}

export function mergeKlinesByOpenTime(...groups: Kline[][]): Kline[] {
  const merged = new Map<number, Kline>()
  for (const group of groups) {
    for (const kline of group) {
      if (Number.isFinite(kline.openTime)) {
        merged.set(kline.openTime, kline)
      }
    }
  }
  return [...merged.values()]
    .sort((first, second) => first.openTime - second.openTime)
    .slice(-10_000)
}
