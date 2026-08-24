<script setup lang="ts">
import { NPopover } from 'naive-ui'
import { nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import NotificationList from './NotificationList.vue'
import { useNotificationStore } from '../../stores/notification'
import { useTimeStore } from '../../stores/time'
import type { NotificationRecord, NotificationUiAction } from '../../types/notification'

const props = defineProps<{
  show: boolean
  triggerElement?: { contains: (target: unknown) => boolean } | null
}>()
const emit = defineEmits<{
  'update:show': [show: boolean]
  action: [action: NotificationUiAction]
}>()

const notificationStore = useNotificationStore()
const timeStore = useTimeStore()
const {
  accountId, error, filter, initialLoading, items, loadingMore, markAllReadPending,
  markReadPendingIds, nextCursor, pageError, removePendingIds, unreadCount,
} = storeToRefs(notificationStore)
const panel = ref<{ focus: () => void; contains: (target: unknown) => boolean } | null>(null)

function close(): void {
  if (props.show) emit('update:show', false)
}

function onEscape(event: { key: string; preventDefault: () => void }): void {
  if (event.key === 'Escape') {
    event.preventDefault()
    close()
  }
}

function onOutsidePointer(event: { target: unknown }): void {
  const target = event.target
  if (!panel.value || panel.value.contains(target) || props.triggerElement?.contains(target)) return
  close()
}

function addDocumentListeners(): void {
  globalThis.document.addEventListener('keydown', onEscape)
  globalThis.document.addEventListener('mousedown', onOutsidePointer)
}

function removeDocumentListeners(): void {
  globalThis.document.removeEventListener('keydown', onEscape)
  globalThis.document.removeEventListener('mousedown', onOutsidePointer)
}

watch(() => props.show, async (isOpen, wasOpen) => {
  if (!isOpen) {
    if (wasOpen) removeDocumentListeners()
    return
  }
  addDocumentListeners()
  await notificationStore.open()
  await nextTick()
  panel.value?.focus()
}, { immediate: true })

onBeforeUnmount(removeDocumentListeners)

async function activate(record: NotificationRecord): Promise<void> {
  await notificationStore.markRead(record.id)
  if (record.action) emit('action', record.action)
}

function openNotificationSettings(): void {
  emit('update:show', false)
  emit('action', { type: 'openNotificationSettings' })
}
</script>

<template>
  <NPopover
    trigger="manual"
    placement="bottom-end"
    :show="show"
    @update:show="emit('update:show', $event)"
  >
    <template #trigger>
      <slot name="trigger" :unread-count="unreadCount" />
    </template>
    <section
      id="notification-popover-dialog"
      ref="panel"
      class="notification-popover"
      role="dialog"
      aria-modal="false"
      aria-labelledby="notification-popover-title"
      tabindex="-1"
    >
      <header class="notification-popover__header">
        <h2 id="notification-popover-title">
          通知
        </h2>
      </header>
      <NotificationList
        :items="items"
        :filter="filter"
        :unread-count="unreadCount"
        :initial-loading="initialLoading"
        :loading-more="loadingMore"
        :error="error"
        :page-error="pageError"
        :next-cursor="nextCursor"
        :now="timeStore.serverNow"
        :mark-read-pending-ids="markReadPendingIds"
        :remove-pending-ids="removePendingIds"
        :mark-all-read-pending="markAllReadPending"
        @filter="notificationStore.setFilter"
        @retry-first="notificationStore.reload"
        @retry-page="notificationStore.loadMore"
        @load-more="notificationStore.loadMore"
        @mark-all="notificationStore.markAllRead"
        @activate="activate"
        @delete="notificationStore.remove"
      />
      <footer class="notification-popover__footer">
        <span>{{ accountId === null ? '仅显示 Global 通知' : '显示当前账户 + Global 通知' }}</span>
        <button data-testid="notification-settings" type="button" @click="openNotificationSettings">
          通知设置
        </button>
      </footer>
    </section>
  </NPopover>
</template>

<style src="./NotificationPopover.css"></style>
