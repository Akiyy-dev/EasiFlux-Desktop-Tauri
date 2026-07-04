<script setup lang="ts">
import { onMounted, onUnmounted, ref, shallowRef, watch } from 'vue'
import {
  CandlestickSeries,
  ColorType,
  createChart,
  type IChartApi,
  type ISeriesApi,
  type CandlestickData,
} from 'lightweight-charts'
import { storeToRefs } from 'pinia'
import { useMarketStore } from '../../stores/market'
import { candlesEqual, toCandlestickData } from '../../utils/kline'
import { NSelect } from 'naive-ui'

const marketStore = useMarketStore()
const { klines, klineInterval, activeSymbol } = storeToRefs(marketStore)

const chartContainer = ref<globalThis.HTMLDivElement | null>(null)
const chart = shallowRef<IChartApi | null>(null)
const series = shallowRef<ISeriesApi<'Candlestick'> | null>(null)
let resizeObserver: globalThis.ResizeObserver | null = null

const lastSeriesKey = ref('')
const lastCandles = ref<CandlestickData[]>([])
const resetViewport = ref(true)

const intervals = [
  { label: '1m', value: '1' },
  { label: '5m', value: '5' },
  { label: '15m', value: '15' },
  { label: '1h', value: '60' },
  { label: '4h', value: '240' },
  { label: '1D', value: 'D' },
]

function seriesKey(): string {
  return `${activeSymbol.value}-${klineInterval.value}`
}

function applyKlines(next: CandlestickData[]): void {
  if (next.length === 0 || !series.value) {
    return
  }

  const key = seriesKey()
  const shouldFit = resetViewport.value || lastSeriesKey.value !== key || lastCandles.value.length === 0
  const prev = lastCandles.value

  if (!shouldFit && prev.length > 0 && next.length > 0) {
    const prefixSame =
      prev.length === next.length
        ? prev.slice(0, -1).every((candle, index) => candlesEqual(candle, next[index]!))
        : prev.length + 1 === next.length &&
          prev.every((candle, index) => candlesEqual(candle, next[index]!))
    const last = next[next.length - 1]
    const prevLast = prev[prev.length - 1]
    if (prefixSame && last && (!prevLast || !candlesEqual(prevLast, last))) {
      series.value.update(last)
      lastCandles.value = next
      lastSeriesKey.value = key
      resetViewport.value = false
      return
    }
  }

  series.value.setData(next)
  lastCandles.value = next
  lastSeriesKey.value = key

  if (shouldFit) {
    chart.value?.timeScale().fitContent()
    resetViewport.value = false
  }
}

function resizeChart(): void {
  if (!chartContainer.value || !chart.value) {
    return
  }
  const { clientWidth, clientHeight } = chartContainer.value
  chart.value.applyOptions({ width: clientWidth, height: clientHeight })
}

onMounted(() => {
  if (!chartContainer.value) {
    return
  }

  const instance = createChart(chartContainer.value, {
    layout: {
      background: { type: ColorType.Solid, color: 'transparent' },
      textColor: '#8b949e',
    },
    grid: {
      vertLines: { color: '#21262d' },
      horzLines: { color: '#21262d' },
    },
    rightPriceScale: {
      borderColor: '#30363d',
    },
    timeScale: {
      borderColor: '#30363d',
      timeVisible: true,
      secondsVisible: false,
    },
    crosshair: {
      vertLine: { color: '#484f58' },
      horzLine: { color: '#484f58' },
    },
  })

  const candleSeries = instance.addSeries(CandlestickSeries, {
    upColor: '#26a69a',
    downColor: '#ef5350',
    borderUpColor: '#26a69a',
    borderDownColor: '#ef5350',
    wickUpColor: '#26a69a',
    wickDownColor: '#ef5350',
  })

  chart.value = instance
  series.value = candleSeries
  resetViewport.value = true
  applyKlines(toCandlestickData(klines.value))

  resizeObserver = new globalThis.ResizeObserver(() => {
    resizeChart()
  })
  resizeObserver.observe(chartContainer.value)
  resizeChart()
})

onUnmounted(() => {
  resizeObserver?.disconnect()
  resizeObserver = null
  chart.value?.remove()
  chart.value = null
  series.value = null
})

watch(klines, (next) => {
  applyKlines(toCandlestickData(next))
})

watch(activeSymbol, () => {
  resetViewport.value = true
})

async function onIntervalChange(interval: string): Promise<void> {
  if (interval === klineInterval.value) {
    return
  }
  resetViewport.value = true
  await marketStore.setKlineInterval(interval)
}
</script>

<template>
  <div class="chart">
    <div class="toolbar">
      <NSelect
        :value="klineInterval"
        :options="intervals"
        size="small"
        style="width: 88px"
        @update:value="onIntervalChange"
      />
    </div>
    <div ref="chartContainer" class="chart-view" />
  </div>
</template>

<style scoped>
.chart {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 240px;
}

.toolbar {
  padding: 6px 8px;
}

.chart-view {
  flex: 1;
  min-height: 200px;
}
</style>
