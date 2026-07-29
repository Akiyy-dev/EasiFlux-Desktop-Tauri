import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { KLineData } from 'klinecharts'
import type { Period, SymbolInfo } from '@klinecharts/pro'
import { EasiKlineDatafeed } from '../../src/composables/easiKlineDatafeed'
import {
  loadChartWorkspace,
  refreshChartKlines,
} from '../../src/services/chartWorkspaceService'
import type { ChartWorkspaceKey } from '../../src/types/chartWorkspace'
import {
  bar,
  deferred,
  key,
  sampleSnapshot,
} from './helpers/chartWorkspaceFixtures'

vi.mock('../../src/services/chartWorkspaceService', () => ({
  loadChartWorkspace: vi.fn(),
  refreshChartKlines: vi.fn(),
}))

const symbol = (ticker = 'BTCUSDT'): SymbolInfo => ({
  ticker,
  shortName: ticker,
  name: ticker,
  exchange: 'EasiCoin',
  market: 'futures',
  priceCurrency: 'USDT',
  type: 'crypto',
})

const period = (interval = '1'): Period => {
  if (interval === '15') {
    return { multiplier: 15, timespan: 'minute', text: '15m' }
  }
  if (interval === '60') {
    return { multiplier: 1, timespan: 'hour', text: '1h' }
  }
  return { multiplier: 1, timespan: 'minute', text: '1m' }
}

interface DatafeedHarnessOptions {
  context?: ChartWorkspaceKey
  onContextChange?: ReturnType<typeof vi.fn<(next: ChartWorkspaceKey) => Promise<void>>>
  onRefresh?: ReturnType<typeof vi.fn>
  report?: ReturnType<typeof vi.fn>
}

function makeDatafeed(options: DatafeedHarnessOptions = {}) {
  let currentContext = options.context ?? key()
  const onContextChange = options.onContextChange ?? vi.fn().mockResolvedValue(undefined)
  const onRefresh = options.onRefresh ?? vi.fn()
  const report = options.report ?? vi.fn()
  const datafeed = new EasiKlineDatafeed(
    () => ['BTCUSDT', 'ETHUSDT', 'SOLUSDT'],
    () => currentContext,
    async (next) => {
      await onContextChange(next)
      currentContext = next
    },
    onRefresh,
    report,
  )
  return { datafeed, onContextChange, onRefresh, report }
}

