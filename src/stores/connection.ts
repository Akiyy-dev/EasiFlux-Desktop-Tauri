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
  connectionRequest: number
  promise: Promise<void>
}

type ConnectionCompletionValidity = () => boolean

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

function completionRemainsValid(isCompletionValid?: ConnectionCompletionValidity): boolean {
  if (!isCompletionValid) return true
  try {
    return isCompletionValid() !== false
  } catch {
    return false
  }
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

  function connectionCompletionRemainsCurrent(
    connectionRequest: number,
    isCompletionValid?: ConnectionCompletionValidity,
  ): boolean {
    if (connectionRequest !== latestConnectionRequest) return false
    if (!completionRemainsValid(isCompletionValid)) return false
    return connectionRequest === latestConnectionRequest
  }

  async function connectWithRequest(
    connectionRequest: number,
    startRealtime = true,
    credential?: ApiCredential,
    isCompletionValid?: ConnectionCompletionValidity,
  ): Promise<void> {
    const canApplyCompletion = () => connectionCompletionRemainsCurrent(
      connectionRequest,
      isCompletionValid,
    )
    if (!canApplyCompletion()) return
    lastError.value = null
    if (!canApplyCompletion()) return
    setStatus('connecting')
    if (!canApplyCompletion()) return
    try {
      await tauriInvoke('connect', { startRealtime, credential })
      if (!canApplyCompletion()) return
      if (!startRealtime) {
        setWsStatus('disconnected')
      }
      if (!canApplyCompletion()) return
      const currentStatus = await refreshStatusForConnection(
        connectionRequest,
        isCompletionValid,
      )
      if (
        currentStatus.applied
        && currentStatus.status === 'connected'
        && canApplyCompletion()
      ) {
        await refreshSyncTask('market', true)
      }
    } catch (error) {
      const message = formatInvokeError(error)
      if (canApplyCompletion()) {
        setStatus('error')
        if (canApplyCompletion()) {
          lastError.value = message
        }
      }
      const wrapped = new Error(message) as Error & { cause?: unknown }
      wrapped.cause = error
      throw wrapped
    }
  }

  function connect(
    startRealtime = true,
    credential?: ApiCredential,
    isCompletionValid?: ConnectionCompletionValidity,
  ): Promise<void> {
    const connectionRequest = ++latestConnectionRequest
    unpublishSupersededReconnectFlight()
    return connectWithRequest(
      connectionRequest,
      startRealtime,
      credential,
      isCompletionValid,
    )
  }

  async function disconnectWithRequest(connectionRequest: number): Promise<boolean> {
    if (connectionRequest !== latestConnectionRequest) return false
    await tauriInvoke('disconnect')
    if (connectionRequest !== latestConnectionRequest) return false
    setStatus('disconnected')
    if (connectionRequest !== latestConnectionRequest) return false
    setWsStatus('disconnected')
    return connectionRequest === latestConnectionRequest
  }

  async function disconnect(): Promise<void> {
    const connectionRequest = ++latestConnectionRequest
    unpublishSupersededReconnectFlight()
    await disconnectWithRequest(connectionRequest)
  }

  function unpublishSupersededReconnectFlight(): void {
    if (!reconnectFlight) return
    reconnectFlight = null
    reconnecting.value = false
  }

  function reconnectFlightHasCurrentIntent(flight: ReconnectFlight): boolean {
    if (flight.connectionRequest !== latestConnectionRequest) return false
    for (let index = 0; index < flight.intents.length; index += 1) {
      if (connectionCompletionRemainsCurrent(
        flight.connectionRequest,
        flight.intents[index].shouldConnect,
      )) {
        return true
      }
      if (flight.connectionRequest !== latestConnectionRequest) return false
    }
    return false
  }

  async function runReconnectFlight(flight: ReconnectFlight): Promise<void> {
    let selected: ReconnectIntent | undefined
    let selectionStarted = false
    try {
      const disconnected = await disconnectWithRequest(flight.connectionRequest)
      if (!disconnected) return
      selectionStarted = true
      for (let index = 0; index < flight.intents.length; index += 1) {
        const candidate = flight.intents[index]
        if (candidate.shouldConnect?.() !== false) {
          selected = candidate
          break
        }
      }
      if (!selected) return
      await connectWithRequest(
        flight.connectionRequest,
        selected.startRealtime,
        undefined,
        selected.shouldConnect,
      )
    } catch (error) {
      const canPublishFailure = selected
        ? connectionCompletionRemainsCurrent(
            flight.connectionRequest,
            selected.shouldConnect,
          )
        : selectionStarted
          ? connectionCompletionRemainsCurrent(flight.connectionRequest)
          : reconnectFlightHasCurrentIntent(flight)
      if (canPublishFailure) {
        reconnectError.value = formatInvokeError(error)
      }
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
    if (
      reconnectFlight
      && reconnectFlight.connectionRequest === latestConnectionRequest
    ) {
      reconnectFlight.intents.push(intent)
      return reconnectFlight.promise
    }
    const flight = {
      intents: [intent],
      connectionRequest: ++latestConnectionRequest,
    } as ReconnectFlight
    flight.promise = Promise.resolve().then(() => runReconnectFlight(flight))
    reconnectFlight = flight
    reconnectError.value = null
    reconnecting.value = true
    return flight.promise
  }

  async function refreshStatusForConnection(
    connectionRequest?: number,
    isCompletionValid?: ConnectionCompletionValidity,
  ): Promise<{ status: ConnectionStatus; applied: boolean }> {
    const requestId = ++latestStatusRequest
    const next = await tauriInvoke<ConnectionStatus>('get_connection_status')
    const applied = requestId === latestStatusRequest
      && (connectionRequest === undefined || (
        connectionCompletionRemainsCurrent(connectionRequest, isCompletionValid)
      ))
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
