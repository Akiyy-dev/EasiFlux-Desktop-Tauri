<script setup lang="ts">
import { NButton, NDataTable } from 'naive-ui'
import { computed, onMounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { usePositionStore } from '../../stores/position'
import { useConnectionStore } from '../../stores/connection'
import { refreshSyncTask } from '../../services/dataSyncService'
import { reportError } from '../../services/errorService'

const TABLE_MAX_HEIGHT = 168

const props = defineProps<{
  active?: boolean
}>()

const positionStore = usePositionStore()
const connectionStore = useConnectionStore()
const { positions } = storeToRefs(positionStore)

const loading = ref(false)

const columns = [
  { title: '交易对', key: 'symbol' },
  { title: '方向', key: 'side' },
  { title: '数量', key: 'size' },
  { title: '开仓价', key: 'entryPrice' },
  { title: '杠杆', key: 'leverage' },
  { title: '未实现盈亏', key: 'unrealisedPnl' },
]

const data = computed(() => positions.value)

async function refreshPanels(): Promise<void> {
  if (!connectionStore.connected) {
    return
  }
  loading.value = true
  try {
    await refreshSyncTask('privatePanels')
  } catch (e) {
    reportError(e)
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  void refreshPanels()
})

watch(
  () => connectionStore.connected,
  (isConnected) => {
    if (isConnected) {
      void refreshPanels()
    }
  },
  { immediate: true },
)

watch(
  () => props.active,
  (isActive) => {
    if (isActive) {
      void refreshPanels()
    }
  },
)
</script>

<template>
  <div class="position-table">
    <div class="toolbar">
      <span class="meta">持仓 ({{ positions.length }})</span>
      <NButton size="tiny" :loading="loading" @click="refreshPanels">刷新</NButton>
    </div>
    <NDataTable
      :columns="columns"
      :data="data"
      :row-key="(row) => `${row.symbol}:${row.positionIdx ?? 0}`"
      :max-height="TABLE_MAX_HEIGHT"
      size="small"
      :bordered="false"
    />
  </div>
</template>

<style scoped>
.position-table {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 4px 8px;
  flex-shrink: 0;
}

.meta {
  font-size: 11px;
  color: var(--text-secondary);
}
</style>
