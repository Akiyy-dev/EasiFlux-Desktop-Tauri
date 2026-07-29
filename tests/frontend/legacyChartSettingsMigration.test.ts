import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  ChartWorkspaceKey,
  ChartWorkspaceSaveResult,
  ChartWorkspaceSnapshot,
  SaveChartWorkspaceRequest,
} from '../../src/types/chartWorkspace'
import { defaultChartPreferences, defaultChartViewState } from '../../src/utils/chartWorkspace'

const mocks = vi.hoisted(() => ({
  load: vi.fn<(key: ChartWorkspaceKey) => Promise<ChartWorkspaceSnapshot>>(),
  save: vi.fn<(request: SaveChartWorkspaceRequest) => Promise<ChartWorkspaceSaveResult>>(),
  report: vi.fn(),
}))

vi.mock('../../src/services/chartWorkspaceService', () => ({
  loadChartWorkspace: mocks.load,
  saveChartWorkspace: mocks.save,
}))

vi.mock('../../src/services/errorService', () => ({ reportError: mocks.report }))

const legacyKey = 'easiflux.chart-settings.v1'
const fallbackKey: ChartWorkspaceKey = { symbol: 'BTCUSDT', interval: '15' }

function snapshot(
  key: ChartWorkspaceKey,
  viewRevision = 0,
  preferencesRevision = 0,
): ChartWorkspaceSnapshot {
  return {
    key,
    klines: [],
    viewState: { ...defaultChartViewState(key), revision: viewRevision },
    preferences: { ...defaultChartPreferences(), revision: preferencesRevision },
  }
}

function saved(
  key: ChartWorkspaceKey,
  request: SaveChartWorkspaceRequest,
  patch: Partial<ChartWorkspaceSaveResult> = {},
): ChartWorkspaceSaveResult {
  return {
    key,
    viewRevision: request.viewState?.revision ?? 0,
    preferencesRevision: request.preferences?.revision ?? 0,
    savedAtMs: 10,
    klineSaved: false,
    viewStateSaved: request.viewState !== null,
    preferencesSaved: request.preferences !== null,
    klineError: 'unrelated kline failure',
    viewStateError: null,
    preferencesError: null,
    ...patch,
  }
}

async function migrate(): Promise<void> {
  const { migrateLegacyChartSettingsOnce } = await import(
    '../../src/services/legacyChartSettingsMigration'
  )
  await migrateLegacyChartSettingsOnce(fallbackKey)
}

