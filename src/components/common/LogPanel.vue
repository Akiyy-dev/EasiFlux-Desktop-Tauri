<script setup lang="ts">
import { ref } from 'vue'
import { NButton } from 'naive-ui'
import { storeToRefs } from 'pinia'
import { tauriInvoke } from '../../composables/useTauriCommand'
import { useLogStore } from '../../stores/log'
import { useConnectionStore } from '../../stores/connection'
import type { ProbePrivateEndpointsResult } from '../../types/models'

const logStore = useLogStore()
const connectionStore = useConnectionStore()
const { entries } = storeToRefs(logStore)

const probing = ref(false)

function formatTime(ts: number): string {
  return new Date(ts).toLocaleTimeString()
}

async function runProbe(): Promise<void> {
  if (!connectionStore.connected) {
    logStore.setError('请先连接账户后再运行接口诊断')
    return
  }
  probing.value = true
  try {
    const result = await tauriInvoke<ProbePrivateEndpointsResult>('probe_private_endpoints')
    logStore.addEntry({
      level: 'info',
      message: `接口诊断: 余额=${result.balanceCount} (${result.balancesOk ? 'ok' : 'fail'})`,
      timestamp: Date.now(),
    })
    for (const endpoint of result.endpoints) {
      const detail = endpoint.success
        ? `${endpoint.endpoint}: raw=${endpoint.rawCount}, parsed=${endpoint.parsedCount}, envelope=${endpoint.envelopeHint}, keys=[${endpoint.dataKeys.join(',')}], first=[${endpoint.firstItemKeys.join(',')}]`
        : `${endpoint.endpoint}: 失败 ${endpoint.error ?? 'unknown'}`
      logStore.addEntry({ level: endpoint.success ? 'info' : 'error', message: detail, timestamp: Date.now() })
    }
  } catch (error) {
    logStore.setError(error instanceof Error ? error.message : String(error))
  } finally {
    probing.value = false
  }
}
</script>

<template>
  <div class="log-panel">
    <div class="toolbar">
      <NButton size="tiny" :loading="probing" @click="runProbe">诊断私有接口</NButton>
    </div>
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
  height: 100%;
  display: flex;
  flex-direction: column;
}

.toolbar {
  padding: 4px 0 8px;
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

.log-row.warn .msg {
  color: #d29922;
}

.log-row.info .msg {
  color: var(--text-primary);
}
</style>
