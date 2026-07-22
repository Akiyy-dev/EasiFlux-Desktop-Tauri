import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import { useConfigStore } from './config'
import type { RiskStatus, UpdateRiskConfigRequest } from '../types/models'
import { validateRiskConfig } from '../utils/risk'

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export const useRiskStore = defineStore('risk', () => {
  const status = ref<RiskStatus | null>(null)
  const reading = ref(false)
  const saving = ref(false)
  const readError = ref<string | null>(null)
  const updateError = ref<string | null>(null)

  async function refresh(): Promise<void> {
    reading.value = true
    readError.value = null
    try {
      status.value = await tauriInvoke<RiskStatus>('get_risk_status')
    } catch (error) {
      readError.value = message(error)
      throw error
    } finally {
      reading.value = false
    }
  }

  async function save(draft: UpdateRiskConfigRequest): Promise<void> {
    const request = {
      ...draft,
      maxOrderQty: draft.maxOrderQty.trim(),
      maxPriceDeviationPct: draft.maxPriceDeviationPct.trim(),
      tradingDayTimezone: draft.tradingDayTimezone.trim(),
    }
    const validationError = validateRiskConfig(request)
    if (validationError) {
      updateError.value = validationError
      throw new Error(validationError)
    }
    saving.value = true
    updateError.value = null
    try {
      status.value = await tauriInvoke<RiskStatus>('update_risk_config', { request })
      const configStore = useConfigStore()
      if (configStore.config) {
        configStore.config = {
          ...configStore.config,
          riskEnabled: status.value.enabled,
          riskMaxOrderQty: status.value.maxOrderQty,
          riskMaxPriceDeviationPct: status.value.maxPriceDeviationPct,
          riskMaxDailyOrders: status.value.maxDailyOrders,
          tradingDayTimezone: status.value.tradingDayTimezone,
        }
      }
    } catch (error) {
      updateError.value = message(error)
      throw error
    } finally {
      saving.value = false
    }
  }

  return { status, reading, saving, readError, updateError, refresh, save }
})
