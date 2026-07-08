<script setup lang="ts">
import { computed } from 'vue'
import { NSelect } from 'naive-ui'
import { storeToRefs } from 'pinia'
import { useConfigStore } from '../../stores/config'
import { useMarketStore } from '../../stores/market'
import { reportError } from '../../services/errorService'

const configStore = useConfigStore()
const marketStore = useMarketStore()
const { config } = storeToRefs(configStore)
const { activeSymbol, symbols, symbolsLoading } = storeToRefs(marketStore)

const options = computed(() => {
  const list =
    symbols.value.length > 0
      ? symbols.value
      : (config.value?.watchlistSymbols ?? ['BTCUSDT', 'ETHUSDT'])
  return list.map((symbol) => ({ label: symbol, value: symbol }))
})

function onUpdate(symbol: string): void {
  if (symbol === activeSymbol.value) {
    return
  }
  void marketStore.setActiveSymbol(symbol).catch((error: unknown) => {
    reportError(error)
  })
}
</script>

<template>
  <div class="symbol-selector">
    <NSelect
      :value="activeSymbol"
      :options="options"
      :loading="symbolsLoading"
      filterable
      placeholder="选择交易对"
      size="small"
      @update:value="onUpdate"
    />
  </div>
</template>

<style scoped>
.symbol-selector {
  width: 168px;
  flex-shrink: 0;
}
</style>
