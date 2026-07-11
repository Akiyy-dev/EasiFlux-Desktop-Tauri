import { describe, expect, it } from 'vitest'
import { applyDailyPnlSnapshot } from '../../src/services/dailyPnlService'
import type { DailyPnlSnapshot } from '../../src/types/models'

describe('dailyPnlService bridge', () => {
  it('passes through backend snapshot payloads', () => {
    const snapshot: DailyPnlSnapshot = {
      value: '12.5000',
      serverTime: 1_700_000_000_000,
      updatedAt: 1_700_000_000_000,
      recordCount: 2,
      dayStart: 1_699_948_800_000,
      dayEnd: 1_700_035_200_000,
      timezone: 'Asia/Shanghai',
    }
    expect(applyDailyPnlSnapshot(snapshot)).toEqual(snapshot)
  })
})
