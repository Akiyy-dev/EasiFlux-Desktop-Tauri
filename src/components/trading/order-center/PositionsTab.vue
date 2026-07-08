<script setup lang="ts">
import { createColumnHelper } from '@tanstack/vue-table'
import { computed, h } from 'vue'
import { storeToRefs } from 'pinia'
import TanstackDataTable from '../../common/TanstackDataTable.vue'
import { useOrderCenterRefresh } from '../../../composables/useOrderCenterRefresh'
import { usePositionStore } from '../../../stores/position'
import type { Position } from '../../../types/models'

const props = defineProps<{
  active: boolean
}>()

const positionStore = usePositionStore()
const { positions } = storeToRefs(positionStore)
const { loading, refresh } = useOrderCenterRefresh(computed(() => props.active))

const columnHelper = createColumnHelper<Position>()

const columns = [
  columnHelper.accessor('symbol', { header: '交易对', enableSorting: true }),
  columnHelper.accessor('side', { header: '方向', enableSorting: true }),
  columnHelper.accessor('size', { header: '数量', enableSorting: true }),
  columnHelper.accessor('entryPrice', { header: '开仓价', enableSorting: true }),
  columnHelper.accessor('leverage', { header: '杠杆', enableSorting: true }),
  columnHelper.accessor('unrealisedPnl', {
    header: '未实现盈亏',
    enableSorting: true,
    cell: (info) => {
      const value = info.getValue()
      const num = Number.parseFloat(value)
      const cls = num > 0 ? 'text-up' : num < 0 ? 'text-down' : ''
      return h('span', { class: cls }, value)
    },
  }),
]

function rowId(row: Position): string {
  return `${row.symbol}:${row.positionIdx ?? 0}`
}
</script>

<template>
  <TanstackDataTable
    :columns="columns"
    :data="positions"
    :get-row-id="rowId"
    :loading="loading"
    search-placeholder="搜索持仓…"
    @refresh="refresh"
  />
</template>
