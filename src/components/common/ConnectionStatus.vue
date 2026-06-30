<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import { useConnectionStore } from '../../stores/connection'

const connectionStore = useConnectionStore()
const { status, connected } = storeToRefs(connectionStore)

const label = computed(() => {
  const map: Record<string, string> = {
    disconnected: '未连接',
    connecting: '连接中…',
    connected: '已连接',
    error: '连接错误',
  }
  return map[status.value] ?? status.value
})

const dotClass = computed(() => ({
  connected: connected.value,
  error: status.value === 'error',
  connecting: status.value === 'connecting',
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
