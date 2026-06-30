<script setup lang="ts">
import { storeToRefs } from 'pinia'
import { useLogStore } from '../../stores/log'

const logStore = useLogStore()
const { entries } = storeToRefs(logStore)

function formatTime(ts: number): string {
  return new Date(ts).toLocaleTimeString()
}
</script>

<template>
  <div class="log-panel">
    <div v-if="entries.length === 0" class="empty">暂无日志</div>
    <div v-for="(entry, i) in entries" :key="i" class="log-row" :class="entry.level">
      <span class="time">{{ formatTime(entry.timestamp) }}</span>
      <span class="msg">{{ entry.message }}</span>
    </div>
  </div>
</template>

<style scoped>
.log-panel {
  font-family: ui-monospace, monospace;
  font-size: 11px;
  padding: 4px 8px;
}

.empty {
  color: var(--text-secondary);
  padding: 8px;
}

.log-row {
  display: flex;
  gap: 8px;
  padding: 2px 0;
}

.time {
  color: var(--text-secondary);
  flex-shrink: 0;
}

.log-row.error .msg {
  color: var(--accent-red);
}

.log-row.info .msg {
  color: var(--text-primary);
}
</style>
