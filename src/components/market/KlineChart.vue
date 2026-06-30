<script setup lang="ts">
import { computed, watch } from 'vue'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { CandlestickChart } from 'echarts/charts'
import { GridComponent, TooltipComponent, DataZoomComponent } from 'echarts/components'
import VChart from 'vue-echarts'
import { storeToRefs } from 'pinia'
import { useMarketStore } from '../../stores/market'
import { NSelect } from 'naive-ui'

use([CanvasRenderer, CandlestickChart, GridComponent, TooltipComponent, DataZoomComponent])

const marketStore = useMarketStore()
const { klines, klineInterval } = storeToRefs(marketStore)

const intervals = [
  { label: '1m', value: '1' },
  { label: '5m', value: '5' },
  { label: '15m', value: '15' },
  { label: '1h', value: '60' },
  { label: '4h', value: '240' },
  { label: '1D', value: 'D' },
]

const option = computed(() => {
  const data = klines.value.map((k) => [
    k.openTime,
    parseFloat(k.open),
    parseFloat(k.close),
    parseFloat(k.low),
    parseFloat(k.high),
  ])
  return {
    backgroundColor: 'transparent',
    grid: { left: 48, right: 16, top: 24, bottom: 48 },
    tooltip: { trigger: 'axis' },
    xAxis: { type: 'time', axisLine: { lineStyle: { color: '#30363d' } } },
    yAxis: {
      scale: true,
      splitLine: { lineStyle: { color: '#21262d' } },
      axisLine: { lineStyle: { color: '#30363d' } },
    },
    dataZoom: [{ type: 'inside' }, { type: 'slider', height: 18, bottom: 4 }],
    series: [
      {
        type: 'candlestick',
        data,
        itemStyle: {
          color: '#26a69a',
          color0: '#ef5350',
          borderColor: '#26a69a',
          borderColor0: '#ef5350',
        },
      },
    ],
  }
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
    <VChart class="chart-view" :option="option" autoresize />
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
