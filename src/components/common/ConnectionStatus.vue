<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import { useConnectionStore } from '../../stores/connection'

const connectionStore = useConnectionStore()
const { status, wsStatus, connected } = storeToRefs(connectionStore)

const label = computed(() => {
  const apiLabels: Record<string, string> = {
    disconnected: '未连接',
    connecting: 'API 连接中…',
    connected: 'API 已连接',
    error: 'API 连接错误',
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
  apiOnly: connected.value && wsStatus.value !== 'connected',
  error: status.value === 'error',
  connecting: status.value === 'connecting' || wsStatus.value === 'connecting',
}))
</script>

<template>
  <div class="connection-status">
    <span class="dot" :class="dotClass" />
    <span>{{ label }}</span>
  </div>
</template>

<style scoped>
.connection-status {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary);
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #6e7681;
}

.dot.connected {
  background: var(--accent-green);
}

.dot.apiOnly {
  background: #d29922;
}

.dot.error {
  background: var(--accent-red);
}

.dot.connecting {
  background: #d29922;
  animation: pulse 1s infinite;
}

@keyframes pulse {
  50% {
    opacity: 0.4;
  }
}
</style>
