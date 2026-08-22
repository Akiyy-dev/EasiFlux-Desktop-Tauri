import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { ApiCredential, ConnectionStatus } from '../types/models'
import { refreshSyncTask } from '../services/dataSyncService'

type ReconnectIntent = {
  startRealtime: boolean
  shouldConnect?: () => boolean
}

type ReconnectFlight = {
  intents: ReconnectIntent[]
  promise: Promise<void>
}

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
  let reconnectFlight: ReconnectFlight | null = null
  let latestConnectionRequest = 0
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
    const connectionRequest = ++latestConnectionRequest
    lastError.value = null
    setStatus('connecting')
    try {
      await tauriInvoke('connect', { startRealtime, credential })
      if (connectionRequest !== latestConnectionRequest) return
      if (!startRealtime) {
        setWsStatus('disconnected')
      }
      const currentStatus = await refreshStatusForConnection(connectionRequest)
      if (
        currentStatus.applied
        && currentStatus.status === 'connected'
        && connectionRequest === latestConnectionRequest
      ) {
        await refreshSyncTask('market', true)
      }
    } catch (error) {
      const message = formatInvokeError(error)
      if (connectionRequest === latestConnectionRequest) {
        setStatus('error')
        lastError.value = message
      }
      const wrapped = new Error(message) as Error & { cause?: unknown }
      wrapped.cause = error
      throw wrapped
    }
  }

  async function disconnect(): Promise<void> {
    const connectionRequest = ++latestConnectionRequest
    await tauriInvoke('disconnect')
    if (connectionRequest !== latestConnectionRequest) return
    setStatus('disconnected')
    setWsStatus('disconnected')
  }

  async function runReconnectFlight(flight: ReconnectFlight): Promise<void> {
    try {
      await disconnect()
      let selected: ReconnectIntent | undefined
      for (let index = 0; index < flight.intents.length; index += 1) {
        const candidate = flight.intents[index]
        if (candidate.shouldConnect?.() !== false) {
          selected = candidate
          break
        }
      }
      if (!selected) return
      await connect(selected.startRealtime)
    } catch (error) {
      reconnectError.value = formatInvokeError(error)
      throw error
    } finally {
      if (reconnectFlight === flight) {
        reconnectFlight = null
        reconnecting.value = false
      }
    }
  }

  function reconnect(
    startRealtime: boolean,
    shouldConnect?: () => boolean,
  ): Promise<void> {
    const intent = { startRealtime, shouldConnect }
    if (reconnectFlight) {
      reconnectFlight.intents.push(intent)
      return reconnectFlight.promise
    }
    const flight = { intents: [intent] } as ReconnectFlight
    flight.promise = Promise.resolve().then(() => runReconnectFlight(flight))
    reconnectFlight = flight
    reconnectError.value = null
    reconnecting.value = true
    return flight.promise
  }

  async function refreshStatusForConnection(
    connectionRequest?: number,
  ): Promise<{ status: ConnectionStatus; applied: boolean }> {
    const requestId = ++latestStatusRequest
    const next = await tauriInvoke<ConnectionStatus>('get_connection_status')
    const applied = requestId === latestStatusRequest
      && (connectionRequest === undefined || connectionRequest === latestConnectionRequest)
    if (applied) status.value = next
    return { status: next, applied }
  }

  async function refreshStatus(): Promise<ConnectionStatus> {
    return (await refreshStatusForConnection()).status
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
