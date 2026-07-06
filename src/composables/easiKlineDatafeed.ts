import type { KLineData } from 'klinecharts'
import type {
  Datafeed,
  DatafeedSubscribeCallback,
  Period,
  SymbolInfo,
} from '@klinecharts/pro'
import { tauriInvoke } from './useTauriCommand'
import type { Kline } from '../types/models'
import { periodToInterval, toKLineData } from '../utils/klinecharts'

type IntervalChangeHandler = (interval: string) => Promise<void>

export class EasiKlineDatafeed implements Datafeed {
  private realtimeCallback: DatafeedSubscribeCallback | null = null
  private lastRealtimeBar: KLineData | null = null

  constructor(
    private getSymbols: () => string[],
    private getInterval: () => string,
    private onIntervalChange: IntervalChangeHandler,
  ) {}

  pushRealtimeBar(bar: KLineData | null): void {
    if (!bar || !this.realtimeCallback) {
      return
    }
    if (this.lastRealtimeBar && bar.timestamp === this.lastRealtimeBar.timestamp) {
      const unchanged =
        this.lastRealtimeBar.open === bar.open &&
        this.lastRealtimeBar.high === bar.high &&
        this.lastRealtimeBar.low === bar.low &&
        this.lastRealtimeBar.close === bar.close &&
        (this.lastRealtimeBar.volume ?? 0) === (bar.volume ?? 0)
      if (unchanged) {
        return
      }
    }
    this.lastRealtimeBar = bar
    this.realtimeCallback(bar)
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
    void from
    void to
    const interval = periodToInterval(period)
    if (interval !== this.getInterval()) {
      await this.onIntervalChange(interval)
    }

    const klines = await tauriInvoke<Kline[]>('fetch_klines', {
      symbol: symbol.ticker,
      interval,
    })
    return toKLineData(klines)
  }

  subscribe(_symbol: SymbolInfo, _period: Period, callback: DatafeedSubscribeCallback): void {
    this.realtimeCallback = callback
    this.lastRealtimeBar = null
  }

  unsubscribe(symbol: SymbolInfo, period: Period): void {
    void symbol
    void period
    this.realtimeCallback = null
    this.lastRealtimeBar = null
  }
}
