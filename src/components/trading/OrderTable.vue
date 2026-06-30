<script setup lang="ts">
import { NButton, NDataTable } from 'naive-ui'
import { computed, h } from 'vue'
import { storeToRefs } from 'pinia'
import { useOrderStore } from '../../stores/order'
import { useConnectionStore } from '../../stores/connection'
import { useLogStore } from '../../stores/log'
import type { Order } from '../../types/models'

const orderStore = useOrderStore()
const connectionStore = useConnectionStore()
const logStore = useLogStore()
const { orders } = storeToRefs(orderStore)

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

const data = computed(() => orders.value)

async function cancel(row: Order): Promise<void> {
  try {
    await orderStore.cancelOrder({ symbol: row.symbol, orderId: row.orderId })
  } catch (e) {
    logStore.setError(e instanceof Error ? e.message : String(e))
  }
}

async function refresh(): Promise<void> {
  if (connectionStore.connected) {
    await orderStore.refreshOrders()
  }
}

refresh()
</script>

<template>
  <div class="order-table">
    <div class="toolbar">
      <NButton size="tiny" @click="refresh">刷新</NButton>
    </div>
    <NDataTable :columns="columns" :data="data" size="small" :bordered="false" flex-height />
  </div>
</template>

<style scoped>
.order-table {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.toolbar {
  padding: 4px 8px;
}
</style>
