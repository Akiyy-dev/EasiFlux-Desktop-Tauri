import { ref, watch } from 'vue'
import { usePluginStore } from '../stores/plugin'
import type { PluginCatalogItem, PluginCommandInfo } from '../types/plugin'

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
  const clear = () => { result.value = null }

  watch(
    [() => store.commandContextKey, pluginSource ?? (() => store.catalog)],
    clear,
    { flush: 'sync', deep: true },
  )

  function run(pluginId: string, contributionId: string, navigationAvailable = false): void {
    const execution = store.runCommand(pluginId, contributionId)
    if (execution?.actionId === 'host.showInfo') {
      result.value = execution.info
      return
    }
    result.value = null
    if (execution?.actionId === 'host.openPage' && navigationAvailable) {
      openPage?.({ pluginId, contributionId })
    }
  }

  return { result, run, clear }
}
