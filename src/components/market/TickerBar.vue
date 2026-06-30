<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import { useMarketStore } from '../../stores/market'

const marketStore = useMarketStore()
const { ticker } = storeToRefs(marketStore)

const changeClass = computed(() => {
  const pct = parseFloat(ticker.value?.change24hPct ?? '0')
  if (pct > 0) return 'text-up'
  if (pct < 0) return 'text-down'
  return ''
})
</script>

<template>
  <div class="ticker-bar panel">
    <div class="symbol">{{ ticker?.symbol ?? '---' }}</div>
    <div class="price" :class="changeClass">{{ ticker?.lastPrice ?? '--' }}</div>
    <div class="meta">
      <span>买一 {{ ticker?.bidPrice ?? '--' }}</span>
      <span>卖一 {{ ticker?.askPrice ?? '--' }}</span>
      <span>24h量 {{ ticker?.volume24h ?? '--' }}</span>
      <span :class="changeClass">{{ ticker?.change24hPct ?? '0' }}%</span>
    </div>
  </div>
</template>

<style scoped>
.ticker-bar {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 10px 14px;
}

.symbol {
  font-weight: 700;
  font-size: 16px;
}

.price {
  font-size: 20px;
  font-weight: 600;
}

.meta {
  display: flex;
  gap: 16px;
  font-size: 12px;
  color: var(--text-secondary);
  margin-left: auto;
}
</style>
