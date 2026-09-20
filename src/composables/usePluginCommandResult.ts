import { ref, watch } from 'vue'
import { usePluginStore } from '../stores/plugin'
import type {
  PluginCatalogItem,
  PluginCommandInfo,
  PluginComputeExecutionIntent,
} from '../types/plugin'

export interface PluginOpenPageIntent {
  pluginId: string
  contributionId: string
}

export function usePluginCommandResult(
  pluginSource?: () => PluginCatalogItem,
  openPage?: (intent: PluginOpenPageIntent) => void,
) {
  const store = usePluginStore()
  const result = ref<PluginCommandInfo | null>(null)
  const computeIntent = ref<PluginComputeExecutionIntent | null>(null)
  const clear = () => {
    result.value = null
    computeIntent.value = null
  }

  watch(
    [() => store.commandContextKey, pluginSource ?? (() => store.catalog)],
    clear,
    { flush: 'sync', deep: true },
  )

  function run(pluginId: string, contributionId: string, navigationAvailable = false): void {
    const execution = store.runCommand(pluginId, contributionId)
    if (execution?.actionId === 'host.showInfo') {
      result.value = execution.info
      computeIntent.value = null
      return
    }
    result.value = null
    if (execution?.actionId === 'host.openPage' && navigationAvailable) {
      computeIntent.value = null
      openPage?.({ pluginId, contributionId })
      return
    }
    if (execution?.actionId === 'sandbox.computeSeries') {
      computeIntent.value = execution
      return
    }
    computeIntent.value = null
  }

  return { result, computeIntent, run, clear }
}
