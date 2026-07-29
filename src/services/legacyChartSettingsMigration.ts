import type { ChartWorkspaceKey } from '../types/chartWorkspace'
import type {
  ChartPreferencesV1,
  ChartViewportSnapshot,
  ChartWorkspaceSnapshot,
  SaveChartWorkspaceRequest,
} from '../types/chartWorkspace'
import { defaultChartPreferences, normalizeChartWorkspaceKey } from '../utils/chartWorkspace'
import { loadChartWorkspace, saveChartWorkspace } from './chartWorkspaceService'
import { reportError } from './errorService'

const LEGACY_STORAGE_KEY = 'easiflux.chart-settings.v1'
const MAIN_INDICATORS = new Set(['MA', 'EMA', 'BOLL', 'SAR'])
const SUB_INDICATORS = new Set(['VOL', 'MACD', 'RSI', 'KDJ'])

interface LegacyChartSettings {
  preferences: ChartPreferencesV1
  viewports: Map<string, { key: ChartWorkspaceKey; viewport: ChartViewportSnapshot }>
}

let migrationPromise: Promise<void> | null = null

function normalizedIndicators(
  value: unknown,
  allowed: ReadonlySet<string>,
  fallback: string[],
): string[] {
  if (!Array.isArray(value)) return [...fallback]
  return [...new Set(value.filter((item): item is string => (
    typeof item === 'string' && allowed.has(item)
  )))]
}

function normalizedViewport(value: unknown): ChartViewportSnapshot | null {
  if (!value || typeof value !== 'object') return null
  const raw = value as Record<string, unknown>
  const viewport: ChartViewportSnapshot = {}
  if (typeof raw.barSpace === 'number' && raw.barSpace >= 1 && raw.barSpace <= 50) {
    viewport.barSpace = raw.barSpace
  }
  if (typeof raw.rightTimestamp === 'number' && raw.rightTimestamp > 0) {
    viewport.rightTimestamp = raw.rightTimestamp
  }
  return Object.keys(viewport).length > 0 ? viewport : null
}

function parseLegacySettings(rawJson: string): LegacyChartSettings {
  const parsed: unknown = JSON.parse(rawJson)
  if (!parsed || typeof parsed !== 'object') {
    throw new Error('Legacy chart settings must be an object')
  }
  const raw = parsed as Record<string, unknown>
  const defaults = defaultChartPreferences()
  const preferences: ChartPreferencesV1 = {
    ...defaults,
    mainIndicators: normalizedIndicators(
      raw.mainIndicators,
      MAIN_INDICATORS,
      defaults.mainIndicators,
    ),
    subIndicators: normalizedIndicators(
      raw.subIndicators,
      SUB_INDICATORS,
      defaults.subIndicators,
    ),
  }
  const viewports = new Map<string, {
    key: ChartWorkspaceKey
    viewport: ChartViewportSnapshot
  }>()
  if (raw.viewports && typeof raw.viewports === 'object') {
    for (const [rawKey, rawViewport] of Object.entries(
      raw.viewports as Record<string, unknown>,
    )) {
      const separator = rawKey.lastIndexOf(':')
      if (separator <= 0 || separator === rawKey.length - 1) continue
      try {
        const key = normalizeChartWorkspaceKey({
          symbol: rawKey.slice(0, separator),
          interval: rawKey.slice(separator + 1),
        })
        const viewport = normalizedViewport(rawViewport)
        if (viewport) viewports.set(`${key.symbol}:${key.interval}`, { key, viewport })
      } catch {
        // Unsupported or unsafe legacy views are intentionally ignored.
      }
    }
  }
  return { preferences, viewports }
}

function migratedRequest(
  snapshot: ChartWorkspaceSnapshot,
  viewport: ChartViewportSnapshot | null,
  preferences: ChartPreferencesV1 | null,
): SaveChartWorkspaceRequest {
  const now = Date.now()
  return {
    key: snapshot.key,
    viewState: viewport && snapshot.viewState.revision === 0
      ? {
        ...snapshot.viewState,
        schemaVersion: 1,
        symbol: snapshot.key.symbol,
        interval: snapshot.key.interval,
        revision: 1,
        savedAtMs: now,
        viewport,
      }
      : null,
    preferences: preferences && snapshot.preferences.revision === 0
      ? { ...preferences, schemaVersion: 1, revision: 1, savedAtMs: now }
      : null,
  }
}

async function runMigration(fallbackKey: ChartWorkspaceKey): Promise<void> {
  const errors: unknown[] = []
  let rawJson: string | null
  try {
    rawJson = globalThis.localStorage?.getItem(LEGACY_STORAGE_KEY) ?? null
  } catch (error) {
    reportError(error, '读取旧图表设置失败')
    return
  }
  if (rawJson === null) return

  let legacy: LegacyChartSettings
  try {
    legacy = parseLegacySettings(rawJson)
  } catch (error) {
    reportError(error, '迁移旧图表设置失败')
    return
  }

  const normalizedFallback = normalizeChartWorkspaceKey(fallbackKey)
  const fallbackId = `${normalizedFallback.symbol}:${normalizedFallback.interval}`
  const targets = new Map(legacy.viewports)
  const fallbackViewport = targets.get(fallbackId)
  targets.delete(fallbackId)
  const orderedTargets = [
    { key: normalizedFallback, viewport: fallbackViewport?.viewport ?? null, preferences: true },
    ...[...targets.values()].map((target) => ({
      key: target.key,
      viewport: target.viewport,
      preferences: false,
    })),
  ]

  for (const target of orderedTargets) {
    let snapshot: ChartWorkspaceSnapshot
    try {
      snapshot = await loadChartWorkspace(target.key)
    } catch (error) {
      errors.push(error)
      continue
    }
    const request = migratedRequest(
      snapshot,
      target.viewport,
      target.preferences ? legacy.preferences : null,
    )
    if (!request.viewState && !request.preferences) continue

    try {
      const result = await saveChartWorkspace(request)
      if (request.viewState
        && (!result.viewStateSaved || result.viewRevision < request.viewState.revision)) {
        errors.push(new Error(result.viewStateError ?? 'Legacy chart view was not durable'))
      }
      if (request.preferences
        && (!result.preferencesSaved
          || result.preferencesRevision < request.preferences.revision)) {
        errors.push(new Error(
          result.preferencesError ?? 'Legacy chart preferences were not durable',
        ))
      }
    } catch (error) {
      errors.push(error)
    }
  }

  if (errors.length > 0) {
    const detail = errors.map((error) => (
      error instanceof Error ? error.message : String(error)
    )).join('; ')
    reportError(
      new Error(`One or more legacy chart settings could not be persisted: ${detail}`),
      '迁移旧图表设置失败',
    )
    return
  }

  try {
    globalThis.localStorage?.removeItem(LEGACY_STORAGE_KEY)
  } catch (error) {
    reportError(error, '清理旧图表设置失败')
  }
}

export function migrateLegacyChartSettingsOnce(
  fallbackKey: ChartWorkspaceKey,
): Promise<void> {
  migrationPromise ??= runMigration(fallbackKey).catch((error: unknown) => {
    reportError(error, '迁移旧图表设置失败')
  })
  return migrationPromise
}
