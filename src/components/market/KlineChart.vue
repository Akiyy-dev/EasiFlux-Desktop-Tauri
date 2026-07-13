<script setup lang="ts">
import { onMounted, onUnmounted, ref, shallowRef, watch } from 'vue'
import { KLineChartPro } from '@klinecharts/pro'
import '@klinecharts/pro/dist/klinecharts-pro.css'
import { NButton, NCheckbox, NCheckboxGroup, NPopover, NRadioButton, NRadioGroup } from 'naive-ui'
import { storeToRefs } from 'pinia'
import { EasiKlineDatafeed } from '../../composables/easiKlineDatafeed'
import { useMarketStore } from '../../stores/market'
import {
  chartViewportKey,
  loadChartSettings,
  normalizeChartSettings,
  saveChartSettings,
  type ChartLayout,
  type ChartSettings,
} from '../../services/chartSettingsService'
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
const settings = ref<ChartSettings>(loadChartSettings())
let currentViewKey = chartViewportKey(activeSymbol.value, klineInterval.value)
let viewportTimer: ReturnType<typeof globalThis.setTimeout> | null = null

type CoreChartBridge = {
  getBarSpace?: () => number
  setBarSpace?: (space: number) => void
  getVisibleRange?: () => { to: number }
  getDataList?: () => Array<{ timestamp: number }>
  scrollToTimestamp?: (timestamp: number) => void
  subscribeAction?: (type: string, callback: () => void) => void
  unsubscribeAction?: (type: string, callback?: () => void) => void
}

function coreChart(): CoreChartBridge | null {
  const bridge = chartPro.value as unknown as { _chartApi?: CoreChartBridge }
  return bridge?._chartApi ?? null
}

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

function saveViewport(): void {
  const chart = coreChart()
  if (!chart) return
  const barSpace = chart.getBarSpace?.()
  const range = chart.getVisibleRange?.()
  const data = chart.getDataList?.() ?? []
  const rightTimestamp = range ? data[Math.min(range.to, data.length - 1)]?.timestamp : undefined
  settings.value.viewports[currentViewKey] = {
    ...(typeof barSpace === 'number' && barSpace >= 1 && barSpace <= 50 ? { barSpace } : {}),
    ...(rightTimestamp ? { rightTimestamp } : {}),
  }
  saveChartSettings(settings.value)
}

function queueViewportSave(): void {
  if (viewportTimer) globalThis.clearTimeout(viewportTimer)
  viewportTimer = globalThis.setTimeout(saveViewport, 250)
}

function restoreViewport(): void {
  const chart = coreChart()
  const viewport = settings.value.viewports[currentViewKey]
  if (!chart || !viewport) return
  if (viewport.barSpace) chart.setBarSpace?.(viewport.barSpace)
  if (viewport.rightTimestamp) chart.scrollToTimestamp?.(viewport.rightTimestamp)
}

function subscribeViewport(): void {
  const chart = coreChart()
  chart?.subscribeAction?.('onZoom', queueViewportSave)
  chart?.subscribeAction?.('onScroll', queueViewportSave)
  chart?.subscribeAction?.('onVisibleRangeChange', queueViewportSave)
  chart?.subscribeAction?.('onDataReady', restoreViewport)
}

function unsubscribeViewport(): void {
  const chart = coreChart()
  chart?.unsubscribeAction?.('onZoom', queueViewportSave)
  chart?.unsubscribeAction?.('onScroll', queueViewportSave)
  chart?.unsubscribeAction?.('onVisibleRangeChange', queueViewportSave)
  chart?.unsubscribeAction?.('onDataReady', restoreViewport)
}

function createChart(): void {
  if (!chartContainer.value) {
    return
  }
  unsubscribeViewport()
  chartContainer.value.replaceChildren()
  chartPro.value = new KLineChartPro({
    container: chartContainer.value,
    theme: 'dark',
    locale: 'zh-CN',
    drawingBarVisible: settings.value.layout === 'standard',
    symbol: toSymbolInfo(activeSymbol.value),
    period: intervalToPeriod(klineInterval.value),
    periods: KLINE_PERIODS,
    mainIndicators: settings.value.mainIndicators,
    subIndicators: settings.value.subIndicators,
    datafeed,
  })
  subscribeViewport()
  pushRealtimeFromStore()
  globalThis.setTimeout(restoreViewport, 0)
}

