import { onMounted, ref } from 'vue'
import { tauriInvoke } from './useTauriCommand'
import type { Ticker } from '../types/models'

export const DASHBOARD_WATCH_SYMBOLS = ['BTCUSDT', 'ETHUSDT', 'SOLUSDT'] as const

export function useDashboardTickers() {
  const tickers = ref<Ticker[]>([])
  const loading = ref(false)

  async function refresh(): Promise<void> {
    loading.value = true
    try {
      const results = await Promise.allSettled(
        DASHBOARD_WATCH_SYMBOLS.map((symbol) =>
          tauriInvoke<Ticker>('fetch_ticker', { symbol }),
        ),
      )
      tickers.value = results
        .filter((result): result is PromiseFulfilledResult<Ticker> => result.status === 'fulfilled')
        .map((result) => result.value)
    } finally {
      loading.value = false
    }
  }

  onMounted(() => {
    void refresh()
  })

  return { tickers, loading, refresh }
}
