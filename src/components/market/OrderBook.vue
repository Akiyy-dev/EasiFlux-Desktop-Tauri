<script setup lang="ts">
import { storeToRefs } from 'pinia'
import { useMarketStore } from '../../stores/market'

const marketStore = useMarketStore()
const { depth } = storeToRefs(marketStore)
</script>

<template>
  <div class="order-book">
    <div class="side asks">
      <div v-for="(level, i) in [...(depth?.asks ?? [])].reverse().slice(0, 12)" :key="'a' + i" class="row">
        <span class="text-down">{{ level.price }}</span>
        <span>{{ level.qty }}</span>
      </div>
    </div>
    <div class="spread">盘口</div>
    <div class="side bids">
      <div v-for="(level, i) in (depth?.bids ?? []).slice(0, 12)" :key="'b' + i" class="row">
        <span class="text-up">{{ level.price }}</span>
        <span>{{ level.qty }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.order-book {
  padding: 4px 8px;
  font-size: 11px;
  font-family: ui-monospace, monospace;
  overflow: auto;
  height: 100%;
}

.row {
  display: flex;
  justify-content: space-between;
  padding: 2px 0;
}

.spread {
  text-align: center;
  color: var(--text-secondary);
  padding: 6px 0;
  font-size: 10px;
  border-top: 1px solid var(--border-color);
  border-bottom: 1px solid var(--border-color);
  margin: 4px 0;
}
</style>
