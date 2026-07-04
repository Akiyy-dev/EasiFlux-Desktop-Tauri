<script setup lang="ts">
import { NDataTable } from 'naive-ui'
import { computed, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { usePositionStore } from '../../stores/position'
import { useConnectionStore } from '../../stores/connection'
import { useLogStore } from '../../stores/log'

const positionStore = usePositionStore()
const connectionStore = useConnectionStore()
const logStore = useLogStore()
const { positions } = storeToRefs(positionStore)

const columns = [
  { title: '交易对', key: 'symbol' },
  { title: '方向', key: 'side' },
  { title: '数量', key: 'size' },
  { title: '开仓价', key: 'entryPrice' },
  { title: '杠杆', key: 'leverage' },
  { title: '未实现盈亏', key: 'unrealisedPnl' },
]

const data = computed(() => positions.value)

async function refresh(): Promise<void> {
  if (!connectionStore.connected) {
    return
  }
  try {
    await positionStore.refreshPositions()
  } catch (e) {
    logStore.setError(e instanceof Error ? e.message : String(e))
  }
}

watch(
  () => connectionStore.connected,
  (isConnected) => {
    if (isConnected) {
      void refresh()
    }
  },
  { immediate: true },
)
</script>

<template>
  <NDataTable :columns="columns" :data="data" size="small" :bordered="false" flex-height />
</template>
