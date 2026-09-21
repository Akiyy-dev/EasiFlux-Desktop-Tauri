import { ref, watch } from 'vue'
import { usePluginStore } from '../stores/plugin'
import type {
  PluginCatalogItem,
  PluginCommandInfo,
  PluginComputeExecutionIntent,
  PluginWorkflowExecutionIntent,
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
  const workflowIntent = ref<PluginWorkflowExecutionIntent | null>(null)
  const clear = () => {
    result.value = null
    computeIntent.value = null
    workflowIntent.value = null
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
      workflowIntent.value = null
      return
    }
    result.value = null
    if (execution?.actionId === 'host.openPage' && navigationAvailable) {
      computeIntent.value = null
      workflowIntent.value = null
      openPage?.({ pluginId, contributionId })
      return
    }
    if (execution?.actionId === 'sandbox.computeSeries') {
      computeIntent.value = execution
      workflowIntent.value = null
      return
    }
    computeIntent.value = null
    if (execution?.actionId === 'sandbox.accountWorkflow') {
      workflowIntent.value = execution
      return
    }
    workflowIntent.value = null
  }

  return { result, computeIntent, workflowIntent, run, clear }
}
