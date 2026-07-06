<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import { useMarketStore } from '../../stores/market'
import { change24hPctValue, formatChange24hPct } from '../../utils/ticker'
import SymbolSelector from '../trading/SymbolSelector.vue'
import TickerMetric from './TickerMetric.vue'

const marketStore = useMarketStore()
const { ticker } = storeToRefs(marketStore)

const formattedChange = computed(() => formatChange24hPct(ticker.value?.change24hPct))

const changeClass = computed(() => {
  const pct = change24hPctValue(ticker.value?.change24hPct)
  if (pct > 0) return 'text-up'
  if (pct < 0) return 'text-down'
  return ''
})
</script>

<template>
  <header class="trading-ticker-bar panel">
    <div class="left">
      <SymbolSelector />
    </div>

    <div class="metrics">
      <div class="price-block">
        <span class="last-price" :class="changeClass">{{ ticker?.lastPrice ?? '--' }}</span>
      </div>

      <TickerMetric label="24h 涨跌" :value="formattedChange" :value-class="changeClass" />
      <TickerMetric label="标记价格" value="--" />
      <TickerMetric label="指数价格" value="--" />
      <TickerMetric label="24h 最高" value="--" />
      <TickerMetric label="24h 最低" value="--" />
      <TickerMetric label="成交额" :value="ticker?.volume24h ?? '--'" />
      <TickerMetric label="资金费率" value="--" />
      <TickerMetric label="费率倒计时" value="--:--:--" />
    </div>
  </header>
</template>

<style scoped>
.trading-ticker-bar {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 8px 12px;
  min-height: 56px;
}

.left {
  display: flex;
  align-items: center;
  flex-shrink: 0;
}

.metrics {
  display: flex;
  align-items: center;
  gap: 18px;
  margin-left: auto;
  min-width: 0;
  overflow-x: auto;
  padding-bottom: 2px;
}

.price-block {
  padding-right: 6px;
  border-right: 1px solid var(--border-color);
  margin-right: 2px;
}

.last-price {
  font-size: 22px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  line-height: 1.1;
}
</style>
