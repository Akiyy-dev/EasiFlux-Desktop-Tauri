<script setup lang="ts" generic="T">
import {
  FlexRender,
  createColumnHelper,
  getCoreRowModel,
  getFilteredRowModel,
  getPaginationRowModel,
  getSortedRowModel,
  useVueTable,
  type ColumnDef,
  type RowSelectionState,
  type SortingState,
} from '@tanstack/vue-table'
import { NCheckbox, NInput, NSelect } from 'naive-ui'
import { computed, h, ref, watch } from 'vue'
import AppButton from '../ui/AppButton.vue'

const props = withDefaults(
  defineProps<{
    /** TanStack column cell value types vary per column. */
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    columns: ColumnDef<T, any>[]
    data: T[]
    getRowId: (row: T) => string
    loading?: boolean
    searchPlaceholder?: string
    pageSize?: number
    enableSelection?: boolean
    statusOptions?: Array<{ label: string; value: string }>
    statusAccessor?: (row: T) => string
  }>(),
  {
    loading: false,
    searchPlaceholder: '搜索…',
    pageSize: 8,
    enableSelection: true,
    statusOptions: undefined,
    statusAccessor: undefined,
  },
)

const emit = defineEmits<{
  refresh: []
}>()

const globalFilter = ref('')
const sorting = ref<SortingState>([])
const rowSelection = ref<RowSelectionState>({})
const statusFilter = ref<string | null>(null)

const columnHelper = createColumnHelper<T>()

const selectionColumn = columnHelper.display({
  id: 'select',
  header: ({ table }) =>
    h(NCheckbox, {
      checked: table.getIsAllPageRowsSelected(),
      indeterminate: table.getIsSomePageRowsSelected(),
      onUpdateChecked: (value: boolean) => table.toggleAllPageRowsSelected(!!value),
    }),
  cell: ({ row }) =>
    h(NCheckbox, {
      checked: row.getIsSelected(),
      onUpdateChecked: (value: boolean) => row.toggleSelected(!!value),
    }),
  enableSorting: false,
  size: 36,
})

const tableColumns = computed(() => {
  if (!props.enableSelection) {
    return props.columns
  }
  return [selectionColumn, ...props.columns]
})

const filteredData = computed(() => {
  if (!statusFilter.value || !props.statusAccessor) {
    return props.data
  }
  return props.data.filter((row) => props.statusAccessor!(row) === statusFilter.value)
})

const table = useVueTable({
  get data() {
    return filteredData.value
  },
  get columns() {
    return tableColumns.value
  },
  state: {
    get globalFilter() {
      return globalFilter.value
    },
    get sorting() {
      return sorting.value
    },
    get rowSelection() {
      return rowSelection.value
    },
  },
  onGlobalFilterChange: (value) => {
    globalFilter.value = value
  },
  onSortingChange: (updater) => {
    sorting.value = typeof updater === 'function' ? updater(sorting.value) : updater
  },
  onRowSelectionChange: (updater) => {
    rowSelection.value = typeof updater === 'function' ? updater(rowSelection.value) : updater
  },
  getCoreRowModel: getCoreRowModel(),
  getFilteredRowModel: getFilteredRowModel(),
  getSortedRowModel: getSortedRowModel(),
  getPaginationRowModel: getPaginationRowModel(),
  getRowId: (row) => props.getRowId(row),
  enableRowSelection: true,
  initialState: {
    pagination: {
      pageSize: props.pageSize,
    },
  },
})

const selectedRows = computed(() =>
  table.getSelectedRowModel().rows.map((row) => row.original),
)

const pageSummary = computed(() => {
  const page = table.getState().pagination.pageIndex + 1
  const total = table.getPageCount()
  return `${page} / ${Math.max(total, 1)}`
})

function clearSelection(): void {
  table.resetRowSelection()
}

watch(
  () => props.data,
  () => {
    clearSelection()
  },
)

defineExpose({
  selectedRows,
  clearSelection,
})
</script>

<template>
  <div class="ef-table-root">
    <div class="ef-table-toolbar">
      <NInput
        v-model:value="globalFilter"
        size="small"
        :placeholder="searchPlaceholder"
        clearable
        class="search"
      />
      <NSelect
        v-if="statusOptions && statusOptions.length > 0"
        v-model:value="statusFilter"
        size="small"
        clearable
        placeholder="状态筛选"
        :options="statusOptions"
        class="status-filter"
      />
      <div class="ef-table-toolbar-actions">
        <slot name="toolbar-actions" :selected-rows="selectedRows" :clear-selection="clearSelection" />
        <AppButton size="sm" variant="ghost" :loading="loading" @click="emit('refresh')">
          刷新
        </AppButton>
      </div>
    </div>

    <div class="ef-table-wrap">
      <table class="ef-table">
        <thead>
          <tr v-for="headerGroup in table.getHeaderGroups()" :key="headerGroup.id">
            <th
              v-for="header in headerGroup.headers"
              :key="header.id"
              :style="{ width: header.getSize() !== 150 ? `${header.getSize()}px` : undefined }"
              @click="header.column.getCanSort() ? header.column.toggleSorting() : undefined"
            >
              <div
                class="ef-table-th-content"
                :class="{ sortable: header.column.getCanSort() }"
              >
                <FlexRender
                  v-if="!header.isPlaceholder"
                  :render="header.column.columnDef.header"
                  :props="header.getContext()"
                />
                <span v-if="header.column.getIsSorted() === 'asc'" class="ef-table-sort">↑</span>
                <span v-else-if="header.column.getIsSorted() === 'desc'" class="ef-table-sort">↓</span>
              </div>
            </th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="table.getRowModel().rows.length === 0">
            <td :colspan="table.getAllColumns().length" class="ef-table-empty">暂无数据</td>
          </tr>
          <tr
            v-for="row in table.getRowModel().rows"
            :key="row.id"
            :class="{ selected: row.getIsSelected() }"
          >
            <td v-for="cell in row.getVisibleCells()" :key="cell.id">
              <FlexRender :render="cell.column.columnDef.cell" :props="cell.getContext()" />
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div class="ef-table-pagination">
      <span>共 {{ filteredData.length }} 条 · 已选 {{ selectedRows.length }} 条</span>
      <div class="ef-table-pager">
        <AppButton
          size="sm"
          variant="ghost"
          :disabled="!table.getCanPreviousPage()"
          @click="table.previousPage()"
        >
          上一页
        </AppButton>
        <span>{{ pageSummary }}</span>
        <AppButton
          size="sm"
          variant="ghost"
          :disabled="!table.getCanNextPage()"
          @click="table.nextPage()"
        >
          下一页
        </AppButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
.search {
  width: 180px;
}

.status-filter {
  width: 120px;
}
</style>
