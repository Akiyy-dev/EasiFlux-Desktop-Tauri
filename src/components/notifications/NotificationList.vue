<script setup lang="ts">
import NotificationItem from './NotificationItem.vue'
import type { NotificationFilter, NotificationRecord } from '../../types/notification'

const props = defineProps<{
  items: readonly NotificationRecord[]
  filter: NotificationFilter
  unreadCount: number | null
  initialLoading: boolean
  loadingMore: boolean
  error: string | null
  pageError: string | null
  nextCursor?: string
  now: number
  markReadPendingIds: ReadonlySet<string>
  removePendingIds: ReadonlySet<string>
  markAllReadPending: boolean
}>()

const emit = defineEmits<{
  filter: [filter: NotificationFilter]
  retryFirst: []
  retryPage: []
  loadMore: []
  markAll: []
  activate: [record: NotificationRecord]
  delete: [id: string]
}>()

function isToday(createdAtMs: number): boolean {
  const boundary = new Date(props.now)
  boundary.setHours(0, 0, 0, 0)
  return createdAtMs >= boundary.getTime()
}

function itemsFor(today: boolean): NotificationRecord[] {
  return props.items.filter((item) => isToday(item.createdAtMs) === today)
}
</script>

<template>
  <section class="notification-list" aria-label="通知列表">
    <div class="notification-list__toolbar">
      <div role="group" aria-label="通知筛选">
        <button
          data-testid="notification-filter-all"
          type="button"
          :aria-pressed="filter === 'all'"
          @click="emit('filter', 'all')"
        >
          全部
        </button>
        <button
          data-testid="notification-filter-unread"
          type="button"
          :aria-pressed="filter === 'unread'"
          @click="emit('filter', 'unread')"
        >
          未读
        </button>
      </div>
      <button
        data-testid="notification-mark-all"
        type="button"
        :disabled="unreadCount === null || unreadCount === 0 || initialLoading || markAllReadPending"
        @click="emit('markAll')"
      >
        全部已读
      </button>
    </div>

    <div v-if="initialLoading && items.length === 0" data-testid="notification-skeleton" class="notification-list__skeleton" aria-label="正在加载通知">
      <span v-for="index in 3" :key="index" class="notification-list__skeleton-row" />
    </div>
    <div v-else-if="items.length === 0 && error" class="notification-list__error" role="status" aria-live="polite">
      <span>{{ error }}</span>
      <button data-testid="notification-retry-first" type="button" @click="emit('retryFirst')">
        重试
      </button>
    </div>
    <p v-else-if="items.length === 0" class="notification-list__empty">
      {{ filter === 'all' ? '暂无通知' : '未读通知为空' }}
    </p>
    <div v-else class="notification-list__items">
      <div v-if="error" class="notification-list__error" role="status" aria-live="polite">
        <span>{{ error }}</span>
        <button data-testid="notification-retry-first" type="button" @click="emit('retryFirst')">
          重试
        </button>
      </div>
      <template v-for="group in [{ label: '今天', values: itemsFor(true) }, { label: '更早', values: itemsFor(false) }]" :key="group.label">
        <h3 v-if="group.values.length" class="notification-list__group-title">
          {{ group.label }}
        </h3>
        <NotificationItem
          v-for="item in group.values"
          :key="item.id"
          :data-notification-id="item.id"
          :record="item"
          :mark-read-pending="markReadPendingIds.has(item.id)"
          :delete-pending="removePendingIds.has(item.id)"
          :now="now"
          @activate="emit('activate', $event)"
          @delete="emit('delete', $event)"
        />
      </template>
      <div v-if="pageError" class="notification-list__error" role="status" aria-live="polite">
        <span>{{ pageError }}</span>
        <button data-testid="notification-retry-page" type="button" @click="emit('retryPage')">
          重试
        </button>
      </div>
      <button
        v-if="nextCursor"
        data-testid="notification-load-more"
        type="button"
        :disabled="loadingMore"
        @click="emit('loadMore')"
      >
        {{ loadingMore ? '加载中…' : '加载更多' }}
      </button>
    </div>
  </section>
</template>

<style scoped>
.notification-list { display: flex; min-height: 0; flex: 1; flex-direction: column; }
.notification-list__toolbar { display: flex; align-items: center; justify-content: space-between; gap: var(--ef-space-2); padding: var(--ef-space-2) var(--ef-space-3); }
.notification-list__items { min-height: 0; overflow-y: auto; }
.notification-list__group-title { margin: 0; padding: var(--ef-space-2) var(--ef-space-3); color: var(--text-secondary); font-size: var(--ef-text-caption-size); }
.notification-list__skeleton { display: grid; gap: var(--ef-space-2); padding: var(--ef-space-3); }
.notification-list__skeleton-row { display: block; height: 72px; border-radius: var(--ef-radius-md); background: var(--border); }
.notification-list__empty, .notification-list__error { padding: var(--ef-space-4); color: var(--text-secondary); text-align: center; }
.notification-list__error { display: grid; gap: var(--ef-space-2); }
</style>
