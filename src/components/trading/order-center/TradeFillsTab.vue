<script setup lang="ts">
import { createColumnHelper } from '@tanstack/vue-table'
import { onMounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import TanstackDataTable from '../../common/TanstackDataTable.vue'
import { tauriInvoke } from '../../../composables/useTauriCommand'
import { useConnectionStore } from '../../../stores/connection'
import { parseTradeFills, type TradeFill } from '../../../utils/tradingRecords'
import { reportError } from '../../../services/errorService'

const props = defineProps<{
  active: boolean
}>()

const connectionStore = useConnectionStore()
const { connected } = storeToRefs(connectionStore)

const fills = ref<TradeFill[]>([])
const loading = ref(false)

const columnHelper = createColumnHelper<TradeFill>()

const columns = [
  columnHelper.accessor('fillId', { header: '成交ID', enableSorting: true }),
  columnHelper.accessor('symbol', { header: '交易对', enableSorting: true }),
  columnHelper.accessor('side', { header: '方向', enableSorting: true }),
  columnHelper.accessor('price', { header: '成交价', enableSorting: true }),
  columnHelper.accessor('qty', { header: '成交量', enableSorting: true }),
  columnHelper.accessor('fee', { header: '手续费', enableSorting: true }),
  columnHelper.accessor('execTime', {
    header: '成交时间',
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
    const payload = await tauriInvoke<unknown>('fetch_trade_fills', {
      symbol: null,
      coin: null,
      orderId: null,
      startTime: null,
      endTime: null,
      execType: null,
      limit: 100,
      cursor: null,
    })
    fills.value = parseTradeFills(payload)
  } catch (error) {
    reportError(error)
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  if (props.active) {
    void refresh()
  }
})

watch(connected, (isConnected) => {
  if (isConnected && props.active) {
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

function rowId(row: TradeFill): string {
  return row.fillId
}
</script>

<template>
  <TanstackDataTable
    :columns="columns"
    :data="fills"
    :get-row-id="rowId"
    :loading="loading"
    search-placeholder="搜索成交记录…"
    :enable-selection="false"
    @refresh="refresh"
  />
</template>