describe('EasiKlineDatafeed history', () => {
  beforeEach(() => {
    vi.mocked(loadChartWorkspace).mockReset()
    vi.mocked(refreshChartKlines).mockReset()
  })

  it('syncs both Pro-selected key fields and reads the requested range', async () => {
    const onContextChange = vi.fn().mockResolvedValue(undefined)
    const onRefresh = vi.fn()
    const { datafeed } = makeDatafeed({ onContextChange, onRefresh })
    vi.mocked(loadChartWorkspace).mockResolvedValue(sampleSnapshot(
      [bar(1_000, 'ETHUSDT', '15')],
      key('ETHUSDT', '15'),
    ))
    vi.mocked(refreshChartKlines).mockResolvedValue([])

    const result = await datafeed.getHistoryKLineData(
      symbol('ETHUSDT'),
      period('15'),
      1_000,
      2_000,
    )

    expect(onContextChange).toHaveBeenCalledWith({ symbol: 'ETHUSDT', interval: '15' })
    expect(loadChartWorkspace).toHaveBeenCalledWith(
      { symbol: 'ETHUSDT', interval: '15' },
      { from: 1_000, to: 2_000, limit: 500 },
    )
    expect(refreshChartKlines).toHaveBeenCalledWith(
      { symbol: 'ETHUSDT', interval: '15' },
      { from: 1_000, to: 2_000, limit: 500 },
    )
    expect(result.map((item) => item.timestamp)).toEqual([1_000])
  })

  it('returns primed local history before the REST refresh settles', async () => {
    const rest = deferred<ReturnType<typeof bar>[]>()
    vi.mocked(refreshChartKlines).mockReturnValue(rest.promise)
    const { datafeed } = makeDatafeed()
    datafeed.primeHistory(key(), 7, [bar(1_000)])

    const result = await datafeed.getHistoryKLineData(symbol(), period(), 1_000, 2_000)

    expect(result.map((item) => item.timestamp)).toEqual([1_000])
    expect(rest.settled).toBe(false)
    expect(loadChartWorkspace).not.toHaveBeenCalled()
    rest.resolve([])
  })

  it('loads an uncovered local range instead of reusing unrelated primed bars', async () => {
    const { datafeed } = makeDatafeed()
    datafeed.primeHistory(key(), 7, [bar(1_000)])
    vi.mocked(loadChartWorkspace).mockResolvedValue(sampleSnapshot([bar(3_000)]))
    vi.mocked(refreshChartKlines).mockResolvedValue([])

    const result = await datafeed.getHistoryKLineData(symbol(), period(), 3_000, 4_000)

    expect(loadChartWorkspace).toHaveBeenCalledWith(key(), {
      from: 3_000,
      to: 4_000,
      limit: 500,
    })
    expect(result.map((item) => item.timestamp)).toEqual([3_000])
  })

  it('awaits REST as the initial Pro payload when no local bars exist', async () => {
    vi.mocked(loadChartWorkspace).mockResolvedValue(sampleSnapshot([], key()))
    vi.mocked(refreshChartKlines).mockResolvedValue([bar(2_000)])

    const result = await makeDatafeed().datafeed.getHistoryKLineData(
      symbol(),
      period(),
      1_000,
      2_000,
    )

    expect(result.map((item) => item.timestamp)).toEqual([2_000])
  })

  it('suppresses an intermediate key during programmatic symbol-period sync', async () => {
    const onContextChange = vi.fn().mockResolvedValue(undefined)
    const { datafeed } = makeDatafeed({ onContextChange })
    datafeed.primeHistory(key(), 7, [bar(1_000)])
    datafeed.expectProgrammaticContext(key('ETHUSDT', '15'), 8)

    const result = await datafeed.getHistoryKLineData(
      symbol('ETHUSDT'),
      period('1'),
      1_000,
      2_000,
    )

    expect(onContextChange).not.toHaveBeenCalled()
    expect(result.map((item) => item.timestamp)).toEqual([1_000])
    expect(loadChartWorkspace).not.toHaveBeenCalled()
    expect(refreshChartKlines).not.toHaveBeenCalled()
  })

  it('resolves the matching expected-context waiter only at the final key', async () => {
    const { datafeed } = makeDatafeed({ context: key('ETHUSDT', '15') })
    datafeed.primeHistory(key('ETHUSDT', '15'), 8, [bar(2_000, 'ETHUSDT', '15')])
    datafeed.expectProgrammaticContext(key('ETHUSDT', '15'), 8)
    const waiter = datafeed.waitForExpectedContext(key('ETHUSDT', '15'), 8, 1_000)
    vi.mocked(refreshChartKlines).mockResolvedValue([])

    await datafeed.getHistoryKLineData(symbol('ETHUSDT'), period('15'), 1_000, 3_000)

    await expect(waiter).resolves.toBe(true)
  })

  it('reports the same offline REST refresh only once after local success', async () => {
    const report = vi.fn()
    const { datafeed } = makeDatafeed({ report })
    datafeed.primeHistory(key(), 7, [bar(1_000)])
    vi.mocked(refreshChartKlines).mockRejectedValue(new Error('offline'))

    await datafeed.getHistoryKLineData(symbol(), period(), 1_000, 2_000)
    await datafeed.getHistoryKLineData(symbol(), period(), 1_000, 2_000)
    await vi.waitFor(() => expect(report).toHaveBeenCalledTimes(1))

    expect(report).toHaveBeenCalledWith(expect.any(String), expect.any(Error))
  })

  it('emits the key, generation, range, and bars after a guarded REST refresh', async () => {
    const rest = deferred<ReturnType<typeof bar>[]>()
    const onRefresh = vi.fn()
    const { datafeed } = makeDatafeed({ onRefresh })
    datafeed.primeHistory(key(), 7, [bar(1_000)])
    vi.mocked(refreshChartKlines).mockReturnValue(rest.promise)

    await datafeed.getHistoryKLineData(symbol(), period(), 1_000, 2_000)
    rest.resolve([bar(2_000)])

    await vi.waitFor(() => {
      expect(onRefresh).toHaveBeenCalledWith({
        key: key(),
        generation: 1,
        range: { from: 1_000, to: 2_000, limit: 500 },
        klines: [{
          timestamp: 2_000,
          open: 1,
          high: 2,
          low: 0.5,
          close: 1.5,
          volume: 10,
        }],
      })
    })
  })

  it('emits_and_merges_concurrent_ranges_from_the_same_key_epoch', async () => {
    const firstRange = deferred<ReturnType<typeof bar>[]>()
    const secondRange = deferred<ReturnType<typeof bar>[]>()
    const merged = new Map<number, KLineData>()
    const onRefresh = vi.fn((result: { klines: KLineData[] }) => {
      for (const item of result.klines) merged.set(item.timestamp, item)
    })
    const { datafeed } = makeDatafeed({ onRefresh })
    datafeed.primeHistory(key(), 7, [bar(1_000), bar(3_000)])
    vi.mocked(refreshChartKlines)
      .mockReturnValueOnce(firstRange.promise)
      .mockReturnValueOnce(secondRange.promise)

    await datafeed.getHistoryKLineData(symbol(), period(), 1_000, 2_000)
    await datafeed.getHistoryKLineData(symbol(), period(), 3_000, 4_000)
    secondRange.resolve([bar(3_500)])
    firstRange.resolve([bar(1_500)])

    await vi.waitFor(() => expect(onRefresh).toHaveBeenCalledTimes(2))
    expect(onRefresh.mock.calls.map(([result]) => result.range)).toEqual([
      { from: 3_000, to: 4_000, limit: 500 },
      { from: 1_000, to: 2_000, limit: 500 },
    ])
    expect([...merged.keys()].sort((first, second) => first - second)).toEqual([1_500, 3_500])
  })

  it('buffers an already-settled REST refresh until after local history is delivered', async () => {
    const events: string[] = []
    const onRefresh = vi.fn(() => events.push('refresh'))
    const { datafeed } = makeDatafeed({ onRefresh })
    datafeed.primeHistory(key(), 7, [bar(1_000)])
    vi.mocked(refreshChartKlines).mockResolvedValue([bar(2_000)])

    await datafeed.getHistoryKLineData(symbol(), period(), 1_000, 2_000)
    events.push('ready')
    await vi.waitFor(() => expect(onRefresh).toHaveBeenCalledTimes(1))

    expect(events).toEqual(['ready', 'refresh'])
  })

  it('returns current B prime when a stale A local read finishes', async () => {
    const localA = deferred<ReturnType<typeof sampleSnapshot>>()
    const onRefresh = vi.fn()
    const { datafeed } = makeDatafeed({ context: key('ETHUSDT', '15'), onRefresh })
    vi.mocked(loadChartWorkspace).mockReturnValue(localA.promise)

    const requestA = datafeed.getHistoryKLineData(
      symbol('ETHUSDT'),
      period('15'),
      1_000,
      3_000,
    )
    await vi.waitFor(() => expect(loadChartWorkspace).toHaveBeenCalledTimes(1))
    datafeed.primeHistory(key('SOLUSDT', '60'), 9, [bar(2_000, 'SOLUSDT', '60')])
    localA.resolve(sampleSnapshot([bar(1_000, 'ETHUSDT', '15')], key('ETHUSDT', '15')))

    const result = await requestA
    expect(result.map((item) => item.timestamp)).toEqual([2_000])
    expect(refreshChartKlines).not.toHaveBeenCalled()
    expect(onRefresh).not.toHaveBeenCalled()
  })
})

