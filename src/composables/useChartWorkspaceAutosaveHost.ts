import { inject, onMounted, onUnmounted, provide, type InjectionKey } from 'vue'
import { ChartWorkspaceAutosave } from '../services/chartWorkspaceAutosave'
import { registerChartWorkspaceFlusher } from '../services/chartWorkspaceFlushRegistry'
import { saveChartWorkspace } from '../services/chartWorkspaceService'
import { reportError } from '../services/errorService'

export const chartWorkspaceAutosaveKey: InjectionKey<ChartWorkspaceAutosave> =
  Symbol('chart-workspace-autosave')

export function useChartWorkspaceAutosaveHost(): ChartWorkspaceAutosave {
  const autosave = new ChartWorkspaceAutosave({
    save: saveChartWorkspace,
    report: (context, error) => { reportError(error, context) },
    setInterval: globalThis.setInterval,
    clearInterval: globalThis.clearInterval,
  })
  const flusher = registerChartWorkspaceFlusher((reason) => autosave.flush(reason))
  provide(chartWorkspaceAutosaveKey, autosave)

  onMounted(() => {
    flusher.setActive(true)
    autosave.start()
  })

  onUnmounted(() => {
    flusher.setActive(false)
    flusher.unregister()
    autosave.stop()
  })

  return autosave
}

export function useChartWorkspaceAutosave(): ChartWorkspaceAutosave {
  const autosave = inject(chartWorkspaceAutosaveKey)
  if (!autosave) {
    throw new Error('Chart workspace autosave host is not installed above this component')
  }
  return autosave
}
