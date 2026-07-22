import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { tauriInvoke } from '../composables/useTauriCommand'
import { useAsyncState } from '../composables/useAsyncState'
import type { AccountProfile, AccountSwitchResult, SaveCredentialRequest } from '../types/models'
import { clearAccountBoundState } from '../services/accountSessionService'
import { useConfigStore } from './config'
import { useConnectionStore } from './connection'

function sanitizeProfile(profile: AccountProfile): AccountProfile {
  return {
    accountId: profile.accountId,
    label: profile.label,
    baseUrl: profile.baseUrl,
    credentialState: profile.credentialState,
    active: profile.active,
  }
}

export const useAccountProfilesStore = defineStore('accountProfiles', () => {
  const profiles = ref<AccountProfile[]>([])
  const listRequest = useAsyncState<AccountProfile[]>((value) => value.length === 0)
  const saveRequest = useAsyncState<void>()
  const switchRequest = useAsyncState<AccountSwitchResult>()
  const deleteRequest = useAsyncState<void>()
  const mutating = computed(
    () => saveRequest.loading.value || switchRequest.loading.value || deleteRequest.loading.value,
  )

  async function refreshProfiles(): Promise<AccountProfile[]> {
    const result = await listRequest.run(() =>
      tauriInvoke<AccountProfile[]>('list_account_profiles'),
    )
    profiles.value = result.map(sanitizeProfile)
    return profiles.value
  }

  async function saveCredentials(request: SaveCredentialRequest): Promise<void> {
    await saveRequest.run(() => tauriInvoke<void>('save_credentials', { request }))
    await refreshProfiles()
  }

  async function switchAccount(accountId: string): Promise<AccountSwitchResult> {
    const result = await switchRequest.run(() =>
      tauriInvoke<AccountSwitchResult>('switch_account', {
        accountId,
        startRealtime: null,
      }),
    )
    clearAccountBoundState()
    const connectionStore = useConnectionStore()
    connectionStore.setStatus(result.connected ? 'connected' : 'disconnected')
    await Promise.allSettled([
      useConfigStore().fetchConfig(),
      refreshProfiles(),
      connectionStore.refreshStatus(),
      result.connected
        ? tauriInvoke('scheduler_run_task', { task: 'bootstrap', force: true })
        : Promise.resolve(),
    ])
    return result
  }

  async function deleteAccount(accountId: string): Promise<void> {
    await deleteRequest.run(() => tauriInvoke<void>('delete_account', { accountId }))
    await refreshProfiles()
  }

  return {
    profiles,
    loading: listRequest.loading,
    listError: listRequest.error,
    saving: saveRequest.loading,
    saveError: saveRequest.error,
    switching: switchRequest.loading,
    switchError: switchRequest.error,
    deleting: deleteRequest.loading,
    deleteError: deleteRequest.error,
    mutating,
    refreshProfiles,
    saveCredentials,
    switchAccount,
    deleteAccount,
  }
})
