import type { KLineData } from 'klinecharts'
import type {
  Datafeed,
  DatafeedSubscribeCallback,
  Period,
  SymbolInfo,
} from '@klinecharts/pro'
import type { ChartWorkspaceKey } from '../types/chartWorkspace'
import type { Kline } from '../types/models'
import {
  loadChartWorkspace,
  refreshChartKlines,
  type ChartHistoryRange,
} from '../services/chartWorkspaceService'
import { normalizeChartWorkspaceKey } from '../utils/chartWorkspace'
import { klineBarsEqual, periodToInterval, toKLineData } from '../utils/klinecharts'

export interface ChartHistoryRefreshResult {
  key: ChartWorkspaceKey
  generation: number
  range: Required<ChartHistoryRange>
  klines: KLineData[]
}

type ChartContextChangeHandler = (key: ChartWorkspaceKey) => Promise<void>
type ChartHistoryRefreshHandler = (result: ChartHistoryRefreshResult) => void

interface PrimedHistory {
  key: ChartWorkspaceKey
  generation: number
  klines: KLineData[]
}

interface ExpectedContext {
  key: ChartWorkspaceKey
  generation: number
}

interface ExpectedContextWaiter extends ExpectedContext {
  resolve: (matched: boolean) => void
  timeout: ReturnType<typeof setTimeout>
}

function sameKey(first: ChartWorkspaceKey, second: ChartWorkspaceKey): boolean {
  return first.symbol === second.symbol && first.interval === second.interval
}

function rangeKey(range: Required<ChartHistoryRange>): string {
  return `${range.from}:${range.to}:${range.limit}`
}

function barsInRange(klines: KLineData[], range: Required<ChartHistoryRange>): KLineData[] {
  return klines.filter(({ timestamp }) => timestamp >= range.from && timestamp <= range.to)
}

export class EasiKlineDatafeed implements Datafeed {
  private realtimeCallback: DatafeedSubscribeCallback | null = null
  private subscriptionKey: ChartWorkspaceKey | null = null
  private lastRealtimeBar: KLineData | null = null
  private primedHistory: PrimedHistory | null = null
  private primeVersion = 0
  private expectedContext: ExpectedContext | null = null
  private resolvedExpectedContext: ExpectedContext | null = null
  private readonly expectedContextWaiters = new Set<ExpectedContextWaiter>()
  private historyRequestGeneration = 0
  private historyKeyEpoch = 0
  private latestHistoryKey: ChartWorkspaceKey | null = null
  private readonly latestHistoryRequestByRange = new Map<string, number>()
  private readonly reportedWarnings = new Set<string>()

  constructor(
    private getSymbols: () => string[],
    private getContext: () => ChartWorkspaceKey,
    private onContextChange: ChartContextChangeHandler,
    private onHistoryRefresh: ChartHistoryRefreshHandler,
    private report: (context: string, error: unknown) => void,
  ) {}

  primeHistory(key: ChartWorkspaceKey, generation: number, klines: Kline[]): void {
    const normalized = normalizeChartWorkspaceKey(key)
    if (this.primedHistory && generation < this.primedHistory.generation) {
      return
    }
    this.primedHistory = {
      key: normalized,
      generation,
      klines: toKLineData(klines),
    }
    this.primeVersion += 1
  }

  pushRealtimeBar(key: ChartWorkspaceKey, bar: KLineData | null): void {
    if (!bar || !this.realtimeCallback || !this.subscriptionKey) {
      return
    }
    const normalized = normalizeChartWorkspaceKey(key)
    if (!sameKey(normalized, this.subscriptionKey)) {
      return
    }
    if (this.lastRealtimeBar && klineBarsEqual(this.lastRealtimeBar, bar)) {
      return
    }
    this.lastRealtimeBar = bar
    this.realtimeCallback(bar)
  }

  expectProgrammaticContext(key: ChartWorkspaceKey, generation: number): void {
    const normalized = normalizeChartWorkspaceKey(key)
    this.resolveSupersededExpectedWaiters(normalized, generation)
    this.expectedContext = { key: normalized, generation }
    this.resolvedExpectedContext = null
  }

