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
import { toCandlestickData } from '../../utils/kline'
import { NSelect } from 'naive-ui'

const marketStore = useMarketStore()
const { klines, klineInterval } = storeToRefs(marketStore)

const chartContainer = ref<globalThis.HTMLDivElement | null>(null)
const chart = shallowRef<IChartApi | null>(null)
const series = shallowRef<ISeriesApi<'Candlestick'> | null>(null)
let resizeObserver: globalThis.ResizeObserver | null = null

const intervals = [
  { label: '1m', value: '1' },
  { label: '5m', value: '5' },
  { label: '15m', value: '15' },
  { label: '1h', value: '60' },
  { label: '4h', value: '240' },
  { label: '1D', value: 'D' },
]

function applyKlines(data: CandlestickData[]): void {
  series.value?.setData(data)
  chart.value?.timeScale().fitContent()
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

watch(klineInterval, async (interval) => {
  await marketStore.setKlineInterval(interval)
})
</script>

<template>
  <div class="chart">
    <div class="toolbar">
      <NSelect
        v-model:value="klineInterval"
        :options="intervals"
        size="small"
        style="width: 88px"
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
