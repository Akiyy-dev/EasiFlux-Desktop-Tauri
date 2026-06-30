<script setup lang="ts">
import { NDataTable } from 'naive-ui'
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import { usePositionStore } from '../../stores/position'
import { useConnectionStore } from '../../stores/connection'

const positionStore = usePositionStore()
const connectionStore = useConnectionStore()
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

if (connectionStore.connected) {
  positionStore.refreshPositions()
}
</script>

<template>
  <NDataTable :columns="columns" :data="data" size="small" :bordered="false" flex-height />
</template>