describe('EasiKlineDatafeed realtime subscription', () => {
  beforeEach(() => {
    vi.mocked(loadChartWorkspace).mockReset()
    vi.mocked(refreshChartKlines).mockReset()
  })

  it('deduplicates only identical realtime updates', () => {
    const { datafeed } = makeDatafeed()
    const callback = vi.fn()
    const first: KLineData = {
      timestamp: 1_000,
      open: 1,
      high: 2,
      low: 0.5,
      close: 1.5,
      volume: 10,
    }
    const changed = { ...first, close: 1.75 }
    datafeed.subscribe(symbol(), period(), callback)

    datafeed.pushRealtimeBar(key(), first)
    datafeed.pushRealtimeBar(key(), { ...first })
    datafeed.pushRealtimeBar(key(), changed)

    expect(callback.mock.calls.map(([item]) => item)).toEqual([first, changed])
  })

  it('ignores realtime bars from another subscription key', () => {
    const { datafeed } = makeDatafeed()
    const callback = vi.fn()
    datafeed.subscribe(symbol(), period(), callback)

    datafeed.pushRealtimeBar(key('ETHUSDT', '15'), {
      timestamp: 1_000,
      open: 1,
      high: 2,
      low: 0.5,
      close: 1.5,
      volume: 10,
    })

    expect(callback).not.toHaveBeenCalled()
  })

  it('keeps the current subscription when a stale key unsubscribes', () => {
    const { datafeed } = makeDatafeed()
    const callback = vi.fn()
    const item: KLineData = {
      timestamp: 1_000,
      open: 1,
      high: 2,
      low: 0.5,
      close: 1.5,
      volume: 10,
    }
    datafeed.subscribe(symbol('ETHUSDT'), period('15'), callback)
    datafeed.unsubscribe(symbol(), period())

    datafeed.pushRealtimeBar(key('ETHUSDT', '15'), item)

    expect(callback).toHaveBeenCalledWith(item)
  })
})
