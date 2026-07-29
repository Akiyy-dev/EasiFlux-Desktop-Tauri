import { describe, expect, it } from 'vitest'
import {
  chartPreferencesContentFingerprint,
  chartViewContentFingerprint,
  defaultChartPreferences,
  defaultChartViewState,
  normalizeChartWorkspaceKey,
  stableChartFingerprint,
} from '../../src/utils/chartWorkspace'
import { segment } from './helpers/chartWorkspaceFixtures'

describe('chart workspace contract', () => {
  it('normalizes safe keys and rejects unsupported values', () => {
    expect(normalizeChartWorkspaceKey({ symbol: ' btcusdt ', interval: '15' }))
      .toEqual({ symbol: 'BTCUSDT', interval: '15' })
    expect(() => normalizeChartWorkspaceKey({ symbol: '../BTC', interval: '15' })).toThrow()
    expect(() => normalizeChartWorkspaceKey({ symbol: 'ß', interval: '15' })).toThrow()
    expect(() => normalizeChartWorkspaceKey({ symbol: 'BTCUSDT', interval: '2' })).toThrow()
  })

  it('creates independent version-one defaults', () => {
    const first = defaultChartViewState({ symbol: 'BTCUSDT', interval: '15' })
    const second = defaultChartViewState({ symbol: 'BTCUSDT', interval: '15' })
    first.overlays.push(segment())
    expect(second.overlays).toEqual([])
    expect(defaultChartPreferences().mainIndicators).toEqual(['MA', 'EMA'])
  })

  it('fingerprints normalized objects independently of object key order', () => {
    expect(stableChartFingerprint({ b: 2, a: 1 }))
      .toBe(stableChartFingerprint({ a: 1, b: 2 }))
  })

  it('excludes persistence metadata from content fingerprints', () => {
    const firstView = { ...defaultChartViewState({ symbol: 'BTCUSDT', interval: '15' }), revision: 1, savedAtMs: 10 }
    const secondView = { ...firstView, revision: 8, savedAtMs: 99 }
    const firstPreferences = { ...defaultChartPreferences(), revision: 2, savedAtMs: 20 }
    const secondPreferences = { ...firstPreferences, revision: 9, savedAtMs: 100 }
    expect(chartViewContentFingerprint(firstView)).toBe(chartViewContentFingerprint(secondView))
    expect(chartPreferencesContentFingerprint(firstPreferences))
      .toBe(chartPreferencesContentFingerprint(secondPreferences))
  })
})
