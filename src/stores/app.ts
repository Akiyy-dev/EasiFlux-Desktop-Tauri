import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { AppConfig, PingResponse } from '../types/models'

export const useAppStore = defineStore('app', () => {
  const version = ref('0.2.0')
  const pingMessage = ref('')
  const ready = ref(false)

  async function ping(): Promise<void> {
    const res = await tauriInvoke<PingResponse>('ping')
    pingMessage.value = res.message
    version.value = res.version
  }

  async function loadConfig(): Promise<AppConfig> {
    return tauriInvoke<AppConfig>('get_config')
  }

  function markReady(v: string): void {
    version.value = v
    ready.value = true
  }

  return { version, pingMessage, ready, ping, loadConfig, markReady }
})