function updateSettings(patch: Partial<ChartSettings>): void {
  saveViewport()
  settings.value = normalizeChartSettings({ ...settings.value, ...patch })
  saveChartSettings(settings.value)
  createChart()
}

function updateMainIndicators(value: Array<string | number>): void {
  updateSettings({ mainIndicators: value.map(String) })
}

function updateSubIndicators(value: Array<string | number>): void {
  updateSettings({ subIndicators: value.map(String) })
}

function updateLayout(value: string | number | boolean): void {
  updateSettings({ layout: value as ChartLayout })
}

onMounted(() => {
  createChart()
})

onUnmounted(() => {
  saveViewport()
  unsubscribeViewport()
  if (viewportTimer) globalThis.clearTimeout(viewportTimer)
  chartContainer.value?.replaceChildren()
  chartPro.value = null
})

watch(klines, () => {
  pushRealtimeFromStore()
})

watch(activeSymbol, (symbol) => {
  saveViewport()
  currentViewKey = chartViewportKey(symbol, klineInterval.value)
  chartPro.value?.setSymbol(toSymbolInfo(symbol))
})

watch(klineInterval, (interval) => {
  saveViewport()
  currentViewKey = chartViewportKey(activeSymbol.value, interval)
  chartPro.value?.setPeriod(intervalToPeriod(interval))
})
</script>

<template>
  <div class="chart">
    <div class="chart-toolbar">
      <span class="chart-state">
        {{ activeSymbol }} · {{ klineInterval }} ·
        {{ settings.layout === 'compact' ? '紧凑布局' : '标准布局' }}
      </span>
      <NPopover trigger="click" placement="bottom-end">
        <template #trigger>
          <NButton size="tiny" secondary>
            图表设置
          </NButton>
        </template>
        <div class="settings-popover">
          <div class="settings-group">
            <strong>布局</strong>
            <NRadioGroup
              :value="settings.layout"
              size="small"
              @update:value="updateLayout"
            >
              <NRadioButton value="compact">
                紧凑
              </NRadioButton>
              <NRadioButton value="standard">
                标准
              </NRadioButton>
            </NRadioGroup>
          </div>
          <div class="settings-group">
            <strong>主图指标</strong>
            <NCheckboxGroup
              :value="settings.mainIndicators"
              @update:value="updateMainIndicators"
            >
              <div class="indicator-grid">
                <NCheckbox v-for="item in ['MA', 'EMA', 'BOLL', 'SAR']" :key="item" :value="item">
                  {{ item }}
                </NCheckbox>
              </div>
            </NCheckboxGroup>
          </div>
          <div class="settings-group">
            <strong>副图指标</strong>
            <NCheckboxGroup
              :value="settings.subIndicators"
              @update:value="updateSubIndicators"
            >
              <div class="indicator-grid">
                <NCheckbox v-for="item in ['VOL', 'MACD', 'RSI', 'KDJ']" :key="item" :value="item">
                  {{ item }}
                </NCheckbox>
              </div>
            </NCheckboxGroup>
          </div>
          <span class="settings-note">周期、缩放和可视位置将自动保存。</span>
        </div>
      </NPopover>
    </div>
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

.chart-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--ef-space-2);
  min-height: 32px;
  padding: 0 var(--ef-space-2) var(--ef-space-1);
}

.chart-state {
  overflow: hidden;
  color: var(--text-secondary);
  font-family: var(--font-mono);
  font-size: var(--ef-text-sm);
  text-overflow: ellipsis;
  white-space: nowrap;
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

.settings-popover {
  display: flex;
  flex-direction: column;
  gap: var(--ef-space-3);
  width: 250px;
}

.settings-group {
  display: flex;
  flex-direction: column;
  gap: var(--ef-space-2);
}

.settings-group strong {
  font-size: var(--ef-text-sm);
}

.indicator-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--ef-space-1) var(--ef-space-3);
}

.settings-note {
  color: var(--text-secondary);
  font-size: var(--ef-text-xs);
}
</style>
