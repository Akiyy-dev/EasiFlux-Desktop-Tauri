import { describe, expect, it } from 'vitest'
import { normalizeEpochMs } from '../../src/utils/time'

describe('time utils', () => {
  it('normalizes second and millisecond epochs', () => {
    expect(normalizeEpochMs(1_700_000_000)).toBe(1_700_000_000_000)
    expect(normalizeEpochMs(1_700_000_000_000)).toBe(1_700_000_000_000)
    expect(normalizeEpochMs(12)).toBeNull()
  })
})
