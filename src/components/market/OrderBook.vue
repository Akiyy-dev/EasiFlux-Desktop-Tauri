<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import { useMarketStore } from '../../stores/market'
import type { DepthLevel } from '../../types/models'

const marketStore = useMarketStore()
const { depth } = storeToRefs(marketStore)

type DepthRow = DepthLevel & { cumulative: number; ratio: number }

function buildRows(levels: DepthLevel[], limit: number, reverse = false): DepthRow[] {
  const slice = reverse ? [...levels].reverse().slice(0, limit) : levels.slice(0, limit)
  let cumulative = 0
  const rows = slice.map((level) => {
    const qty = Number.parseFloat(level.qty)
    cumulative += Number.isFinite(qty) ? qty : 0
    return { ...level, cumulative, ratio: 0 }
  })
  const maxCumulative = rows[rows.length - 1]?.cumulative ?? 0
  return rows.map((row) => ({
    ...row,
    ratio: maxCumulative > 0 ? row.cumulative / maxCumulative : 0,
  }))
}

const askRows = computed(() => buildRows(depth.value?.asks ?? [], 14, true))
const bidRows = computed(() => buildRows(depth.value?.bids ?? [], 14))
</script>

<template>
  <div class="order-book">
    <div class="header">
      <span>价格</span>
      <span>数量</span>
      <span>累计</span>
    </div>

    <div class="side asks">
      <div v-for="(row, index) in askRows" :key="'a' + index" class="row ask-row">
        <div class="bar ask-bar" :style="{ width: `${row.ratio * 100}%` }" />
        <span class="price text-down">{{ row.price }}</span>
        <span class="qty">{{ row.qty }}</span>
        <span class="cum">{{ row.cumulative.toFixed(4) }}</span>
      </div>
    </div>

    <div class="spread">盘口深度</div>

    <div class="side bids">
      <div v-for="(row, index) in bidRows" :key="'b' + index" class="row bid-row">
        <div class="bar bid-bar" :style="{ width: `${row.ratio * 100}%` }" />
        <span class="price text-up">{{ row.price }}</span>
        <span class="qty">{{ row.qty }}</span>
        <span class="cum">{{ row.cumulative.toFixed(4) }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.order-book {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 4px 8px 8px;
  font-size: 11px;
  font-family: ui-monospace, monospace;
  overflow: auto;
}

.header,
.row {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  gap: 8px;
  align-items: center;
}

.header {
  color: var(--text-secondary);
  padding: 4px 0 6px;
  border-bottom: 1px solid var(--border-color);
}

.row {
  position: relative;
  padding: 2px 0;
}

.bar {
  position: absolute;
  top: 1px;
  bottom: 1px;
  right: 0;
  opacity: 0.18;
  pointer-events: none;
}

.ask-bar {
  background: var(--accent-red);
}

.bid-bar {
  background: var(--accent-green);
}

.price,
.qty,
.cum {
  position: relative;
  z-index: 1;
}

.qty,
.cum {
  text-align: right;
  color: var(--text-secondary);
}

.spread {
  text-align: center;
  color: var(--text-secondary);
  padding: 8px 0;
  font-size: 10px;
  border-top: 1px solid var(--border-color);
  border-bottom: 1px solid var(--border-color);
  margin: 4px 0;
}
</style>
