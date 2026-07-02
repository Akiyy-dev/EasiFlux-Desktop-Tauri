import { describe, expect, it } from 'vitest'
import { normalizeAccountId } from '../../src/utils/account'

describe('normalizeAccountId', () => {
  it('falls back to default for empty values', () => {
    expect(normalizeAccountId(undefined)).toBe('default')
    expect(normalizeAccountId(null)).toBe('default')
    expect(normalizeAccountId('')).toBe('default')
    expect(normalizeAccountId('   ')).toBe('default')
  })

  it('preserves non-empty ids', () => {
    expect(normalizeAccountId('main')).toBe('main')
    expect(normalizeAccountId('  alt  ')).toBe('alt')
  })
})
