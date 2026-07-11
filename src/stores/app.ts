import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { EnvironmentStatus } from '../types/models'
import { applyReadyVersion, ensureVersionLoaded, useVersionRef } from '../services/versionService'
import { useAsyncState } from '../composables/useAsyncState'
import { refreshSyncTask } from '../services/dataSyncService'

export const useAppStore = defineStore('app', () => {
  const version = useVersionRef()
  const environmentState = useAsyncState<EnvironmentStatus>()
  const environmentChecking = ref(false)
  const environmentLoading = computed(
    () => environmentChecking.value && environmentState.state.value.data == null,
  )
  let environmentPromise: Promise<void> | null = null

  async function initVersion(): Promise<void> {
    await ensureVersionLoaded()
  }

  function markReady(v: string): void {
    applyReadyVersion(v)
  }

  function applyEnvironment(status: EnvironmentStatus): void {
    environmentState.setData(status)
    environmentChecking.value = false
  }

  async function refreshEnvironment(force = false): Promise<void> {
    if (environmentPromise) {
      return environmentPromise
    }

    environmentChecking.value = true
    if (environmentState.state.value.data == null) {
      environmentState.setLoading()
    }
    environmentPromise = (async () => {
      try {
        await refreshSyncTask('environment', force)
        const result = await tauriInvoke<EnvironmentStatus>('get_environment_status')
        environmentState.setData(result)
      } catch (error) {
        environmentState.setError(error)
        throw error
      } finally {
        environmentChecking.value = false
        environmentPromise = null
      }
    })()
    return environmentPromise
  }

  return {
    version,
    environment: environmentState.state,
    environmentLoading,
    environmentChecking,
    environmentError: environmentState.error,
    environmentStatus: environmentState.status,
    initVersion,
    markReady,
    applyEnvironment,
    refreshEnvironment,
  }
})
