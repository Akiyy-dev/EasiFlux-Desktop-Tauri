import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import { useAccountStore } from '../../src/stores/account'
import { normalizeAccountId } from '../../src/utils/account'
import type { Balance } from '../../src/types/models'

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

describe('account store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('setBalance syncs summary and total equity', () => {
    const store = useAccountStore()
    const usdt: Balance = {
      asset: 'USDT',
      available: '80',
      frozen: '20',
      total: '100',
    }
    store.setBalance(usdt)

    expect(store.summary).not.toBeNull()
    expect(store.summary?.balances).toHaveLength(1)
    expect(store.summary?.totalEquity).toBe('100')

    store.setBalance({
      asset: 'USDT',
      available: '150',
      frozen: '50',
      total: '200',
    })
    expect(store.summary?.totalEquity).toBe('200')
    expect(store.balances[0]?.available).toBe('150')
  })
})
