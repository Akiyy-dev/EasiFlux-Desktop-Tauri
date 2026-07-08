import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import { parseInstrumentSymbols } from '../utils/instruments'
import type { Depth, Kline, Ticker } from '../types/models'
import { useAsyncState } from '../composables/useAsyncState'

const INSTRUMENTS_CACHE_KEY = 'easiflux_instruments_v1'

function intervalToMs(interval: string): number {
  if (interval.toUpperCase() === 'D') {
    return 86_400_000
  }
  const minutes = Number.parseInt(interval, 10)
  return Number.isFinite(minutes) && minutes > 0 ? minutes * 60_000 : 60_000
}

export const useMarketStore = defineStore('market', () => {
  const activeSymbol = ref('BTCUSDT')
  const klineInterval = ref('1')
  const ticker = ref<Ticker | null>(null)
  const depth = ref<Depth | null>(null)
  const klines = ref<Kline[]>([])
  const symbols = ref<string[]>([])
  const symbolsLoading = ref(false)
  let instrumentsFetchPromise: Promise<void> | null = null
  const marketRequest = useAsyncState<null>()
  const instrumentsRequest = useAsyncState<string[]>((value) => value.length === 0)

  function readCachedSymbols(): string[] {
    try {
      const raw = localStorage.getItem(INSTRUMENTS_CACHE_KEY)
      if (!raw) {
        return []
      }
      const parsed: unknown = JSON.parse(raw)
      if (!Array.isArray(parsed)) {
        return []
      }
      return parsed.filter((item): item is string => typeof item === 'string' && item.length > 0)
    } catch {
      return []
    }
  }

  function writeCachedSymbols(next: string[]): void {
    localStorage.setItem(INSTRUMENTS_CACHE_KEY, JSON.stringify(next))
  }

  async function loadInstruments(fallbackSymbols: string[] = []): Promise<void> {
    const cached = readCachedSymbols()
    if (cached.length > 0) {
      symbols.value = cached
    } else if (fallbackSymbols.length > 0) {
      symbols.value = [...fallbackSymbols]
    }

    if (instrumentsFetchPromise) {
      return instrumentsFetchPromise
    }

    instrumentsFetchPromise = (async () => {
      symbolsLoading.value = true
      try {
        const payload = await tauriInvoke<unknown>('fetch_instruments', {})
        const parsed = parseInstrumentSymbols(payload)
        if (parsed.length > 0) {
          symbols.value = parsed
          writeCachedSymbols(parsed)
        }
        instrumentsRequest.setData(parsed)
      } catch {
        instrumentsRequest.setError('交易对加载失败')
        if (symbols.value.length === 0 && fallbackSymbols.length > 0) {
          symbols.value = [...fallbackSymbols]
        }
      } finally {
        symbolsLoading.value = false
      }
    })()

    return instrumentsFetchPromise
  }

  function setTicker(next: Ticker): void {
    ticker.value = next
  }

  function setDepth(next: Depth): void {
    depth.value = next
  }

  function clearKlines(): void {
    klines.value = []
  }

  function setKlines(next: Kline[]): void {
    if (next.length === 0) {
      klines.value = []
      return
    }
    const normalized = normalizeKlines(next)
    if (normalized.length === 0) {
      return
    }
    klines.value = normalized
  }

  function normalizeKlines(next: Kline[]): Kline[] {
    const byTime = new Map<number, Kline>()
    for (const kline of next) {
      if (
        kline.symbol !== activeSymbol.value ||
        kline.interval !== klineInterval.value ||
        !Number.isFinite(kline.openTime) ||
        kline.openTime <= 0
      ) {
        continue
      }
      byTime.set(kline.openTime, kline)
    }

    const sorted = Array.from(byTime.values()).sort((a, b) => a.openTime - b.openTime)
    if (sorted.length < 2) {
      return sorted
    }

    const intervalMs = intervalToMs(klineInterval.value)
    const continuous: Kline[] = []
    for (const kline of sorted) {
      const previous = continuous[continuous.length - 1]
      if (previous) {
        let expected = previous.openTime + intervalMs
        let inserted = 0
        while (expected < kline.openTime && inserted < 500) {
          continuous.push({
            symbol: previous.symbol,
            interval: previous.interval,
            openTime: expected,
            open: previous.close,
            high: previous.close,
            low: previous.close,
            close: previous.close,
            volume: '0',
          })
          expected += intervalMs
          inserted += 1
        }
      }
      continuous.push(kline)
    }
    return continuous
  }

  async function setActiveSymbol(symbol: string): Promise<void> {
    if (symbol === activeSymbol.value) {
      return
    }
    clearKlines()
    activeSymbol.value = symbol
    try {
      await tauriInvoke('set_active_symbol', { symbol })
    } catch (error) {
      throw error instanceof Error ? error : new Error(String(error))
    }
  }

  async function setKlineInterval(interval: string): Promise<void> {
    clearKlines()
    await tauriInvoke('set_kline_interval', { interval })
    klineInterval.value = interval
  }

  async function refreshMarket(): Promise<void> {
    await marketRequest.run(async () => {
      await tauriInvoke('refresh_market')
      return null
    })
  }

  return {
    activeSymbol,
    klineInterval,
    ticker,
    depth,
    klines,
    symbols,
    symbolsLoading,
    marketLoading: marketRequest.loading,
    marketError: marketRequest.error,
    marketStatus: marketRequest.status,
    instrumentsError: instrumentsRequest.error,
    instrumentsStatus: instrumentsRequest.status,
    setTicker,
    setDepth,
    setKlines,
    clearKlines,
    setActiveSymbol,
    setKlineInterval,
    refreshMarket,
    loadInstruments,
  }
})
