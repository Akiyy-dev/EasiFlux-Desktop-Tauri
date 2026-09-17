import { ref, watch } from 'vue'
import { usePluginStore } from '../stores/plugin'
import type { PluginCatalogItem, PluginCommandInfo } from '../types/plugin'

export function usePluginCommandResult(pluginSource?: () => PluginCatalogItem) {
  const store = usePluginStore()
  const result = ref<PluginCommandInfo | null>(null)
  const clear = () => { result.value = null }

  watch(
    [() => store.commandContextKey, pluginSource ?? (() => store.catalog)],
    clear,
    { flush: 'sync', deep: true },
  )

  function run(pluginId: string, contributionId: string): void {
    result.value = store.runCommand(pluginId, contributionId)
  }

  return { result, run, clear }
}
