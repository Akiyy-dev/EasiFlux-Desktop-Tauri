<script setup lang="ts">
import { createColumnHelper } from '@tanstack/vue-table'
import { h, onMounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import TanstackDataTable from '../../common/TanstackDataTable.vue'
import { tauriInvoke } from '../../../composables/useTauriCommand'
import { useConnectionStore } from '../../../stores/connection'
import { parseClosedPnlRecords, type ClosedPnlRecord } from '../../../utils/tradingRecords'
import { reportError } from '../../../services/errorService'

const props = defineProps<{
  active: boolean
}>()

const connectionStore = useConnectionStore()
const { connected } = storeToRefs(connectionStore)

const records = ref<ClosedPnlRecord[]>([])
const loading = ref(false)

const columnHelper = createColumnHelper<ClosedPnlRecord>()

const columns = [
  columnHelper.accessor('symbol', { header: '交易对', enableSorting: true }),
  columnHelper.accessor('side', { header: '方向', enableSorting: true }),
  columnHelper.accessor('closedSize', { header: '平仓量', enableSorting: true }),
  columnHelper.accessor('avgEntryPrice', { header: '开仓均价', enableSorting: true }),
  columnHelper.accessor('avgExitPrice', { header: '平仓均价', enableSorting: true }),
  columnHelper.accessor('closedPnl', {
    header: '平仓盈亏',
    enableSorting: true,
    cell: (info) => {
      const value = info.getValue()
      const num = Number.parseFloat(value)
      const cls = num > 0 ? 'text-up' : num < 0 ? 'text-down' : ''
      return h('span', { class: cls }, value)
    },
  }),
  columnHelper.accessor('closedTime', {
    header: '平仓时间',
    enableSorting: true,
    cell: (info) => {
      const value = info.getValue()
      if (!value) {
        return '--'
      }
      return new Date(value).toLocaleString()
    },
  }),
]

async function refresh(): Promise<void> {
  if (!connectionStore.connected) {
    return
  }
  loading.value = true
  try {
    const payload = await tauriInvoke<unknown>('fetch_closed_pnl', {
      symbol: null,
      coin: null,
      startTime: null,
      endTime: null,
      limit: 100,
      cursor: null,
    })
    records.value = parseClosedPnlRecords(payload)
  } catch (error) {
    reportError(error)
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  void refresh()
})

watch(connected, (isConnected) => {
  if (isConnected) {
    void refresh()
  }
})

watch(
  () => props.active,
  (isActive) => {
    if (isActive) {
      void refresh()
    }
  },
)

function rowId(row: ClosedPnlRecord): string {
  return `${row.symbol}:${row.closedTime}:${row.closedPnl}`
}
</script>

<template>
  <TanstackDataTable
    :columns="columns"
    :data="records"
    :get-row-id="rowId"
    :loading="loading"
    search-placeholder="搜索平仓盈亏…"
    :enable-selection="false"
    @refresh="refresh"
  />
</template>
