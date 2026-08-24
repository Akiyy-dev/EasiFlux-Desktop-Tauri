<script setup lang="ts">
import { AlertCircle, CheckCircle2, CircleAlert, Info, ShieldAlert, Trash2 } from 'lucide-vue-next'
import { computed } from 'vue'
import AppIcon from '../ui/AppIcon.vue'
import type { NotificationRecord, NotificationSeverity } from '../../types/notification'

const props = defineProps<{
  record: NotificationRecord
  markReadPending: boolean
  deletePending: boolean
  now: number
}>()

const emit = defineEmits<{
  activate: [record: NotificationRecord]
  delete: [id: string]
}>()

const pending = computed(() => props.markReadPending || props.deletePending)
const unread = computed(() => props.record.readAtMs === undefined)
const relativeTime = computed(() => {
  const elapsed = Math.max(0, props.now - props.record.createdAtMs)
  if (elapsed < 60_000) return '刚刚'
  if (elapsed < 3_600_000) return `${Math.floor(elapsed / 60_000)} 分钟前`
  if (elapsed < 86_400_000) return `${Math.floor(elapsed / 3_600_000)} 小时前`
  return `${Math.floor(elapsed / 86_400_000)} 天前`
})

const categoryLabels: Record<NotificationRecord['category'], string> = {
  trading: '交易',
  riskAccount: '风险与账户',
  connectionSystem: '连接与系统',
}

const severityLabels: Record<NotificationSeverity, string> = {
  success: '成功',
  info: '提示',
  warning: '警告',
  error: '错误',
  critical: '严重',
}

const severityIcons = {
  success: CheckCircle2,
  info: Info,
  warning: CircleAlert,
  error: AlertCircle,
  critical: ShieldAlert,
} as const

const actionHint = computed(() => {
  switch (props.record.action?.type) {
    case 'openTrading': return '前往交易'
    case 'openAccountSettings': return '前往账户设置'
    case 'openGeneralSettings': return '前往通用设置'
    default: return null
  }
})

function activate(): void {
  if (!pending.value) emit('activate', props.record)
}

function handlePrimaryKeydown(event: { key: string; preventDefault: () => void }): void {
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault()
    activate()
    return
  }
  if (event.key === 'Delete') {
    event.preventDefault()
    if (!pending.value) emit('delete', props.record.id)
  }
}
</script>

<template>
  <article
    class="notification-item"
    :data-unread="unread"
    :aria-busy="pending || undefined"
  >
    <button
      class="notification-item__primary"
      type="button"
      :disabled="pending"
      @click="activate"
      @keydown="handlePrimaryKeydown"
    >
      <span class="notification-item__headline">
        <AppIcon :icon="severityIcons[record.severity]" :size="16" aria-hidden="true" />
        <span class="notification-item__title">{{ record.content.fallbackTitle || '通知' }}</span>
        <span v-if="unread" class="notification-item__unread">未读</span>
      </span>
      <span class="notification-item__body">{{ record.content.fallbackBody || '暂无详情' }}</span>
      <span class="notification-item__meta">
        <span>{{ categoryLabels[record.category] }}</span>
        <span>{{ severityLabels[record.severity] }}</span>
        <time :datetime="new Date(record.createdAtMs).toISOString()" :title="new Date(record.createdAtMs).toLocaleString('zh-CN')">
          {{ relativeTime }}
        </time>
        <span v-if="record.occurrenceCount > 1">{{ record.occurrenceCount }} 次</span>
      </span>
      <span v-if="actionHint" class="notification-item__action-hint">{{ actionHint }}</span>
    </button>
    <button
      class="notification-item__delete"
      type="button"
      aria-label="删除通知"
      :disabled="pending"
      @click="emit('delete', record.id)"
    >
      <AppIcon :icon="Trash2" :size="16" aria-hidden="true" />
    </button>
  </article>
</template>

<style scoped>
.notification-item { display: flex; gap: var(--ef-space-2); padding: var(--ef-space-3); border-bottom: 1px solid var(--border); }
.notification-item[data-unread="true"] { border-left: 3px solid var(--accent); }
.notification-item__primary { flex: 1; min-width: 0; border: 0; padding: 0; background: transparent; color: inherit; text-align: left; cursor: pointer; }
.notification-item__headline, .notification-item__meta { display: flex; align-items: center; gap: var(--ef-space-2); }
.notification-item__title { font-weight: var(--ef-text-label-weight); }
.notification-item__unread { font-size: var(--ef-text-caption-size); font-weight: var(--ef-text-label-weight); }
.notification-item__body { display: block; margin-top: var(--ef-space-1); color: var(--text-secondary); }
.notification-item__meta, .notification-item__action-hint { display: block; margin-top: var(--ef-space-1); color: var(--text-secondary); font-size: var(--ef-text-caption-size); }
.notification-item__delete { align-self: flex-start; border: 0; background: transparent; color: var(--text-secondary); cursor: pointer; opacity: 0; }
.notification-item:hover .notification-item__delete, .notification-item:focus-within .notification-item__delete { opacity: 1; }
</style>
