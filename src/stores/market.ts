import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import { parseInstrumentSymbols } from '../utils/instruments'
import type { Depth, Kline, Ticker } from '../types/models'

const INSTRUMENTS_CACHE_KEY = 'easiflux_instruments_v1'

export const useMarketStore = defineStore('market', () => {
  const activeSymbol = ref('BTCUSDT')
  const klineInterval = ref('1')
  const ticker = ref<Ticker | null>(null)
  const depth = ref<Depth | null>(null)
  const klines = ref<Kline[]>([])
  const symbols = ref<string[]>([])
  const symbolsLoading = ref(false)
  let instrumentsFetchPromise: Promise<void> | null = null

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
      } catch {
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
    const first = next[0]
    if (
      first.symbol !== activeSymbol.value ||
      first.interval !== klineInterval.value
    ) {
      return
    }
    klines.value = next
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
    await tauriInvoke('refresh_market')
  }

  return {
    activeSymbol,
    klineInterval,
    ticker,
    depth,
    klines,
    symbols,
    symbolsLoading,
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
