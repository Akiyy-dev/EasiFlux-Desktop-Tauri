import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useMarketStore } from '../../src/stores/market'
import type { Ticker } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({
  tauriInvoke: vi.fn(),
}))

function ticker(fundingRate: string): Ticker {
  return {
    symbol: 'BTCUSDT',
    lastPrice: '60000',
    bidPrice: '59999',
    askPrice: '60001',
    volume24h: '100',
    change24hPct: '1',
    markPrice: '60000',
    high24h: '61000',
    low24h: '59000',
    fundingRate,
  }
}

describe('market funding rate guard', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('stores backend funding metadata for display', () => {
    const store = useMarketStore()
    store.setTicker({
      ...ticker('0.0001'),
      fundingRateUpdatedAt: 123,
      fundingRateError: '获取失败',
    })

    expect(store.ticker?.fundingRate).toBe('0.0001')
    expect(store.ticker?.fundingRateUpdatedAt).toBe(123)
    expect(store.ticker?.fundingRateError).toBe('获取失败')
  })

  it('ignores ticker events for a non-active symbol', () => {
    const store = useMarketStore()
    store.setTicker(ticker('0.0001'))
    store.setTicker({ ...ticker('0.0002'), symbol: 'ETHUSDT' })

    expect(store.ticker?.symbol).toBe('BTCUSDT')
    expect(store.ticker?.fundingRate).toBe('0.0001')
  })
})
