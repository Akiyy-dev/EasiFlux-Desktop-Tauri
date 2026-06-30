import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { ConnectionStatus } from '../types/models'

export const useConnectionStore = defineStore('connection', () => {
  const status = ref<ConnectionStatus>('disconnected')
  const connecting = computed(() => status.value === 'connecting')
  const connected = computed(() => status.value === 'connected')

  function setStatus(next: string): void {
    if (
      next === 'disconnected' ||
      next === 'connecting' ||
      next === 'connected' ||
      next === 'error'
    ) {
      status.value = next
    }
  }

  async function connect(startRealtime = true): Promise<void> {
    status.value = 'connecting'
    try {
      await tauriInvoke('connect', { startRealtime })
      status.value = 'connected'
    } catch {
      status.value = 'error'
      throw new Error('连接失败')
    }
  }

  async function disconnect(): Promise<void> {
    await tauriInvoke('disconnect')
    status.value = 'disconnected'
  }

  async function refreshStatus(): Promise<void> {
    const s = await tauriInvoke<ConnectionStatus>('get_connection_status')
    status.value = s
  }

  return { status, connecting, connected, setStatus, connect, disconnect, refreshStatus }
})
