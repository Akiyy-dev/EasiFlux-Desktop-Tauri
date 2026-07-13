import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { ApiCredential, ConnectionStatus } from '../types/models'
import { refreshSyncTask } from '../services/dataSyncService'

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
  const connecting = computed(() => status.value === 'connecting')
  const connected = computed(() => status.value === 'connected')
  const wsConnected = computed(() => wsStatus.value === 'connected')

  function setStatus(next: string): void {
    const parsed = parseStatus(next)
    if (parsed) {
      status.value = parsed
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
      if (currentStatus === 'connected') {
        await refreshSyncTask('market', true)
      }
    } catch (error) {
      status.value = 'error'
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
    connecting,
    connected,
    wsConnected,
    setStatus,
    setWsStatus,
    connect,
    disconnect,
    refreshStatus,
    refreshTask: refreshSyncTask,
  }
})
