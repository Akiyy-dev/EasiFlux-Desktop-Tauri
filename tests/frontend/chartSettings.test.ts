import { describe, expect, it } from 'vitest'
import {
  chartViewportKey,
  DEFAULT_CHART_SETTINGS,
  normalizeChartSettings,
} from '../../src/services/chartSettingsService'

describe('chart settings', () => {
  it('falls back safely when persisted settings are invalid', () => {
    const settings = normalizeChartSettings({
      layout: 'broken',
      mainIndicators: ['MA', 'UNKNOWN'],
      subIndicators: null,
      viewports: {
        'ETHUSDT:1m': { barSpace: 500, rightTimestamp: -1 },
      },
    })

    expect(settings.layout).toBe('compact')
    expect(settings.mainIndicators).toEqual(['MA'])
    expect(settings.subIndicators).toEqual(DEFAULT_CHART_SETTINGS.subIndicators)
    expect(settings.viewports).toEqual({})
  })

  it('keeps valid per-symbol viewport state', () => {
    const settings = normalizeChartSettings({
      layout: 'standard',
      mainIndicators: ['EMA'],
      subIndicators: ['RSI'],
      viewports: {
        'ETHUSDT:15m': { barSpace: 12, rightTimestamp: 1_700_000_000_000 },
      },
    })

    expect(chartViewportKey('ethusdt', '15m')).toBe('ETHUSDT:15m')
    expect(settings.viewports['ETHUSDT:15m']).toEqual({
      barSpace: 12,
      rightTimestamp: 1_700_000_000_000,
    })
  })
})
