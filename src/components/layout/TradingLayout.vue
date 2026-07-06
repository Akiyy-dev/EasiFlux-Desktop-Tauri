<script setup lang="ts">
import { onMounted } from 'vue'
import { storeToRefs } from 'pinia'
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
      <div class="panel chart-panel">
        <div class="panel-title">K 线</div>
        <KlineChart :key="`${activeSymbol}-${klineInterval}`" />
      </div>

      <div class="panel depth-panel">
        <div class="panel-title">深度</div>
        <OrderBook />
      </div>

      <RightPanel class="trade-panel" />
    </div>

    <BottomPanel />
  </div>
</template>

<style scoped>
.trading-layout {
  display: flex;
  flex-direction: column;
  gap: 8px;
  height: 100%;
  min-height: 0;
}

.trading-main {
  display: grid;
  grid-template-columns:
    minmax(360px, var(--trading-col-chart, 2.2fr))
    minmax(200px, var(--trading-col-depth, 0.85fr))
    minmax(280px, var(--trading-col-trade, 1fr));
  gap: 8px;
  flex: 1;
  min-height: 0;
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

.chart-panel :deep(.chart),
.depth-panel :deep(.order-book) {
  flex: 1;
  min-height: 0;
}
</style>
