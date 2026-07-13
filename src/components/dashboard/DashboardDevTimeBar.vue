<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import { useTimeStore } from '../../stores/time'
import { formatClockTime, formatDateTime } from '../../utils/time'

const timeStore = useTimeStore()
const { snapshot, serverNow } = storeToRefs(timeStore)

const syncLabel = computed(() => {
  switch (snapshot.value.syncStatus) {
    case 'synced':
      return '已同步'
    case 'syncing':
      return '同步中'
    case 'failed':
      return '同步失败'
    default:
      return '本地降级'
  }
})

const syncClass = computed(() => {
  if (snapshot.value.syncStatus === 'failed' || snapshot.value.source === 'local') {
    return 'error'
  }
  if (snapshot.value.syncStatus === 'syncing') {
    return 'warning'
  }
  return 'ok'
})

const lastSyncText = computed(() => {
  const at = snapshot.value.lastSyncAt
  return at ? formatDateTime(at) : '尚未同步'
})
</script>

<template>
  <div class="dev-time-bar" :class="syncClass">
    <span>服务器 {{ formatClockTime(serverNow) }}</span>
    <span>本地 {{ formatClockTime(snapshot.localTimeMs) }}</span>
    <span>偏移 {{ snapshot.offsetMs }}ms</span>
    <span>{{ syncLabel }}</span>
    <span>最近同步 {{ lastSyncText }}</span>
    <span v-if="snapshot.lastError" class="error-text">{{ snapshot.lastError }}</span>
  </div>
</template>

<style scoped>
.dev-time-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  font-size: 11px;
  color: var(--muted-foreground);
  padding: 4px 0;
}

.dev-time-bar.ok {
  color: var(--muted-foreground);
}

.dev-time-bar.warning {
  color: #d29922;
}

.dev-time-bar.error {
  color: var(--down);
}

.error-text {
  max-width: 240px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
