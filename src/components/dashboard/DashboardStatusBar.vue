<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import { Wifi } from 'lucide-vue-next'
import AppCard from '../ui/AppCard.vue'
import AppIcon from '../ui/AppIcon.vue'
import MonoValue from '../ui/MonoValue.vue'
import { useConnectionStore } from '../../stores/connection'

const props = defineProps<{
  version: string
}>()

const connectionStore = useConnectionStore()
const { status, wsStatus } = storeToRefs(connectionStore)

const apiLabel = computed(() => {
  const labels: Record<string, string> = {
    disconnected: 'API 未连接',
    connecting: 'API 连接中',
    connected: 'API 已连接',
    error: 'API 异常',
  }
  return labels[status.value] ?? status.value
})

const wsLabel = computed(() => {
  const labels: Record<string, string> = {
    disconnected: 'WS 断开',
    connecting: 'WS 连接中',
    connected: 'WS 已连接',
    error: 'WS 异常',
  }
  return labels[wsStatus.value] ?? wsStatus.value
})

const networkLabel = computed(() =>
  status.value === 'connected' || wsStatus.value === 'connected' ? '网络正常' : '等待连接',
)
</script>

<template>
  <AppCard as="footer" compact class="status-bar" flush>
    <div class="status-inner">
      <div class="status-item">
        <AppIcon :icon="Wifi" :size="14" />
        <span>{{ networkLabel }}</span>
      </div>
      <div class="status-item">
        <span class="dot" :class="status" />
        <span>{{ apiLabel }}</span>
      </div>
      <div class="status-item">
        <span class="dot" :class="wsStatus" />
        <span>{{ wsLabel }}</span>
      </div>
      <div class="status-item version">
        <span class="label">版本</span>
        <MonoValue size="sm">v{{ props.version }}</MonoValue>
      </div>
    </div>
  </AppCard>
</template>

<style scoped>
.status-bar :deep(.ef-card-body) {
  padding: 0;
}

.status-inner {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 8px 12px;
  flex-wrap: wrap;
  font-size: 12px;
  color: var(--muted-foreground);
}

.status-item {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.version {
  margin-left: auto;
}

.version .label {
  font-size: 11px;
}

.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--muted-foreground);
}

.dot.connected {
  background: var(--up);
}

.dot.connecting {
  background: #d29922;
}

.dot.error {
  background: var(--down);
}

.dot.disconnected {
  background: var(--muted-foreground);
}
</style>
