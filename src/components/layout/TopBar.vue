<script setup lang="ts">
import { Bell, User, Wifi } from 'lucide-vue-next'
import { storeToRefs } from 'pinia'
import { nextTick, ref, watch } from 'vue'
import ConnectionStatus from '../common/ConnectionStatus.vue'
import NotificationPopover from '../notifications/NotificationPopover.vue'
import { AppButton, AppIcon, MonoValue } from '../ui'
import { useAppStore } from '../../stores/app'
import type { NotificationUiAction } from '../../types/notification'

const props = defineProps<{
  title: string
}>()

const appStore = useAppStore()
const { version } = storeToRefs(appStore)
const showNotifications = ref(false)
const bell = ref<{ focus: () => void } | null>(null)

const emit = defineEmits<{ action: [action: NotificationUiAction] }>()

watch(showNotifications, async (isOpen, wasOpen) => {
  if (!isOpen && wasOpen) {
    await nextTick()
    bell.value?.focus()
  }
})

function notificationLabel(unreadCount: number | null): string {
  if (unreadCount === null) return '通知，未读数量未知'
  if (unreadCount === 0) return '通知，无未读'
  return `通知，${unreadCount} 条未读`
}

function badgeLabel(unreadCount: number | null): string | null {
  if (unreadCount === null || unreadCount === 0) return null
  return unreadCount > 99 ? '99+' : String(unreadCount)
}
</script>

<template>
  <header class="top-bar" aria-label="TopBar">
    <div class="left">
      <div class="brand">
        <span class="ef-text-title">EasiFlux</span>
        <MonoValue class="version ef-text-caption" size="sm">
          v{{ version }}
        </MonoValue>
      </div>
      <div class="divider" aria-hidden="true" />
      <div class="page">
        <span class="ef-text-body page-title">{{ props.title }}</span>
      </div>
    </div>
    <div class="right">
      <ConnectionStatus />
      <AppButton variant="ghost" size="sm" icon-only title="网络（占位）" disabled>
        <AppIcon :icon="Wifi" :size="16" />
      </AppButton>
      <NotificationPopover v-model:show="showNotifications" @action="emit('action', $event)">
        <template #trigger="{ unreadCount }">
          <button
            ref="bell"
            data-testid="notification-bell"
            class="top-bar__notification-bell ef-btn ef-motion-hover ef-motion-press ef-focus-ring ef-btn-ghost ef-btn-sm ef-btn-icon"
            type="button"
            :aria-label="notificationLabel(unreadCount)"
            aria-haspopup="dialog"
            :aria-expanded="showNotifications"
            aria-controls="notification-popover-dialog"
            @click="showNotifications = !showNotifications"
          >
            <AppIcon :icon="Bell" :size="16" />
            <span v-if="badgeLabel(unreadCount)" class="top-bar__notification-badge" aria-hidden="true">{{ badgeLabel(unreadCount) }}</span>
            <span class="top-bar__sr-only" aria-live="polite">{{ unreadCount === null ? '未读通知数量未知' : `未读通知 ${unreadCount} 条` }}</span>
          </button>
        </template>
      </NotificationPopover>
      <AppButton variant="ghost" size="sm" icon-only title="用户（占位）" disabled>
        <AppIcon :icon="User" :size="16" />
      </AppButton>
    </div>
  </header>
</template>

<style scoped>
.top-bar {
  min-height: clamp(46px, 2.4rem + 0.75vw, 58px);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 var(--ef-space-3);
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: var(--ef-radius-lg);
  flex-shrink: 0;
}

.left {
  display: flex;
  align-items: center;
  gap: var(--ef-space-3);
  min-width: 0;
}

.brand {
  display: flex;
  align-items: baseline;
  gap: var(--ef-space-2);
}

.version {
  color: var(--text-secondary);
}

.divider {
  width: 1px;
  height: 18px;
  background: var(--border);
}

.page {
  min-width: 0;
}

.page-title {
  font-weight: var(--ef-text-label-weight);
}

.right {
  display: flex;
  align-items: center;
  gap: var(--ef-space-2);
  flex-shrink: 0;
}

.top-bar__notification-bell { position: relative; }
.top-bar__notification-badge { position: absolute; top: -5px; right: -7px; min-width: 16px; padding: 0 4px; border-radius: 999px; background: var(--danger); color: #fff; font-size: 10px; line-height: 16px; }
.top-bar__sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; }

</style>
