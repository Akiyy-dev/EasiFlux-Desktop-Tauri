import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  getTimeSnapshot,
  normalizeTimeSnapshot,
  onTimeUpdated,
  serverNowMs,
  startTimeService,
  stopTimeService,
  timeServiceRunning,
} from '../../src/services/timeService'

vi.mock('../../src/composables/useTauriCommand', () => ({
  tauriInvoke: vi.fn(),
}))

import { tauriInvoke } from '../../src/composables/useTauriCommand'

describe('timeService', () => {
  beforeEach(() => {
    vi.mocked(tauriInvoke).mockReset()
    stopTimeService()
    onTimeUpdated({
      serverTimeMs: 1_700_000_000_000,
      localTimeMs: 1_700_000_000_100,
      offsetMs: -100,
      syncStatus: 'synced',
      source: 'server',
      lastSyncAt: 1_700_000_000_100,
      lastAttemptAt: 1_700_000_000_100,
      lastError: null,
    })
  })

  afterEach(() => {
    stopTimeService()
  })

  it('normalizes missing snapshot fields', () => {
    const snapshot = normalizeTimeSnapshot({})
    expect(snapshot.syncStatus).toBe('localFallback')
    expect(snapshot.source).toBe('local')
  })

  it('advances serverNow from calibrated anchor', () => {
    const before = serverNowMs()
    onTimeUpdated({
      ...getTimeSnapshot(),
      serverTimeMs: before + 5_000,
    })
    expect(serverNowMs()).toBeGreaterThanOrEqual(before + 4_900)
  })

  it('start is idempotent and does not create duplicate timers', () => {
    vi.mocked(tauriInvoke).mockResolvedValue(getTimeSnapshot())
    startTimeService()
    startTimeService()
    expect(timeServiceRunning()).toBe(true)
  })
})