describe('legacy chart settings migration', () => {
  beforeEach(() => {
    vi.resetModules()
    localStorage.clear()
    mocks.load.mockReset()
    mocks.save.mockReset()
    mocks.report.mockReset()
    mocks.load.mockImplementation(async (key) => snapshot(key))
    mocks.save.mockImplementation(async (request) => saved(request.key, request))
  })

  it('migrates_legacy_preferences_and_every_safe_viewport_before_first_chart_load', async () => {
    localStorage.setItem(legacyKey, JSON.stringify({
      version: 1,
      layout: 'standard',
      mainIndicators: ['EMA', 'UNKNOWN', 'EMA'],
      subIndicators: ['RSI', 'VOL', 2],
      viewports: {
        'BTCUSDT:15': { barSpace: 12, rightTimestamp: 1_700_000_000_000 },
        'ETH-USDT:60': { barSpace: 7 },
        '../BAD:15': { barSpace: 8 },
        'SOLUSDT:2': { barSpace: 9 },
      },
    }))
    const remove = vi.spyOn(Storage.prototype, 'removeItem')

    await migrate()

    expect(mocks.load.mock.calls.map(([key]) => key)).toEqual([
      { symbol: 'BTCUSDT', interval: '15' },
      { symbol: 'ETH-USDT', interval: '60' },
    ])
    expect(mocks.save.mock.calls.map(([request]) => request)).toEqual([
      {
        key: { symbol: 'BTCUSDT', interval: '15' },
        viewState: expect.objectContaining({
          revision: 1,
          overlays: [],
          viewport: { barSpace: 12, rightTimestamp: 1_700_000_000_000 },
        }),
        preferences: expect.objectContaining({
          revision: 1,
          mainIndicators: ['EMA'],
          subIndicators: ['RSI', 'VOL'],
        }),
      },
      {
        key: { symbol: 'ETH-USDT', interval: '60' },
        viewState: expect.objectContaining({ revision: 1, viewport: { barSpace: 7 } }),
        preferences: null,
      },
    ])
    expect(remove).toHaveBeenCalledExactlyOnceWith(legacyKey)
    expect(localStorage.getItem(legacyKey)).toBeNull()
    remove.mockRestore()
  })

  it('does_not_overwrite_a_nonzero_rust_revision', async () => {
    localStorage.setItem(legacyKey, JSON.stringify({
      version: 1,
      mainIndicators: ['BOLL'],
      subIndicators: ['KDJ'],
      viewports: { 'BTCUSDT:15': { barSpace: 14 } },
    }))
    mocks.load.mockResolvedValueOnce(snapshot(fallbackKey, 8, 5))

    await migrate()

    expect(mocks.save).not.toHaveBeenCalled()
    expect(localStorage.getItem(legacyKey)).toBeNull()
  })

  it('keeps_the_legacy_key_after_any_failed_view_or_preference_part', async () => {
    localStorage.setItem(legacyKey, JSON.stringify({
      version: 1,
      mainIndicators: ['BOLL'],
      subIndicators: ['KDJ'],
      viewports: {
        'BTCUSDT:15': { barSpace: 14 },
        'ETHUSDT:60': { rightTimestamp: 9_000 },
      },
    }))
    mocks.save.mockImplementation(async (request) => saved(request.key, request, {
      viewStateSaved: request.key.symbol !== 'ETHUSDT',
      viewStateError: request.key.symbol === 'ETHUSDT' ? 'disk full' : null,
    }))

    await migrate()

    expect(mocks.save).toHaveBeenCalledTimes(2)
    expect(localStorage.getItem(legacyKey)).not.toBeNull()
    expect(mocks.report).toHaveBeenCalledOnce()
  })

  it('retains_the_legacy_key_when_preferences_are_not_durable', async () => {
    localStorage.setItem(legacyKey, JSON.stringify({
      version: 1,
      mainIndicators: ['BOLL'],
      subIndicators: ['KDJ'],
      viewports: {},
    }))
    mocks.save.mockImplementation(async (request) => saved(request.key, request, {
      preferencesSaved: false,
      preferencesRevision: 0,
      preferencesError: 'revision conflict',
    }))

    await migrate()

    expect(mocks.save).toHaveBeenCalledWith({
      key: fallbackKey,
      viewState: null,
      preferences: expect.objectContaining({ revision: 1 }),
    })
    expect(localStorage.getItem(legacyKey)).not.toBeNull()
    expect(mocks.report).toHaveBeenCalledOnce()
  })

  it('removes_the_legacy_key_only_after_all_targets_are_durable', async () => {
    localStorage.setItem(legacyKey, JSON.stringify({
      version: 1,
      mainIndicators: ['MA'],
      subIndicators: ['MACD'],
      viewports: { 'ETHUSDT:60': { barSpace: 6 } },
    }))
    const remove = vi.spyOn(Storage.prototype, 'removeItem')
    const firstLoad = snapshot(fallbackKey, 0, 4)
    mocks.load.mockImplementation(async (key) => (
      key.symbol === fallbackKey.symbol ? firstLoad : snapshot(key)
    ))

    await Promise.all([migrate(), migrate()])

    expect(mocks.load).toHaveBeenCalledTimes(2)
    expect(mocks.save).toHaveBeenCalledExactlyOnceWith({
      key: { symbol: 'ETHUSDT', interval: '60' },
      viewState: expect.objectContaining({ revision: 1, viewport: { barSpace: 6 } }),
      preferences: null,
    })
    expect(remove).toHaveBeenCalledExactlyOnceWith(legacyKey)
    remove.mockRestore()
  })

  it('malformed_legacy_json_falls_back_without_blocking_chart_startup', async () => {
    localStorage.setItem(legacyKey, '{nope')

    await expect(migrate()).resolves.toBeUndefined()

    expect(mocks.load).not.toHaveBeenCalled()
    expect(mocks.save).not.toHaveBeenCalled()
    expect(localStorage.getItem(legacyKey)).toBe('{nope')
    expect(mocks.report).toHaveBeenCalledOnce()
  })
})
