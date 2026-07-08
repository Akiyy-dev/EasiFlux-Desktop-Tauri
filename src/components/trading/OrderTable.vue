<script setup lang="ts">
import { NButton, NDataTable, NRadioButton, NRadioGroup } from 'naive-ui'
import { computed, h, onMounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useOrderStore } from '../../stores/order'
import { useConnectionStore } from '../../stores/connection'
import { refreshSyncTask } from '../../services/dataSyncService'
import type { Order } from '../../types/models'
import { reportError } from '../../services/errorService'

const TABLE_MAX_HEIGHT = 168

const props = defineProps<{
  active?: boolean
}>()

const orderStore = useOrderStore()
const connectionStore = useConnectionStore()
const { openOrders, orderHistory } = storeToRefs(orderStore)

const activeScope = ref<'open' | 'history'>('open')
const loading = ref(false)

const columns = [
  { title: '订单ID', key: 'orderId', ellipsis: true },
  { title: '交易对', key: 'symbol', width: 90 },
  { title: '方向', key: 'side', width: 60 },
  { title: '类型', key: 'orderType', width: 70 },
  { title: '价格', key: 'price', width: 90 },
  { title: '数量', key: 'qty', width: 80 },
  { title: '状态', key: 'status', width: 80 },
  {
    title: '操作',
    key: 'actions',
    width: 70,
    render(row: Order) {
      return h(
        NButton,
        {
          size: 'tiny',
          quaternary: true,
          disabled: row.status === 'Filled' || row.status === 'Cancelled',
          onClick: () => cancel(row),
        },
        { default: () => '撤单' },
      )
    },
  },
]

const data = computed(() =>
  activeScope.value === 'open' ? openOrders.value : orderHistory.value,
)

const scopeLabel = computed(() =>
  activeScope.value === 'open'
    ? `当前挂单 (${openOrders.value.length})`
    : `历史订单 (${orderHistory.value.length})`,
)

async function cancel(row: Order): Promise<void> {
  try {
    await orderStore.cancelOrder({ symbol: row.symbol, orderId: row.orderId })
    await refreshPanels()
  } catch (e) {
    reportError(e)
  }
}

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
  <div class="order-table">
    <div class="toolbar">
      <NRadioGroup v-model:value="activeScope" size="small" class="scope-switch">
        <NRadioButton value="open">当前挂单</NRadioButton>
        <NRadioButton value="history">历史订单</NRadioButton>
      </NRadioGroup>
      <span class="meta">{{ scopeLabel }}</span>
      <NButton size="tiny" :loading="loading" @click="refreshPanels">刷新</NButton>
    </div>
    <NDataTable
      :columns="columns"
      :data="data"
      :row-key="(row: Order) => row.orderId"
      :max-height="TABLE_MAX_HEIGHT"
      size="small"
      :bordered="false"
    />
  </div>
</template>

<style scoped>
.order-table {
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

.scope-switch {
  flex-shrink: 0;
}

.meta {
  flex: 1;
  font-size: 11px;
  color: var(--text-secondary);
  text-align: right;
}
</style>