  waitForExpectedContext(
    key: ChartWorkspaceKey,
    generation: number,
    timeoutMs: number,
  ): Promise<boolean> {
    const normalized = normalizeChartWorkspaceKey(key)
    if (
      this.resolvedExpectedContext &&
      this.resolvedExpectedContext.generation === generation &&
      sameKey(this.resolvedExpectedContext.key, normalized)
    ) {
      return Promise.resolve(true)
    }

    return new Promise((resolve) => {
      const waiter: ExpectedContextWaiter = {
        key: normalized,
        generation,
        resolve,
        timeout: setTimeout(() => {
          this.expectedContextWaiters.delete(waiter)
          resolve(false)
        }, timeoutMs),
      }
      this.expectedContextWaiters.add(waiter)
    })
  }

  async searchSymbols(search?: string): Promise<SymbolInfo[]> {
    const query = (search ?? '').trim().toUpperCase()
    return this.getSymbols()
      .filter((symbol) => !query || symbol.toUpperCase().includes(query))
      .map((symbol) => ({
        ticker: symbol,
        shortName: symbol,
        name: symbol,
        exchange: 'EasiCoin',
        market: 'futures',
        priceCurrency: 'USDT',
        type: 'crypto',
      }))
  }

  async getHistoryKLineData(
    symbol: SymbolInfo,
    period: Period,
    from: number,
    to: number,
  ): Promise<KLineData[]> {
    const key = normalizeChartWorkspaceKey({
      symbol: symbol.ticker,
      interval: periodToInterval(period),
    })
    const range: Required<ChartHistoryRange> = { from, to, limit: 500 }
    const generation = ++this.historyRequestGeneration
    const keyEpoch = this.selectHistoryKey(key)
    const startingPrimeVersion = this.primeVersion
    this.latestHistoryRequestByRange.set(rangeKey(range), generation)

    if (this.expectedContext) {
      if (!sameKey(key, this.expectedContext.key)) {
        return this.currentPrimedRange(range)
      }
      this.resolveExpectedContext(this.expectedContext)
    } else if (!sameKey(key, normalizeChartWorkspaceKey(this.getContext()))) {
      await this.onContextChange(key)
      if (!this.isCurrentRequest(key, range, generation, keyEpoch, startingPrimeVersion)) {
        return this.currentPrimedRange(range)
      }
    }

    const primed = this.matchingPrimedRange(key, range)
    let local = primed
    if (local.length === 0) {
      const warning = this.warningKey('local', key, range)
      try {
        const snapshot = await loadChartWorkspace(key, range)
        this.reportedWarnings.delete(warning)
        if (!this.isCurrentRequest(key, range, generation, keyEpoch, startingPrimeVersion)) {
          return this.currentPrimedRange(range)
        }
        local = barsInRange(toKLineData(snapshot.klines), range)
      } catch (error) {
        this.reportOnce(warning, '加载本地图表历史失败', error)
        if (!this.isCurrentRequest(key, range, generation, keyEpoch, startingPrimeVersion)) {
          return this.currentPrimedRange(range)
        }
      }
    }

    const warning = this.warningKey('rest', key, range)
    const rest = refreshChartKlines(key, range)
    if (local.length > 0) {
      void rest.then((klines) => {
        this.reportedWarnings.delete(warning)
        globalThis.setTimeout(() => {
          if (!this.isCurrentRequest(key, range, generation, keyEpoch, startingPrimeVersion)) {
            return
          }
          this.onHistoryRefresh({
            key,
            generation,
            range,
            klines: barsInRange(toKLineData(klines), range),
          })
        }, 0)
      }).catch((error: unknown) => {
        this.reportOnce(warning, '刷新图表历史失败', error)
      })
      return local
    }

    try {
      const klines = await rest
      this.reportedWarnings.delete(warning)
      if (!this.isCurrentRequest(key, range, generation, keyEpoch, startingPrimeVersion)) {
        return this.currentPrimedRange(range)
      }
      return barsInRange(toKLineData(klines), range)
    } catch (error) {
      this.reportOnce(warning, '刷新图表历史失败', error)
      if (!this.isCurrentRequest(key, range, generation, keyEpoch, startingPrimeVersion)) {
        return this.currentPrimedRange(range)
      }
      return []
    }
  }

