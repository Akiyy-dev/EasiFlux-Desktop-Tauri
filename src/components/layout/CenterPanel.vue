<script setup lang="ts">
import { storeToRefs } from 'pinia'
import TickerBar from '../market/TickerBar.vue'
import KlineChart from '../market/KlineChart.vue'
import OrderBook from '../market/OrderBook.vue'
import { useMarketStore } from '../../stores/market'

const { activeSymbol, klineInterval } = storeToRefs(useMarketStore())
</script>

<template>
  <section class="center-panel">
    <TickerBar />
    <div class="center-grid">
      <div class="panel chart-panel">
        <div class="panel-title">K 线</div>
        <KlineChart :key="`${activeSymbol}-${klineInterval}`" />
      </div>
      <div class="panel depth-panel">
        <div class="panel-title">深度</div>
        <OrderBook />
      </div>
    </div>
  </section>
</template>

<style scoped>
.center-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
  height: 100%;
  min-width: 0;
}

.center-grid {
  display: grid;
  grid-template-columns: 1fr 220px;
  gap: 8px;
  flex: 1;
  min-height: 0;
}

.chart-panel,
.depth-panel {
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.chart-panel :deep(.chart),
.depth-panel :deep(.order-book) {
  flex: 1;
  min-height: 0;
}
</style>
