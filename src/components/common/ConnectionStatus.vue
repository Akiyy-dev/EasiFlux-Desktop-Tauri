<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import { useConnectionStore } from '../../stores/connection'

const connectionStore = useConnectionStore()
const { status, wsStatus, connected, lastError } = storeToRefs(connectionStore)

const label = computed(() => {
  const apiLabels: Record<string, string> = {
    disconnected: '未连接',
    connecting: 'API 连接中…',
    connected: 'API 已连接',
    error: 'API 连接错误',
  }

  if (status.value === 'error' && lastError.value) {
    const short =
      lastError.value.length > 48
        ? `${lastError.value.slice(0, 45)}…`
        : lastError.value
    return `${apiLabels.error} · ${short}`
  }

  if (status.value !== 'connected') {
    return apiLabels[status.value] ?? status.value
  }

  const wsLabels: Record<string, string> = {
    disconnected: 'API 已连接 · WS 断开',
    connecting: 'API 已连接 · WS 连接中…',
    connected: 'API · WS 已连接',
    error: 'API 已连接 · WS 重连中',
  }
  return wsLabels[wsStatus.value] ?? 'API 已连接'
})

const dotClass = computed(() => ({
  connected: connected.value && wsStatus.value === 'connected',
  'api-only': connected.value && wsStatus.value !== 'connected',
  error: status.value === 'error',
  connecting: status.value === 'connecting' || wsStatus.value === 'connecting',
}))
</script>

<template>
  <div class="connection-status ef-text-caption">
    <span class="ef-status-dot" :class="dotClass" />
    <span>{{ label }}</span>
  </div>
</template>

<style scoped>
.connection-status {
  display: flex;
  align-items: center;
  gap: var(--ef-space-2);
}
</style>
