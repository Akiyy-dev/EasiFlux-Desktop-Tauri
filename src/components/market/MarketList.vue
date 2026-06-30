<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import { useConfigStore } from '../../stores/config'
import { useMarketStore } from '../../stores/market'

const configStore = useConfigStore()
const marketStore = useMarketStore()
const { config } = storeToRefs(configStore)
const { activeSymbol } = storeToRefs(marketStore)

const symbols = computed(() => config.value?.watchlistSymbols ?? ['BTCUSDT', 'ETHUSDT'])

async function selectSymbol(symbol: string): Promise<void> {
  await marketStore.setActiveSymbol(symbol)
}
</script>

<template>
  <ul class="market-list">
    <li
      v-for="symbol in symbols"
      :key="symbol"
      class="item"
      :class="{ active: symbol === activeSymbol }"
      @click="selectSymbol(symbol)"
    >
      {{ symbol }}
    </li>
  </ul>
</template>

<style scoped>
.market-list {
  list-style: none;
  margin: 0;
  padding: 4px;
  overflow: auto;
  flex: 1;
}

.item {
  padding: 8px 10px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 13px;
}

.item:hover {
  background: var(--bg-tertiary);
}

.item.active {
  background: var(--bg-tertiary);
  color: var(--accent-blue);
  font-weight: 600;
}
</style>
