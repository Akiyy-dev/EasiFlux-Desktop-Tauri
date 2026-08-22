import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { ApiCredential, ConnectionStatus } from '../types/models'
import { refreshSyncTask } from '../services/dataSyncService'

function parseStatus(next: string): ConnectionStatus | null {
  if (
    next === 'disconnected'
    || next === 'connecting'
    || next === 'connected'
    || next === 'error'
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
  const reconnecting = ref(false)
  const reconnectError = ref<string | null>(null)
  let reconnectPromise: Promise<void> | null = null
  let latestStatusRequest = 0
  let latestWsStatusRequest = 0
  const connecting = computed(() => status.value === 'connecting')
  const connected = computed(() => status.value === 'connected')
  const wsConnected = computed(() => wsStatus.value === 'connected')

  function setStatus(next: string): void {
    const parsed = parseStatus(next)
    if (parsed) {
      latestStatusRequest += 1
      status.value = parsed
    }
  }

  function setWsStatus(next: string): void {
    const parsed = parseStatus(next)
    if (parsed) {
      latestWsStatusRequest += 1
      wsStatus.value = parsed
    }
  }

  async function connect(
    startRealtime = true,
    credential?: ApiCredential,
  ): Promise<void> {
    lastError.value = null
    setStatus('connecting')
    try {
      await tauriInvoke('connect', { startRealtime, credential })
      if (!startRealtime) {
        setWsStatus('disconnected')
      }
      const currentStatus = await refreshStatus()
      if (currentStatus === 'connected') {
        await refreshSyncTask('market', true)
      }
    } catch (error) {
      setStatus('error')
      const message = formatInvokeError(error)
      lastError.value = message
      const wrapped = new Error(message) as Error & { cause?: unknown }
      wrapped.cause = error
      throw wrapped
    }
  }

  async function disconnect(): Promise<void> {
    await tauriInvoke('disconnect')
    setStatus('disconnected')
    setWsStatus('disconnected')
  }

  function reconnect(startRealtime: boolean): Promise<void> {
    if (reconnectPromise) return reconnectPromise
    reconnecting.value = true
    reconnectError.value = null
    reconnectPromise = (async () => {
      try {
        await disconnect()
        await connect(startRealtime)
      } catch (error) {
        reconnectError.value = formatInvokeError(error)
        throw error
      } finally {
        reconnecting.value = false
        reconnectPromise = null
      }
    })()
    return reconnectPromise
  }

  async function refreshStatus(): Promise<ConnectionStatus> {
    const requestId = ++latestStatusRequest
    const next = await tauriInvoke<ConnectionStatus>('get_connection_status')
    if (requestId === latestStatusRequest) status.value = next
    return next
  }

  async function refreshWsStatus(): Promise<ConnectionStatus> {
    const requestId = ++latestWsStatusRequest
    const next = await tauriInvoke<ConnectionStatus>('get_websocket_status')
    if (requestId === latestWsStatusRequest) wsStatus.value = next
    return next
  }

  return {
    status,
    wsStatus,
    lastError,
    reconnecting,
    reconnectError,
    connecting,
    connected,
    wsConnected,
    setStatus,
    setWsStatus,
    connect,
    disconnect,
    reconnect,
    refreshStatus,
    refreshWsStatus,
    refreshTask: refreshSyncTask,
  }
})
