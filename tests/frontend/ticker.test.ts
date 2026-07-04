import { describe, expect, it } from 'vitest'
import { change24hPctValue, formatChange24hPct } from '../../src/utils/ticker'

describe('formatChange24hPct', () => {
  it('converts ratio values to percentage display', () => {
    expect(formatChange24hPct('0.034')).toBe('+3.40%')
    expect(formatChange24hPct('-0.012')).toBe('-1.20%')
  })

  it('keeps percentage-like values as-is', () => {
    expect(formatChange24hPct('5.2')).toBe('+5.20%')
    expect(formatChange24hPct('-2.35')).toBe('-2.35%')
  })

  it('handles zero and invalid input', () => {
    expect(formatChange24hPct('0')).toBe('0.00%')
    expect(formatChange24hPct(undefined)).toBe('0.00%')
    expect(formatChange24hPct('not-a-number')).toBe('0.00%')
  })
})

describe('change24hPctValue', () => {
  it('normalizes ratio and percentage inputs', () => {
    expect(change24hPctValue('0.034')).toBeCloseTo(3.4)
    expect(change24hPctValue('5.2')).toBeCloseTo(5.2)
  })
})
