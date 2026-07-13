<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import { useAccountStore } from '../../stores/account'
import { useMarketStore } from '../../stores/market'
import { usePositionStore } from '../../stores/position'

const accountStore = useAccountStore()
const marketStore = useMarketStore()
const positionStore = usePositionStore()

const { summary } = storeToRefs(accountStore)
const { activeSymbol } = storeToRefs(marketStore)
const { positions } = storeToRefs(positionStore)

const usdtBalance = computed(() =>
  summary.value?.balances.find((balance) => balance.asset === 'USDT'),
)
const marginBalance = computed(() => usdtBalance.value?.frozen ?? summary.value?.balances[0]?.frozen)

const symbolUnrealisedPnl = computed(() => {
  const rows = positions.value.filter((position) => position.symbol === activeSymbol.value)
  const total = rows
    .map((position) => Number.parseFloat(position.unrealisedPnl))
    .filter((value) => Number.isFinite(value))
    .reduce((sum, value) => sum + value, 0)
  return rows.length > 0 ? total.toFixed(4) : '--'
})

const pnlClass = computed(() => {
  const value = Number.parseFloat(symbolUnrealisedPnl.value)
  if (!Number.isFinite(value) || symbolUnrealisedPnl.value === '--') {
    return ''
  }
  if (value > 0) return 'text-up'
  if (value < 0) return 'text-down'
  return ''
})
</script>

<template>
  <section class="asset-panel">
    <div class="title">资产信息</div>
    <div class="grid">
      <div class="item">
        <span class="label">币种权益</span>
        <span class="value">{{ summary?.totalEquity ?? '--' }} USDT</span>
      </div>
      <div class="item">
        <span class="label">可用保证金</span>
        <span class="value">{{ usdtBalance?.available ?? '--' }} USDT</span>
      </div>
      <div class="item">
        <span class="label">持仓保证金</span>
        <span class="value">{{ marginBalance ?? '--' }} USDT</span>
      </div>
      <div class="item">
        <span class="label">未实现盈亏</span>
        <span class="value" :class="pnlClass">{{ symbolUnrealisedPnl }}</span>
      </div>
      <div class="item">
        <span class="label">保证金率</span>
        <span class="value">--</span>
      </div>
      <div class="item">
        <span class="label">体验金</span>
        <span class="value">--</span>
      </div>
    </div>
  </section>
</template>

<style scoped>
.asset-panel {
  border-top: 1px solid var(--border-color);
  padding: var(--ef-space-3) 0 0;
}

.title {
  font-size: var(--ef-text-xs);
  font-weight: 600;
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  margin-bottom: 8px;
}

.grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--ef-space-2) var(--ef-space-3);
}

.item {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.label {
  font-size: var(--ef-text-xs);
  color: var(--text-secondary);
}

.value {
  font-size: var(--ef-text-sm);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
</style>
