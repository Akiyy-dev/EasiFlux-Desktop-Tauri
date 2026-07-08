<script setup lang="ts">
import { onMounted, onUnmounted, ref, shallowRef, watch } from 'vue'
import { KLineChartPro } from '@klinecharts/pro'
import '@klinecharts/pro/dist/klinecharts-pro.css'
import { storeToRefs } from 'pinia'
import { EasiKlineDatafeed } from '../../composables/easiKlineDatafeed'
import { useMarketStore } from '../../stores/market'
import {
  intervalToPeriod,
  KLINE_PERIODS,
  toKLineData,
  toSymbolInfo,
} from '../../utils/klinecharts'

const marketStore = useMarketStore()
const { klines, klineInterval, activeSymbol, symbols } = storeToRefs(marketStore)

const chartContainer = ref<globalThis.HTMLDivElement | null>(null)
const chartPro = shallowRef<KLineChartPro | null>(null)
const datafeed = new EasiKlineDatafeed(
  () => symbols.value,
  () => klineInterval.value,
  async (interval) => {
    if (interval !== klineInterval.value) {
      await marketStore.setKlineInterval(interval)
    }
  },
)

function pushRealtimeFromStore(): void {
  const bars = toKLineData(klines.value)
  const last = bars[bars.length - 1] ?? null
  datafeed.pushRealtimeBar(last)
}

onMounted(() => {
  if (!chartContainer.value) {
    return
  }

  chartPro.value = new KLineChartPro({
    container: chartContainer.value,
    theme: 'dark',
    locale: 'zh-CN',
    drawingBarVisible: false,
    symbol: toSymbolInfo(activeSymbol.value),
    period: intervalToPeriod(klineInterval.value),
    periods: KLINE_PERIODS,
    mainIndicators: ['MA', 'EMA'],
    subIndicators: ['VOL', 'MACD', 'RSI', 'KDJ'],
    datafeed,
  })

  pushRealtimeFromStore()
})

onUnmounted(() => {
  chartContainer.value?.replaceChildren()
  chartPro.value = null
})

watch(klines, () => {
  pushRealtimeFromStore()
})

watch(activeSymbol, (symbol) => {
  chartPro.value?.setSymbol(toSymbolInfo(symbol))
})

watch(klineInterval, (interval) => {
  chartPro.value?.setPeriod(intervalToPeriod(interval))
})
</script>

<template>
  <div class="chart">
    <div ref="chartContainer" class="chart-view" />
  </div>
</template>

<style scoped>
.chart {
  display: flex;
  flex-direction: column;
  height: 100%;
  width: 100%;
  min-height: clamp(320px, 42vh, 620px);
}

.chart-view {
  flex: 1;
  min-height: 0;
  width: 100%;
  overflow: hidden;
  border-radius: var(--ef-radius-md);
}

.chart-view :deep(.klinecharts-pro) {
  height: 100%;
  min-height: 0;
}
</style>
