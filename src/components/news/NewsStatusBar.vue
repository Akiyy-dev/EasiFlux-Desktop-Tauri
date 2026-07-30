<script setup lang="ts">
import { computed } from 'vue'
import type { NewsStatusKind, NewsStatusSnapshot } from '../../types/news'
import AppButton from '../ui/AppButton.vue'
import './newsStatusBar.css'

type RecoveryAction = 'recheckCredentials' | 'retrySync'

interface StatusPresentation {
  label: string
  tone: 'neutral' | 'live' | 'warning' | 'danger'
  action?: RecoveryAction
}

const props = defineProps<{
  status: NewsStatusSnapshot
  hasCachedMessages: boolean
}>()

const emit = defineEmits<{
  recheckCredentials: []
  retrySync: []
}>()

const presentations: Record<NewsStatusKind, StatusPresentation> = {
  deploymentMisconfigured: { label: '新闻数据源未内置，请使用正确的安装包', tone: 'danger' },
  notConfigured: { label: '新闻服务未配置', tone: 'warning', action: 'recheckCredentials' },
  credentialStoreUnavailable: { label: '凭据存储暂不可用', tone: 'danger', action: 'recheckCredentials' },
  initialSync: { label: '正在同步历史新闻', tone: 'neutral' },
  live: { label: '实时', tone: 'live' },
  retrying: { label: '新闻服务暂时不可用，正在重试', tone: 'warning' },
  credentialInvalid: { label: '新闻服务凭据无效', tone: 'danger', action: 'recheckCredentials' },
  contractError: { label: '新闻服务协议异常', tone: 'danger', action: 'retrySync' },
  storageError: { label: '新闻缓存暂不可用', tone: 'danger', action: 'retrySync' },
  stopped: { label: '新闻服务已停止', tone: 'neutral' },
}

const presentation = computed(() => presentations[props.status.kind])
const statusLabel = computed(() => props.status.message?.trim() || presentation.value.label)
const showCachedCopy = computed(() => props.hasCachedMessages && props.status.kind !== 'live')
const retryLabel = computed(() => {
  if (props.status.kind !== 'retrying' || !props.status.retryAt) return null
  const retryAt = new Date(props.status.retryAt)
  if (Number.isNaN(retryAt.getTime())) return null
  const pad = (value: number) => String(value).padStart(2, '0')
  return `下次重试 ${pad(retryAt.getHours())}:${pad(retryAt.getMinutes())}:${pad(retryAt.getSeconds())}`
})

function recover(): void {
  if (presentation.value.action === 'recheckCredentials') emit('recheckCredentials')
  if (presentation.value.action === 'retrySync') emit('retrySync')
}
</script>

<template>
  <section
    class="news-status-bar"
    :class="`is-${presentation.tone}`"
    role="status"
    aria-live="polite"
  >
    <span class="news-status-indicator" aria-hidden="true" />
    <div class="news-status-copy">
      <span class="news-status-label">{{ statusLabel }}</span>
      <span v-if="status.kind === 'initialSync'" class="news-status-detail">
        已同步 {{ status.syncedCount }} 条
      </span>
      <span v-if="showCachedCopy" class="news-status-detail">已缓存新闻仍可阅读</span>
      <span v-if="retryLabel" class="news-status-detail">{{ retryLabel }}</span>
    </div>
    <AppButton
      v-if="presentation.action"
      class="news-status-action"
      variant="ghost"
      size="sm"
      @click="recover"
    >
      {{ presentation.action === 'recheckCredentials' ? '重新检查' : '重试同步' }}
    </AppButton>
  </section>
</template>
