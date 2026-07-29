import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { flushActiveChartWorkspace } from '../../src/services/chartWorkspaceFlushRegistry'
import { reportError } from '../../src/services/errorService'
import { useMarketStore } from '../../src/stores/market'
import { bar, deferred } from './helpers/chartWorkspaceFixtures'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))
vi.mock('../../src/services/chartWorkspaceFlushRegistry', () => ({
  flushActiveChartWorkspace: vi.fn().mockResolvedValue(undefined),
}))
vi.mock('../../src/services/errorService', () => ({ reportError: vi.fn() }))

describe('market chart context switching', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(flushActiveChartWorkspace).mockReset().mockResolvedValue(undefined)
    vi.mocked(reportError).mockReset()
  })

  it('flushes once before atomically changing symbol and interval', async () => {
    const market = useMarketStore()
    market.activeSymbol = 'BTCUSDT'
    market.klineInterval = '1'
    market.klines = [bar(1_000)]
    const restored = [bar(2_000, 'ETHUSDT', '15')]
    vi.mocked(tauriInvoke).mockResolvedValue(restored)

    await market.setChartContext({ symbol: 'ETHUSDT', interval: '15' })

    expect(flushActiveChartWorkspace).toHaveBeenCalledTimes(1)
    expect(flushActiveChartWorkspace).toHaveBeenCalledWith('context')
    expect(tauriInvoke).toHaveBeenCalledTimes(1)
    expect(tauriInvoke).toHaveBeenCalledWith('set_chart_context', {
      symbol: 'ETHUSDT',
      interval: '15',
    })
    expect(market.activeSymbol).toBe('ETHUSDT')
    expect(market.klineInterval).toBe('15')
    expect(market.klines).toEqual(restored)
  })

  it('does nothing when the normalized key is already visible and authoritative', async () => {
    const market = useMarketStore()
    market.klines = [bar(1_000)]

    await market.setChartContext({ symbol: ' btcusdt ', interval: '1' })

    expect(flushActiveChartWorkspace).not.toHaveBeenCalled()
    expect(tauriInvoke).not.toHaveBeenCalled()
    expect(market.klines).toEqual([bar(1_000)])
  })

  it('keeps the old visible key and bars when the Rust transition fails', async () => {
    const market = useMarketStore()
    const original = [bar(1_000)]
    market.klines = original
    vi.mocked(tauriInvoke).mockRejectedValue(new Error('config rejected'))

    await expect(
      market.setChartContext({ symbol: 'ETHUSDT', interval: '15' }),
    ).rejects.toThrow('config rejected')

    expect(market.activeSymbol).toBe('BTCUSDT')
    expect(market.klineInterval).toBe('1')
    expect(market.klines).toEqual(original)
  })

  it('treats the old-key flush as best effort', async () => {
    const market = useMarketStore()
    const restored = [bar(2_000, 'ETHUSDT', '15')]
    vi.mocked(flushActiveChartWorkspace).mockRejectedValue(new Error('disk full'))
    vi.mocked(tauriInvoke).mockResolvedValue(restored)

    await expect(
      market.setChartContext({ symbol: 'ETHUSDT', interval: '15' }),
    ).resolves.toBeUndefined()

    expect(reportError).toHaveBeenCalledWith(
      expect.any(Error),
      expect.stringContaining('图表'),
    )
    expect(market.klines).toEqual(restored)
  })

  it('delegates legacy symbol and interval setters to the atomic command', async () => {
    const market = useMarketStore()
    vi.mocked(tauriInvoke)
      .mockResolvedValueOnce([bar(2_000, 'ETHUSDT', '1')])
      .mockResolvedValueOnce([bar(3_000, 'ETHUSDT', '15')])

    await market.setActiveSymbol('ETHUSDT')
    await market.setKlineInterval('15')

    expect(vi.mocked(tauriInvoke).mock.calls).toEqual([
      ['set_chart_context', { symbol: 'ETHUSDT', interval: '1' }],
      ['set_chart_context', { symbol: 'ETHUSDT', interval: '15' }],
    ])
  })

  it('serializes rapid transitions and only exposes the latest intent', async () => {
    const market = useMarketStore()
    const b = deferred<ReturnType<typeof bar>[]>()
    const c = deferred<ReturnType<typeof bar>[]>()
    vi.mocked(tauriInvoke)
      .mockReturnValueOnce(b.promise)
      .mockReturnValueOnce(c.promise)

    const toB = market.setChartContext({ symbol: 'ETHUSDT', interval: '15' })
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledTimes(1))
    const toC = market.setChartContext({ symbol: 'SOLUSDT', interval: '60' })

    expect(tauriInvoke).toHaveBeenCalledTimes(1)
    b.resolve([bar(2_000, 'ETHUSDT', '15')])
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledTimes(2))
    expect(market.activeSymbol).toBe('BTCUSDT')
    expect(market.klineInterval).toBe('1')

    c.resolve([bar(3_000, 'SOLUSDT', '60')])
    await expect(Promise.all([toB, toC])).resolves.toEqual([undefined, undefined])
    expect(market.activeSymbol).toBe('SOLUSDT')
    expect(market.klineInterval).toBe('60')
    expect(market.klines).toEqual([bar(3_000, 'SOLUSDT', '60')])
  })

  it('compensates when the latest intent returns to the original key', async () => {
    const market = useMarketStore()
    const original = [bar(1_000)]
    market.klines = original
    const b = deferred<ReturnType<typeof bar>[]>()
    const restoredA = deferred<ReturnType<typeof bar>[]>()
    vi.mocked(tauriInvoke)
      .mockReturnValueOnce(b.promise)
      .mockReturnValueOnce(restoredA.promise)

    const toB = market.setChartContext({ symbol: 'ETHUSDT', interval: '15' })
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledTimes(1))
    const backToA = market.setChartContext({ symbol: 'BTCUSDT', interval: '1' })

    b.resolve([bar(2_000, 'ETHUSDT', '15')])
    await vi.waitFor(() => {
      expect(tauriInvoke).toHaveBeenNthCalledWith(2, 'set_chart_context', {
        symbol: 'BTCUSDT',
        interval: '1',
      })
    })
    expect(market.activeSymbol).toBe('BTCUSDT')
    expect(market.klines).toEqual(original)

    restoredA.resolve([bar(3_000)])
    await expect(Promise.all([toB, backToA])).resolves.toEqual([undefined, undefined])
    expect(market.activeSymbol).toBe('BTCUSDT')
    expect(market.klineInterval).toBe('1')
    expect(market.klines).toEqual([bar(3_000)])
  })

  it('does not reject the latest intent when a superseded command fails', async () => {
    const market = useMarketStore()
    const b = deferred<ReturnType<typeof bar>[]>()
    const c = deferred<ReturnType<typeof bar>[]>()
    vi.mocked(tauriInvoke)
      .mockReturnValueOnce(b.promise)
      .mockReturnValueOnce(c.promise)

    const toB = market.setChartContext({ symbol: 'ETHUSDT', interval: '15' })
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledTimes(1))
    const toC = market.setChartContext({ symbol: 'SOLUSDT', interval: '60' })

    b.reject(new Error('superseded B failed'))
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledTimes(2))
    c.resolve([bar(3_000, 'SOLUSDT', '60')])

    await expect(Promise.all([toB, toC])).resolves.toEqual([undefined, undefined])
    expect(reportError).not.toHaveBeenCalledWith(
      expect.anything(),
      expect.stringContaining('superseded'),
    )
    expect(market.activeSymbol).toBe('SOLUSDT')
  })
})
