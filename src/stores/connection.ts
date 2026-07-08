import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { ApiCredential, ConnectionStatus } from '../types/models'
import {
  refreshAllSyncTasks,
  refreshSyncTask,
  startDataSync as startSyncScheduler,
  stopDataSync as stopSyncScheduler,
  syncRunning as isSyncRunning,
} from '../services/dataSyncService'

function parseStatus(next: string): ConnectionStatus | null {
  if (
    next === 'disconnected' ||
    next === 'connecting' ||
    next === 'connected' ||
    next === 'error'
  ) {
    return next
  }
  return null
}

function formatInvokeError(error: unknown): string {
  if (typeof error === 'string') {
    return error
  }
  if (error instanceof Error) {
    return error.message
  }
  return '连接失败'
}

export const useConnectionStore = defineStore('connection', () => {
  const status = ref<ConnectionStatus>('disconnected')
  const wsStatus = ref<ConnectionStatus>('disconnected')
  const lastError = ref<string | null>(null)
  const syncRunning = ref(false)
  const connecting = computed(() => status.value === 'connecting')
  const connected = computed(() => status.value === 'connected')
  const wsConnected = computed(() => wsStatus.value === 'connected')

  async function refreshSyncedData(): Promise<void> {
    if (status.value !== 'connected') {
      return
    }
    await refreshAllSyncTasks()
  }

  function startDataSync(): void {
    startSyncScheduler()
    syncRunning.value = isSyncRunning()
  }

  function stopDataSync(): void {
    stopSyncScheduler()
    syncRunning.value = false
  }

  function setStatus(next: string): void {
    const parsed = parseStatus(next)
    if (parsed) {
      status.value = parsed
      if (parsed === 'connected') {
        startDataSync()
      } else if (parsed === 'disconnected' || parsed === 'error') {
        stopDataSync()
      }
    }
  }

  function setWsStatus(next: string): void {
    const parsed = parseStatus(next)
    if (parsed) {
      wsStatus.value = parsed
    }
  }

  async function connect(
    startRealtime = true,
    credential?: ApiCredential,
  ): Promise<void> {
    lastError.value = null
    status.value = 'connecting'
    try {
      await tauriInvoke('connect', { startRealtime, credential })
      const currentStatus = await refreshStatus()
      await refreshAllSyncTasks()
      if (currentStatus === 'connected') {
        startDataSync()
      }
    } catch (error) {
      status.value = 'error'
      stopDataSync()
      const message = formatInvokeError(error)
      lastError.value = message
      const wrapped = new Error(message) as Error & { cause?: unknown }
      wrapped.cause = error
      throw wrapped
    }
  }

  async function disconnect(): Promise<void> {
    await tauriInvoke('disconnect')
    status.value = 'disconnected'
    wsStatus.value = 'disconnected'
    stopDataSync()
  }

  async function refreshStatus(): Promise<ConnectionStatus> {
    const s = await tauriInvoke<ConnectionStatus>('get_connection_status')
    status.value = s
    return s
  }

  return {
    status,
    wsStatus,
    lastError,
    syncRunning,
    connecting,
    connected,
    wsConnected,
    setStatus,
    setWsStatus,
    connect,
    disconnect,
    refreshStatus,
    startDataSync,
    stopDataSync,
    refreshSyncedData,
    refreshTask: refreshSyncTask,
  }
})
