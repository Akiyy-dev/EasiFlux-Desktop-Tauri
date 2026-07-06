<script setup lang="ts">
import { createColumnHelper } from '@tanstack/vue-table'
import { NButton } from 'naive-ui'
import { computed, h, ref } from 'vue'
import { storeToRefs } from 'pinia'
import TanstackDataTable from '../../common/TanstackDataTable.vue'
import { useOrderCenterRefresh } from '../../../composables/useOrderCenterRefresh'
import { useConnectionStore } from '../../../stores/connection'
import { useLogStore } from '../../../stores/log'
import { useOrderStore } from '../../../stores/order'
import { refreshPrivatePanels } from '../../../stores/privatePanels'
import type { Order } from '../../../types/models'
import { filterOpenOrders, type OpenOrderScope } from '../../../utils/orderFilters'

const props = defineProps<{
  active: boolean
}>()

const orderStore = useOrderStore()
const connectionStore = useConnectionStore()
const logStore = useLogStore()
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
    await refreshPrivatePanels()
    tableRef.value?.clearSelection()
  } catch (error) {
    logStore.setError(error instanceof Error ? error.message : String(error))
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
    await refreshPrivatePanels()
    tableRef.value?.clearSelection()
  } catch (error) {
    logStore.setError(error instanceof Error ? error.message : String(error))
  } finally {
    actionLoading.value = false
  }
}

async function cancelAll(): Promise<void> {
  actionLoading.value = true
  try {
    await orderStore.cancelAllOrders({})
    await refreshPrivatePanels()
    tableRef.value?.clearSelection()
  } catch (error) {
    logStore.setError(error instanceof Error ? error.message : String(error))
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
        NButton,
        {
          size: 'tiny',
          quaternary: true,
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
    <div class="sub-tabs">
      <button
        v-for="item in scopes"
        :key="item.key"
        class="sub-tab"
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
        <NButton
          size="tiny"
          :disabled="!connectionStore.connected || selectedRows.length === 0 || actionLoading"
          @click="batchCancel(selectedRows).then(() => clearSelection())"
        >
          批量撤单
        </NButton>
        <NButton
          size="tiny"
          type="error"
          secondary
          :disabled="!connectionStore.connected || scopedOrders.length === 0 || actionLoading"
          @click="cancelAll"
        >
          一键全部撤单
        </NButton>
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
  display: flex;
  gap: 4px;
  padding: 0 4px;
  flex-shrink: 0;
}

.sub-tab {
  background: transparent;
  border: 1px solid transparent;
  color: var(--text-secondary);
  padding: 4px 10px;
  border-radius: 8px;
  cursor: pointer;
  font-size: 11px;
}

.sub-tab:hover {
  color: var(--text-primary);
  background: var(--bg-tertiary);
}

.sub-tab.active {
  color: var(--text-primary);
  border-color: var(--border-color);
  background: var(--bg-tertiary);
}
</style>
