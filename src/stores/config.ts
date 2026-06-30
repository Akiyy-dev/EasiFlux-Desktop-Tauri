import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { AppConfig, SaveCredentialRequest } from '../types/models'

export const useConfigStore = defineStore('config', () => {
  const config = ref<AppConfig | null>(null)
  const loading = ref(false)

  async function fetchConfig(): Promise<void> {
    loading.value = true
    try {
      config.value = await tauriInvoke<AppConfig>('get_config')
    } finally {
      loading.value = false
    }
  }

  async function saveConfig(next: AppConfig): Promise<void> {
    await tauriInvoke('save_config', { config: next })
    config.value = next
  }

  async function saveCredentials(req: SaveCredentialRequest): Promise<void> {
    await tauriInvoke('save_credentials', { request: req })
  }

  async function hasCredentials(accountId: string): Promise<boolean> {
    return tauriInvoke<boolean>('has_credentials', { accountId })
  }

  return { config, loading, fetchConfig, saveConfig, saveCredentials, hasCredentials }
})
