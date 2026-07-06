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
import { NButton, NCheckbox, NInput, NSelect } from 'naive-ui'
import { computed, h, ref, watch } from 'vue'

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
  onGlobalFilterChange: (updater) => {
    globalFilter.value =
      typeof updater === 'function' ? updater(globalFilter.value) : updater
  },
  onSortingChange: (updater) => {
    sorting.value = typeof updater === 'function' ? updater(sorting.value) : updater
  },
  onRowSelectionChange: (updater) => {
    rowSelection.value =
      typeof updater === 'function' ? updater(rowSelection.value) : updater
  },
  getCoreRowModel: getCoreRowModel(),
  getSortedRowModel: getSortedRowModel(),
  getFilteredRowModel: getFilteredRowModel(),
  getPaginationRowModel: getPaginationRowModel(),
  enableRowSelection: props.enableSelection,
  getRowId: (row) => props.getRowId(row),
  initialState: {
    pagination: {
      pageSize: props.pageSize,
    },
  },
})

const selectedRows = computed(() => table.getSelectedRowModel().rows.map((row) => row.original))

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
  <div class="data-table">
    <div class="toolbar">
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
      <div class="toolbar-actions">
        <slot name="toolbar-actions" :selected-rows="selectedRows" :clear-selection="clearSelection" />
        <NButton size="tiny" :loading="loading" @click="emit('refresh')">刷新</NButton>
      </div>
    </div>

    <div class="table-wrap">
      <table class="table">
        <thead>
          <tr v-for="headerGroup in table.getHeaderGroups()" :key="headerGroup.id">
            <th
              v-for="header in headerGroup.headers"
              :key="header.id"
              :style="{ width: header.getSize() !== 150 ? `${header.getSize()}px` : undefined }"
              @click="header.column.getCanSort() ? header.column.toggleSorting() : undefined"
            >
              <div class="th-content" :class="{ sortable: header.column.getCanSort() }">
                <FlexRender
                  v-if="!header.isPlaceholder"
                  :render="header.column.columnDef.header"
                  :props="header.getContext()"
                />
                <span v-if="header.column.getIsSorted() === 'asc'" class="sort">↑</span>
                <span v-else-if="header.column.getIsSorted() === 'desc'" class="sort">↓</span>
              </div>
            </th>
          </tr>
        </thead>
        <tbody>
          <tr v-if="table.getRowModel().rows.length === 0">
            <td :colspan="table.getAllColumns().length" class="empty">暂无数据</td>
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

    <div class="pagination">
      <span class="meta">共 {{ filteredData.length }} 条 · 已选 {{ selectedRows.length }} 条</span>
      <div class="pager">
        <NButton size="tiny" :disabled="!table.getCanPreviousPage()" @click="table.previousPage()">
          上一页
        </NButton>
        <span>{{ pageSummary }}</span>
        <NButton size="tiny" :disabled="!table.getCanNextPage()" @click="table.nextPage()">
          下一页
        </NButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
.data-table {
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 4px;
  flex-shrink: 0;
}

.search {
  width: 180px;
}

.status-filter {
  width: 120px;
}

.toolbar-actions {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 6px;
}

.table-wrap {
  flex: 1;
  min-height: 0;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: var(--ef-radius-lg);
}

.table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
  font-family: var(--font-mono);
  font-variant-numeric: tabular-nums;
}

thead {
  position: sticky;
  top: 0;
  z-index: 1;
  background: var(--bg-tertiary);
}

th,
td {
  padding: 6px 8px;
  border-bottom: 1px solid var(--border-color);
  text-align: left;
  white-space: nowrap;
}

.th-content {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.th-content.sortable {
  cursor: pointer;
}

.sort {
  color: var(--text-secondary);
  font-size: 10px;
}

tbody tr:hover {
  background: color-mix(in srgb, var(--bg-tertiary) 70%, transparent);
}

tbody tr.selected {
  background: color-mix(in srgb, var(--accent-blue) 12%, transparent);
}

.empty {
  text-align: center;
  color: var(--text-secondary);
  padding: 20px 8px;
}

.pagination {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 0 4px;
  flex-shrink: 0;
}

.meta,
.pager {
  font-size: 11px;
  color: var(--text-secondary);
}

.pager {
  display: flex;
  align-items: center;
  gap: 8px;
}
</style>
