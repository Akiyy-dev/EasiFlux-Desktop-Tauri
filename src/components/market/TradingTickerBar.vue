<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { storeToRefs } from 'pinia'
import AppCard from '../ui/AppCard.vue'
import MonoValue from '../ui/MonoValue.vue'
import { useMarketStore } from '../../stores/market'
import { change24hPctValue, formatChange24hPct } from '../../utils/ticker'
import SymbolSelector from '../trading/SymbolSelector.vue'
import TickerMetric from './TickerMetric.vue'

const marketStore = useMarketStore()
const { ticker } = storeToRefs(marketStore)
const now = ref(Date.now())
let clockTimer: ReturnType<typeof globalThis.setInterval> | null = null

const formattedChange = computed(() => formatChange24hPct(ticker.value?.change24hPct))

const changeClass = computed(() => {
  const pct = change24hPctValue(ticker.value?.change24hPct)
  if (pct > 0) return 'text-up'
  if (pct < 0) return 'text-down'
  return ''
})

function displayValue(value?: string | null): string {
  if (!value || value === '0') {
    return '--'
  }
  return value
}

const formattedFundingRate = computed(() => {
  const value = Number.parseFloat(ticker.value?.fundingRate ?? '')
  if (!Number.isFinite(value)) {
    return '--'
  }
  return `${(value * 100).toFixed(4)}%`
})

const fundingCountdown = computed(() => {
  const next = ticker.value?.nextFundingTime
  if (!next || next <= now.value) {
    return '--:--:--'
  }
  const totalSeconds = Math.floor((next - now.value) / 1000)
  const hours = Math.floor(totalSeconds / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  const seconds = totalSeconds % 60
  return [hours, minutes, seconds].map((part) => String(part).padStart(2, '0')).join(':')
})

onMounted(() => {
  clockTimer = globalThis.setInterval(() => {
    now.value = Date.now()
  }, 1000)
})

onUnmounted(() => {
  if (clockTimer) {
    globalThis.clearInterval(clockTimer)
    clockTimer = null
  }
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
        <TickerMetric label="标记价格" :value="displayValue(ticker?.markPrice)" />
        <TickerMetric label="24h 最高" :value="displayValue(ticker?.high24h)" />
        <TickerMetric label="24h 最低" :value="displayValue(ticker?.low24h)" />
        <TickerMetric label="成交额" :value="displayValue(ticker?.volume24h)" />
        <TickerMetric label="资金费率" :value="formattedFundingRate" />
        <TickerMetric label="费率倒计时" :value="fundingCountdown" />
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
