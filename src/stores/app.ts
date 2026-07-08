import { defineStore } from 'pinia'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { EnvironmentStatus } from '../types/models'
import { applyReadyVersion, ensureVersionLoaded, useVersionRef } from '../services/versionService'
import { useAsyncState } from '../composables/useAsyncState'

export const useAppStore = defineStore('app', () => {
  const version = useVersionRef()
  const environmentState = useAsyncState<EnvironmentStatus>((value) => !value.reachable)

  async function initVersion(): Promise<void> {
    await ensureVersionLoaded()
  }

  function markReady(v: string): void {
    applyReadyVersion(v)
  }

  async function refreshEnvironment(): Promise<void> {
    try {
      await environmentState.run(() => tauriInvoke<EnvironmentStatus>('get_environment_status'))
    } catch (error) {
      environmentState.setError(error)
    }
  }

  return {
    version,
    environment: environmentState.state,
    environmentLoading: environmentState.loading,
    environmentError: environmentState.error,
    environmentStatus: environmentState.status,
    initVersion,
    markReady,
    refreshEnvironment,
  }
})
