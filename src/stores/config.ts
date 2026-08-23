import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type {
  AppConfig,
  GeneralSettings,
  SaveCredentialRequest,
  UpdateGeneralSettingsRequest,
} from '../types/models'
import { normalizeAccountId } from '../utils/account'

type RiskConfigFields = Pick<
  AppConfig,
  | 'riskEnabled'
  | 'riskMaxOrderQty'
  | 'riskMaxPriceDeviationPct'
  | 'riskMaxDailyOrders'
  | 'tradingDayTimezone'
>

export const useConfigStore = defineStore('config', () => {
  const config = ref<AppConfig | null>(null)
  const loading = ref(false)
  let latestFetchRequest = 0
  let authoritativeActiveAccountId: string | null = null

  function withAuthoritativeAccount(next: AppConfig): AppConfig {
    return authoritativeActiveAccountId
      ? { ...next, activeAccountId: authoritativeActiveAccountId }
      : next
  }

  async function fetchConfig(): Promise<AppConfig> {
    const requestId = ++latestFetchRequest
    loading.value = true
    try {
      const result = withAuthoritativeAccount(await tauriInvoke<AppConfig>('get_config'))
      if (requestId === latestFetchRequest) config.value = result
      return result
    } finally {
      if (requestId === latestFetchRequest) loading.value = false
    }
  }

  async function saveConfig(next: AppConfig): Promise<void> {
    const requestId = ++latestFetchRequest
    const result = withAuthoritativeAccount(
      await tauriInvoke<AppConfig>('save_config', { config: next }),
    )
    if (requestId === latestFetchRequest) config.value = result
  }

  function adoptActiveAccountId(accountId: string): void {
    authoritativeActiveAccountId = normalizeAccountId(accountId)
    latestFetchRequest += 1
    loading.value = false
    if (config.value) {
      config.value = { ...config.value, activeAccountId: authoritativeActiveAccountId }
    }
  }

  function adoptRiskConfig(risk: RiskConfigFields): void {
    latestFetchRequest += 1
    loading.value = false
    if (config.value) {
      config.value = { ...config.value, ...risk }
    }
  }

  function adoptGeneralSettings(settings: GeneralSettings): void {
    latestFetchRequest += 1
    loading.value = false
    if (config.value) config.value = { ...config.value, ...settings }
  }

  async function updateGeneralSettings(
    request: UpdateGeneralSettingsRequest,
  ): Promise<GeneralSettings> {
    const result = await tauriInvoke<GeneralSettings>('update_general_settings', {
      request: {
        useWebsocket: request.useWebsocket,
        tickerPollInterval: request.tickerPollInterval,
      },
    })
    adoptGeneralSettings(result)
    return result
  }

  async function saveCredentials(req: SaveCredentialRequest): Promise<void> {
    await tauriInvoke('save_credentials', { request: req })
  }

  async function hasCredentials(accountId: string): Promise<boolean> {
    return tauriInvoke<boolean>('has_credentials', { accountId })
  }

  return {
    config,
    loading,
    fetchConfig,
    saveConfig,
    adoptActiveAccountId,
    adoptRiskConfig,
    adoptGeneralSettings,
    updateGeneralSettings,
    saveCredentials,
    hasCredentials,
  }
})
