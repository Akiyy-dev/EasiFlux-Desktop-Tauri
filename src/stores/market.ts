import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { Depth, Kline, Ticker } from '../types/models'

export const useMarketStore = defineStore('market', () => {
  const activeSymbol = ref('BTCUSDT')
  const klineInterval = ref('1')
  const ticker = ref<Ticker | null>(null)
  const depth = ref<Depth | null>(null)
  const klines = ref<Kline[]>([])

  function setTicker(next: Ticker): void {
    ticker.value = next
    activeSymbol.value = next.symbol
  }

  function setDepth(next: Depth): void {
    depth.value = next
  }

  function setKlines(next: Kline[]): void {
    if (next.length === 0 && klines.value.length > 0) {
      return
    }
    klines.value = next
  }

  async function setActiveSymbol(symbol: string): Promise<void> {
    await tauriInvoke('set_active_symbol', { symbol })
    activeSymbol.value = symbol
  }

  async function setKlineInterval(interval: string): Promise<void> {
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
    setTicker,
    setDepth,
    setKlines,
    setActiveSymbol,
    setKlineInterval,
    refreshMarket,
  }
})
