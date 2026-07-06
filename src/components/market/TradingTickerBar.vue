<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import AppCard from '../ui/AppCard.vue'
import MonoValue from '../ui/MonoValue.vue'
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
  <AppCard as="header" flush class="trading-ticker-bar">
    <div class="ticker-inner">
      <div class="left">
        <SymbolSelector />
      </div>

      <div class="metrics">
        <div class="price-block">
          <MonoValue class="last-price" :class="changeClass" size="lg">
            {{ ticker?.lastPrice ?? '--' }}
          </MonoValue>
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
    </div>
  </AppCard>
</template>

<style scoped>
.trading-ticker-bar {
  flex-shrink: 0;
}

.ticker-inner {
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
  scrollbar-width: none;
  padding-bottom: 2px;
}

.metrics::-webkit-scrollbar {
  display: none;
}

.price-block {
  padding-right: 6px;
  border-right: 1px solid var(--border);
  margin-right: 2px;
  flex-shrink: 0;
}
</style>
