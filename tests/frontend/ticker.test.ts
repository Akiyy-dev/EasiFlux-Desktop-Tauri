import { describe, expect, it } from 'vitest'
import { change24hPctValue, formatChange24hPct } from '../../src/utils/ticker'

describe('formatChange24hPct', () => {
  it('formats normalized decimal percent values', () => {
    expect(formatChange24hPct('0.1829')).toBe('+0.18%')
    expect(formatChange24hPct('-1.7421')).toBe('-1.74%')
    expect(formatChange24hPct('5.2')).toBe('+5.20%')
  })

  it('handles zero and invalid input', () => {
    expect(formatChange24hPct('0')).toBe('0.00%')
    expect(formatChange24hPct(undefined)).toBe('0.00%')
    expect(formatChange24hPct('not-a-number')).toBe('0.00%')
  })
})

describe('change24hPctValue', () => {
  it('returns parsed percent values', () => {
    expect(change24hPctValue('0.1829')).toBeCloseTo(0.1829)
    expect(change24hPctValue('5.2')).toBeCloseTo(5.2)
  })
})