  subscribe(symbol: SymbolInfo, period: Period, callback: DatafeedSubscribeCallback): void {
    this.subscriptionKey = normalizeChartWorkspaceKey({
      symbol: symbol.ticker,
      interval: periodToInterval(period),
    })
    this.realtimeCallback = callback
    this.lastRealtimeBar = null
  }

  unsubscribe(symbol: SymbolInfo, period: Period): void {
    if (!this.subscriptionKey) {
      return
    }
    const key = normalizeChartWorkspaceKey({
      symbol: symbol.ticker,
      interval: periodToInterval(period),
    })
    if (!sameKey(key, this.subscriptionKey)) {
      return
    }
    this.subscriptionKey = null
    this.realtimeCallback = null
    this.lastRealtimeBar = null
  }

  private matchingPrimedRange(
    key: ChartWorkspaceKey,
    range: Required<ChartHistoryRange>,
  ): KLineData[] {
    if (!this.primedHistory || !sameKey(key, this.primedHistory.key)) {
      return []
    }
    return barsInRange(this.primedHistory.klines, range)
  }

  private currentPrimedRange(range: Required<ChartHistoryRange>): KLineData[] {
    return this.primedHistory ? barsInRange(this.primedHistory.klines, range) : []
  }

  private isCurrentRequest(
    key: ChartWorkspaceKey,
    range: Required<ChartHistoryRange>,
    generation: number,
    keyEpoch: number,
    startingPrimeVersion: number,
  ): boolean {
    if (
      keyEpoch !== this.historyKeyEpoch ||
      !this.latestHistoryKey ||
      !sameKey(key, this.latestHistoryKey) ||
      this.latestHistoryRequestByRange.get(rangeKey(range)) !== generation
    ) {
      return false
    }
    if (this.primeVersion !== startingPrimeVersion) {
      return false
    }
    return sameKey(key, normalizeChartWorkspaceKey(this.getContext()))
  }

  private selectHistoryKey(key: ChartWorkspaceKey): number {
    if (!this.latestHistoryKey || !sameKey(key, this.latestHistoryKey)) {
      this.latestHistoryKey = key
      this.historyKeyEpoch += 1
      this.latestHistoryRequestByRange.clear()
    }
    return this.historyKeyEpoch
  }

  private warningKey(
    operation: 'local' | 'rest',
    key: ChartWorkspaceKey,
    range: Required<ChartHistoryRange>,
  ): string {
    return `${operation}:${key.symbol}:${key.interval}:${rangeKey(range)}`
  }

  private reportOnce(warning: string, context: string, error: unknown): void {
    if (this.reportedWarnings.has(warning)) {
      return
    }
    this.reportedWarnings.add(warning)
    this.report(context, error)
  }

  private resolveExpectedContext(expected: ExpectedContext): void {
    this.expectedContext = null
    this.resolvedExpectedContext = expected
    for (const waiter of [...this.expectedContextWaiters]) {
      if (waiter.generation === expected.generation && sameKey(waiter.key, expected.key)) {
        clearTimeout(waiter.timeout)
        this.expectedContextWaiters.delete(waiter)
        waiter.resolve(true)
      }
    }
  }

  private resolveSupersededExpectedWaiters(key: ChartWorkspaceKey, generation: number): void {
    for (const waiter of [...this.expectedContextWaiters]) {
      if (waiter.generation !== generation || !sameKey(waiter.key, key)) {
        clearTimeout(waiter.timeout)
        this.expectedContextWaiters.delete(waiter)
        waiter.resolve(false)
      }
    }
  }
}
