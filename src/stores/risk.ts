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

  interface QueuedOperation {
    run: () => Promise<void>
    resolve: () => void
    reject: (error: unknown) => void
  }

  const operations: QueuedOperation[] = []
  let draining = false

  async function drainOperations(): Promise<void> {
    if (draining) return
    draining = true
    try {
      while (operations.length > 0) {
        const operation = operations.shift()!
        try {
          await operation.run()
          operation.resolve()
        } catch (error) {
          operation.reject(error)
        }
      }
    } finally {
      draining = false
    }
  }

  function enqueue(operation: () => Promise<void>): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      operations.push({ run: operation, resolve, reject })
      void drainOperations()
    })
  }

  function refresh(): Promise<void> {
    return enqueue(async () => {
      reading.value = true
      readError.value = null
      updateError.value = null
      try {
        status.value = await tauriInvoke<RiskStatus>('get_risk_status')
      } catch (error) {
        readError.value = message(error)
        throw error
      } finally {
        reading.value = false
      }
    })
  }

  function save(draft: UpdateRiskConfigRequest): Promise<void> {
    const request = {
      ...draft,
      maxOrderQty: draft.maxOrderQty.trim(),
      maxPriceDeviationPct: draft.maxPriceDeviationPct.trim(),
      tradingDayTimezone: draft.tradingDayTimezone.trim(),
    }
    const validationError = validateRiskConfig(request)

    return enqueue(async () => {
      readError.value = null
      updateError.value = null
      if (validationError) {
        updateError.value = validationError
        throw new Error(validationError)
      }

      saving.value = true
      try {
        const updated = await tauriInvoke<RiskStatus>('update_risk_config', { request })
        status.value = updated
        const configStore = useConfigStore()
        configStore.adoptRiskConfig({
          riskEnabled: updated.enabled,
          riskMaxOrderQty: updated.maxOrderQty,
          riskMaxPriceDeviationPct: updated.maxPriceDeviationPct,
          riskMaxDailyOrders: updated.maxDailyOrders,
          tradingDayTimezone: updated.tradingDayTimezone,
        })
      } catch (error) {
        updateError.value = message(error)
        throw error
      } finally {
        saving.value = false
      }
    })
  }

  return { status, reading, saving, readError, updateError, refresh, save }
})
