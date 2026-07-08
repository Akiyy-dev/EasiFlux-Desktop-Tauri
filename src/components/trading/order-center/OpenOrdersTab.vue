<script setup lang="ts">
import { createColumnHelper } from '@tanstack/vue-table'
import { computed, h, ref } from 'vue'
import { storeToRefs } from 'pinia'
import TanstackDataTable from '../../common/TanstackDataTable.vue'
import { AppButton } from '../../ui'
import { useOrderCenterRefresh } from '../../../composables/useOrderCenterRefresh'
import { useConnectionStore } from '../../../stores/connection'
import { useOrderStore } from '../../../stores/order'
import { refreshSyncTask } from '../../../services/dataSyncService'
import type { Order } from '../../../types/models'
import { filterOpenOrders, type OpenOrderScope } from '../../../utils/orderFilters'
import { reportError } from '../../../services/errorService'

const props = defineProps<{
  active: boolean
}>()

const orderStore = useOrderStore()
const connectionStore = useConnectionStore()
const { openOrders } = storeToRefs(orderStore)
const { loading, refresh } = useOrderCenterRefresh(computed(() => props.active))

const scope = ref<OpenOrderScope>('limit')
const tableRef = ref<{ selectedRows: Order[]; clearSelection: () => void } | null>(null)
const actionLoading = ref(false)

const scopes: Array<{ key: OpenOrderScope; label: string }> = [
  { key: 'limit', label: '限价委托' },
  { key: 'plan', label: '计划委托' },
  { key: 'tpsl', label: '止盈止损委托' },
]

const scopedOrders = computed(() => filterOpenOrders(openOrders.value, scope.value))

const columnHelper = createColumnHelper<Order>()

async function cancelOne(row: Order): Promise<void> {
  actionLoading.value = true
  try {
    await orderStore.cancelOrder({ symbol: row.symbol, orderId: row.orderId })
    await refreshSyncTask('privatePanels')
    tableRef.value?.clearSelection()
  } catch (error) {
    reportError(error)
  } finally {
    actionLoading.value = false
  }
}

async function batchCancel(rows: Order[]): Promise<void> {
  if (rows.length === 0) {
    return
  }
  actionLoading.value = true
  try {
    for (const row of rows) {
      await orderStore.cancelOrder({ symbol: row.symbol, orderId: row.orderId })
    }
    await refreshSyncTask('privatePanels')
    tableRef.value?.clearSelection()
  } catch (error) {
    reportError(error)
  } finally {
    actionLoading.value = false
  }
}

async function cancelAll(): Promise<void> {
  actionLoading.value = true
  try {
    await orderStore.cancelAllOrders({})
    await refreshSyncTask('privatePanels')
    tableRef.value?.clearSelection()
  } catch (error) {
    reportError(error)
  } finally {
    actionLoading.value = false
  }
}

const columns = [
  columnHelper.accessor('orderId', { header: '订单ID', enableSorting: true }),
  columnHelper.accessor('symbol', { header: '交易对', enableSorting: true }),
  columnHelper.accessor('side', { header: '方向', enableSorting: true }),
  columnHelper.accessor('orderType', { header: '类型', enableSorting: true }),
  columnHelper.accessor('price', { header: '价格', enableSorting: true }),
  columnHelper.accessor('qty', { header: '数量', enableSorting: true }),
  columnHelper.accessor('status', { header: '状态', enableSorting: true }),
  columnHelper.display({
    id: 'actions',
    header: '操作',
    cell: ({ row }) =>
      h(
        AppButton,
        {
          variant: 'ghost',
          size: 'sm',
          disabled: !connectionStore.connected || actionLoading.value,
          onClick: () => void cancelOne(row.original),
        },
        { default: () => '撤单' },
      ),
  }),
]

function rowId(row: Order): string {
  return row.orderId
}
</script>

<template>
  <div class="open-orders-tab">
    <div class="ef-tabs sub-tabs">
      <button
        v-for="item in scopes"
        :key="item.key"
        class="ef-tab ef-motion-tab"
        :class="{ active: scope === item.key }"
        type="button"
        @click="scope = item.key"
      >
        {{ item.label }}
      </button>
    </div>

    <TanstackDataTable
      ref="tableRef"
      :columns="columns"
      :data="scopedOrders"
      :get-row-id="rowId"
      :loading="loading || actionLoading"
      search-placeholder="搜索当前委托…"
      :status-options="[
        { label: 'New', value: 'New' },
        { label: 'Active', value: 'Active' },
        { label: 'PartiallyFilled', value: 'PartiallyFilled' },
      ]"
      :status-accessor="(row) => row.status"
      @refresh="refresh"
    >
      <template #toolbar-actions="{ selectedRows, clearSelection }">
        <AppButton
          size="sm"
          variant="ghost"
          :disabled="!connectionStore.connected || selectedRows.length === 0 || actionLoading"
          @click="batchCancel(selectedRows).then(() => clearSelection())"
        >
          批量撤单
        </AppButton>
        <AppButton
          size="sm"
          variant="danger"
          :disabled="!connectionStore.connected || scopedOrders.length === 0 || actionLoading"
          @click="cancelAll"
        >
          一键全部撤单
        </AppButton>
      </template>
    </TanstackDataTable>
  </div>
</template>

<style scoped>
.open-orders-tab {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.sub-tabs {
  padding: 0 4px;
  border-bottom: none;
}
</style>
