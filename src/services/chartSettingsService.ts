export type ChartLayout = 'standard' | 'compact'

export interface ChartViewport {
  barSpace?: number
  rightTimestamp?: number
}

export interface ChartSettings {
  version: 1
  layout: ChartLayout
  mainIndicators: string[]
  subIndicators: string[]
  viewports: Record<string, ChartViewport>
}

const STORAGE_KEY = 'easiflux.chart-settings.v1'
const MAIN_INDICATORS = new Set(['MA', 'EMA', 'BOLL', 'SAR'])
const SUB_INDICATORS = new Set(['VOL', 'MACD', 'RSI', 'KDJ'])

export const DEFAULT_CHART_SETTINGS: ChartSettings = {
  version: 1,
  layout: 'compact',
  mainIndicators: ['MA', 'EMA'],
  subIndicators: ['VOL', 'MACD'],
  viewports: {},
}

function defaultChartSettings(): ChartSettings {
  return {
    ...DEFAULT_CHART_SETTINGS,
    mainIndicators: [...DEFAULT_CHART_SETTINGS.mainIndicators],
    subIndicators: [...DEFAULT_CHART_SETTINGS.subIndicators],
    viewports: {},
  }
}

function validIndicators(value: unknown, allowed: Set<string>, fallback: string[]): string[] {
  if (!Array.isArray(value)) return [...fallback]
  return [
    ...new Set(value.filter((item): item is string => typeof item === 'string' && allowed.has(item))),
  ]
}

function validViewport(value: unknown): ChartViewport | null {
  if (!value || typeof value !== 'object') return null
  const raw = value as Record<string, unknown>
  const viewport: ChartViewport = {}
  if (typeof raw.barSpace === 'number' && raw.barSpace >= 1 && raw.barSpace <= 50) {
    viewport.barSpace = raw.barSpace
  }
  if (typeof raw.rightTimestamp === 'number' && raw.rightTimestamp > 0) {
    viewport.rightTimestamp = raw.rightTimestamp
  }
  return Object.keys(viewport).length > 0 ? viewport : null
}

export function normalizeChartSettings(value: unknown): ChartSettings {
  if (!value || typeof value !== 'object') {
    return defaultChartSettings()
  }
  const raw = value as Record<string, unknown>
  const viewports: Record<string, ChartViewport> = {}
  if (raw.viewports && typeof raw.viewports === 'object') {
    for (const [key, viewport] of Object.entries(raw.viewports as Record<string, unknown>)) {
      const normalized = validViewport(viewport)
      if (normalized && /^[A-Z0-9_-]+:[A-Za-z0-9]+$/.test(key)) viewports[key] = normalized
    }
  }
  return {
    version: 1,
    layout: raw.layout === 'standard' ? 'standard' : 'compact',
    mainIndicators: validIndicators(
      raw.mainIndicators,
      MAIN_INDICATORS,
      DEFAULT_CHART_SETTINGS.mainIndicators,
    ),
    subIndicators: validIndicators(
      raw.subIndicators,
      SUB_INDICATORS,
      DEFAULT_CHART_SETTINGS.subIndicators,
    ),
    viewports,
  }
}

export function loadChartSettings(): ChartSettings {
  try {
    const raw = globalThis.localStorage?.getItem(STORAGE_KEY)
    return raw ? normalizeChartSettings(JSON.parse(raw)) : defaultChartSettings()
  } catch {
    return defaultChartSettings()
  }
}

export function saveChartSettings(settings: ChartSettings): void {
  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, JSON.stringify(normalizeChartSettings(settings)))
  } catch {
    // Local storage may be unavailable; chart usage must remain unaffected.
  }
}

export function chartViewportKey(symbol: string, interval: string): string {
  return `${symbol.trim().toUpperCase()}:${interval}`
}
