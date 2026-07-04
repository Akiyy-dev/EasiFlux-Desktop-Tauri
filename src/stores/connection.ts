import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { ApiCredential, ConnectionStatus } from '../types/models'
import { useAccountStore } from './account'
import { useLogStore } from './log'
import { useMarketStore } from './market'
import { useOrderStore } from './order'
import { usePositionStore } from './position'

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

async function refreshPostConnectData(): Promise<void> {
  const logStore = useLogStore()

  const tasks: Array<{ label: string; run: () => Promise<void> }> = [
    { label: '账户', run: () => useAccountStore().refreshAccount() },
    { label: '行情', run: () => useMarketStore().refreshMarket() },
    { label: '订单', run: () => useOrderStore().refreshOrders() },
    { label: '持仓', run: () => usePositionStore().refreshPositions() },
  ]

  for (const task of tasks) {
    try {
      await task.run()
    } catch (error) {
      logStore.setError(`${task.label}刷新失败: ${formatInvokeError(error)}`)
    }
  }
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
      await refreshStatus()
      await refreshPostConnectData()
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

  async function refreshStatus(): Promise<void> {
    const s = await tauriInvoke<ConnectionStatus>('get_connection_status')
    status.value = s
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
  }
})
