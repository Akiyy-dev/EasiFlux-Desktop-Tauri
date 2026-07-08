<script setup lang="ts">
import { onMounted } from 'vue'
import { storeToRefs } from 'pinia'
import AppCard from '../ui/AppCard.vue'
import TradingTickerBar from '../market/TradingTickerBar.vue'
import KlineChart from '../market/KlineChart.vue'
import OrderBook from '../market/OrderBook.vue'
import RightPanel from './RightPanel.vue'
import BottomPanel from './BottomPanel.vue'
import { useConfigStore } from '../../stores/config'
import { useMarketStore } from '../../stores/market'

const configStore = useConfigStore()
const marketStore = useMarketStore()
const { config } = storeToRefs(configStore)
const { activeSymbol, klineInterval } = storeToRefs(marketStore)

onMounted(() => {
  void marketStore.loadInstruments(config.value?.watchlistSymbols ?? [])
})
</script>

<template>
  <div class="trading-layout">
    <TradingTickerBar />

    <div class="trading-main">
      <AppCard title="K 线" flush class="chart-panel">
        <KlineChart :key="`${activeSymbol}-${klineInterval}`" />
      </AppCard>

      <AppCard title="深度" flush class="depth-panel">
        <OrderBook />
      </AppCard>

      <RightPanel class="trade-panel" />
    </div>

    <BottomPanel />
  </div>
</template>

<style scoped>
.trading-layout {
  display: flex;
  flex-direction: column;
  gap: var(--ef-space-2);
  height: 100%;
  min-height: 0;
  overflow: hidden;
}

.trading-main {
  display: grid;
  grid-template-columns:
    minmax(420px, var(--trading-col-chart, 2.65fr))
    minmax(190px, var(--trading-col-depth, 0.78fr))
    minmax(300px, var(--trading-col-trade, 0.96fr));
  gap: var(--ef-space-2);
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.chart-panel,
.depth-panel,
.trade-panel {
  min-width: 0;
  min-height: 0;
}

.chart-panel,
.depth-panel {
  display: flex;
  flex-direction: column;
}

.chart-panel :deep(.ef-card-body),
.depth-panel :deep(.ef-card-body) {
  display: flex;
  flex: 1;
  min-height: 0;
}

.chart-panel :deep(.chart),
.depth-panel :deep(.order-book) {
  flex: 1;
  min-height: 0;
}

.chart-panel :deep(.ef-card-body) {
  padding: var(--ef-space-2);
}

@media (max-width: 1280px) {
  .trading-main {
    grid-template-columns:
      minmax(360px, 2fr)
      minmax(180px, 0.74fr)
      minmax(280px, 0.92fr);
  }
}
</style>
