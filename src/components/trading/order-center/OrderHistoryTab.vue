<script setup lang="ts">
import { createColumnHelper } from '@tanstack/vue-table'
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import TanstackDataTable from '../../common/TanstackDataTable.vue'
import { useOrderCenterRefresh } from '../../../composables/useOrderCenterRefresh'
import { useOrderStore } from '../../../stores/order'
import type { Order } from '../../../types/models'

const props = defineProps<{
  active: boolean
}>()

const orderStore = useOrderStore()
const { orderHistory } = storeToRefs(orderStore)
const { loading, refresh } = useOrderCenterRefresh(computed(() => props.active))

const columnHelper = createColumnHelper<Order>()

const columns = [
  columnHelper.accessor('orderId', { header: '订单ID', enableSorting: true }),
  columnHelper.accessor('symbol', { header: '交易对', enableSorting: true }),
  columnHelper.accessor('side', { header: '方向', enableSorting: true }),
  columnHelper.accessor('orderType', { header: '类型', enableSorting: true }),
  columnHelper.accessor('price', { header: '价格', enableSorting: true }),
  columnHelper.accessor('qty', { header: '数量', enableSorting: true }),
  columnHelper.accessor('filledQty', { header: '成交量', enableSorting: true }),
  columnHelper.accessor('status', { header: '状态', enableSorting: true }),
]

function rowId(row: Order): string {
  return row.orderId
}
</script>

<template>
  <TanstackDataTable
    :columns="columns"
    :data="orderHistory"
    :get-row-id="rowId"
    :loading="loading"
    search-placeholder="搜索历史委托…"
    :status-options="[
      { label: 'Filled', value: 'Filled' },
      { label: 'Cancelled', value: 'Cancelled' },
      { label: 'Rejected', value: 'Rejected' },
    ]"
    :status-accessor="(row) => row.status"
    :enable-selection="false"
    @refresh="refresh"
  />
</template>
