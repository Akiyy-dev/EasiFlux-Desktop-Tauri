import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { tauriInvoke } from '../composables/useTauriCommand'
import { useAsyncState } from '../composables/useAsyncState'
import type {
  AccountProfile,
  AccountSessionEvent,
  AccountSwitchResult,
  SaveCredentialRequest,
} from '../types/models'
import { clearAccountBoundState } from '../services/accountSessionService'
import { decideAccountSessionEpoch } from '../services/accountSessionEpochService'
import { useConfigStore } from './config'
import { useConnectionStore } from './connection'
import { normalizeAccountId } from '../utils/account'
import {
  collectReconciliationFailures,
  formatReconciliationError,
  type AccountReconciliationStep,
  type AccountReconciliationTask,
} from '../services/accountReconciliationService'
import {
  ACCOUNT_SWITCHING_MESSAGE,
  ACCOUNT_SYNCING_MESSAGE,
  ACCOUNT_SYNC_FAILED_MESSAGE,
} from '../utils/tradingMutationGuard'

const ACCOUNT_SWITCH_RECOVERY_REQUIRED_MARKER = 'ACCOUNT_SWITCH_RECOVERY_REQUIRED'
const ACCOUNT_SWITCH_RECOVERY_MESSAGE =
  'Account switch recovery is required. Trading and account changes are disabled; restart the application before continuing.'
const ACCOUNT_PROFILE_REFRESH_MESSAGE =
  'Account profiles must be refreshed before making another account change.'
const ACCOUNT_MUTATION_BLOCKED_MESSAGE =
  'Account changes are temporarily unavailable while account state is synchronizing.'

function sanitizeProfile(profile: AccountProfile): AccountProfile {
  return {
    accountId: profile.accountId,
    label: profile.label,
    baseUrl: profile.baseUrl,
    credentialState: profile.credentialState,
    active: profile.active,
  }
}

function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return '请求失败'
}

export const useAccountProfilesStore = defineStore('accountProfiles', () => {
  const profiles = ref<AccountProfile[]>([])
  const loading = ref(false)
  const listError = ref<string | null>(null)
  const authoritativeActiveAccountId = ref<string | null>(null)
  const sessionEpoch = ref(0)
  const transitionPending = ref(false)
  const recoveryRequired = ref(false)
  const reconciliationLoading = ref(false)
  const reconciliationFailedSteps = ref<AccountReconciliationStep[]>([])
  let latestListRequest = 0
  let reconciliationPromise: Promise<void> | null = null
  let reconciliationContext: { activeAccountId: string } | null = null
  const saveRequest = useAsyncState<void>()
  const switchRequest = useAsyncState<AccountSwitchResult>()
  const deleteRequest = useAsyncState<void>()
  const activeAccountId = computed(() => authoritativeActiveAccountId.value
    ?? normalizeAccountId(useConfigStore().config?.activeAccountId))
  const reconciliationError = computed(() =>
    formatReconciliationError(reconciliationFailedSteps.value),
  )
  const recoveryError = computed(() =>
    recoveryRequired.value ? ACCOUNT_SWITCH_RECOVERY_MESSAGE : null,
  )
  const switching = computed(() =>
    transitionPending.value || switchRequest.loading.value || reconciliationLoading.value,
  )
  const hasTradingCriticalFailure = computed(() =>
    reconciliationFailedSteps.value.some((step) =>
      step === 'connection' || step === 'bootstrap'),
  )
  const tradingBlocked = computed(() =>
    switching.value || recoveryRequired.value || hasTradingCriticalFailure.value,
  )
  const tradingBlockedMessage = computed(() => {
    if (transitionPending.value || switchRequest.loading.value) return ACCOUNT_SWITCHING_MESSAGE
    if (reconciliationLoading.value) return ACCOUNT_SYNCING_MESSAGE
    if (recoveryRequired.value) return ACCOUNT_SWITCH_RECOVERY_MESSAGE
    if (hasTradingCriticalFailure.value) return ACCOUNT_SYNC_FAILED_MESSAGE
    return null
  })
  const mutating = computed(
    () => saveRequest.loading.value || switching.value || deleteRequest.loading.value,
  )
  const accountMutationsBlocked = computed(() =>
    mutating.value
    || loading.value
    || Boolean(listError.value)
    || Boolean(reconciliationError.value)
    || recoveryRequired.value,
  )

  function assertAccountMutationAllowed(): void {
    if (recoveryRequired.value) throw new Error(ACCOUNT_SWITCH_RECOVERY_MESSAGE)
    if (loading.value || listError.value) throw new Error(ACCOUNT_PROFILE_REFRESH_MESSAGE)
    if (accountMutationsBlocked.value) throw new Error(ACCOUNT_MUTATION_BLOCKED_MESSAGE)
  }

  function alignActiveProfile(next: AccountProfile[]): AccountProfile[] {
    if (!authoritativeActiveAccountId.value) return next
    return next.map((profile) => ({
      ...profile,
      active: profile.accountId === authoritativeActiveAccountId.value,
    }))
  }

  function adoptActiveAccountId(accountId: string): string {
    const normalized = normalizeAccountId(accountId)
    authoritativeActiveAccountId.value = normalized
    useConfigStore().adoptActiveAccountId(normalized)
    profiles.value = alignActiveProfile(profiles.value)
    return normalized
  }

  function adoptSessionEpoch(nextEpoch: number): void {
    if (nextEpoch > sessionEpoch.value) sessionEpoch.value = nextEpoch
  }

  function handleSessionEvent<T>(
    event: AccountSessionEvent<T>,
    handler: (payload: T) => void,
  ): boolean {
    if (transitionPending.value || recoveryRequired.value) return false
    const decision = decideAccountSessionEpoch(sessionEpoch.value, event.sessionEpoch)
    if (decision === 'reject') return false
    if (decision === 'advance') {
      sessionEpoch.value = event.sessionEpoch
      clearAccountBoundState()
    }
    handler(event.payload)
    return true
  }

  async function refreshProfiles(): Promise<AccountProfile[]> {
    const requestId = ++latestListRequest
    loading.value = true
    listError.value = null
    try {
      const result = await tauriInvoke<AccountProfile[]>('list_account_profiles')
      const sanitized = alignActiveProfile(result.map(sanitizeProfile))
      if (requestId === latestListRequest) {
        profiles.value = sanitized
        loading.value = false
      }
      return sanitized
    } catch (error) {
      if (requestId === latestListRequest) {
        listError.value = errorMessage(error)
        loading.value = false
      }
      throw error
    }
  }

  async function saveCredentials(request: SaveCredentialRequest): Promise<void> {
    assertAccountMutationAllowed()
    await saveRequest.run(() => tauriInvoke<void>('save_credentials', { request }))
    void refreshProfiles().catch(() => undefined)
  }

  function runReconciliation(): Promise<void> {
    if (reconciliationPromise) return reconciliationPromise
    const context = reconciliationContext
    if (!context) return Promise.resolve()
    const configStore = useConfigStore()
    const connectionStore = useConnectionStore()
    const tasks: AccountReconciliationTask[] = [
      { step: 'config', run: () => configStore.fetchConfig() },
      { step: 'profiles', run: () => refreshProfiles() },
      {
        step: 'connection',
        run: () => Promise.all([
          connectionStore.refreshStatus(),
          connectionStore.refreshWsStatus(),
        ]),
      },
    ]
    tasks.push({
      step: 'bootstrap',
      run: () => tauriInvoke('scheduler_run_task', { task: 'bootstrap', force: true }),
    })
    reconciliationLoading.value = true
    reconciliationFailedSteps.value = []
    const current = collectReconciliationFailures(tasks)
      .then((failures) => {
        reconciliationFailedSteps.value = failures
        adoptActiveAccountId(context.activeAccountId)
      })
      .finally(() => {
        reconciliationLoading.value = false
        if (reconciliationPromise === current) reconciliationPromise = null
      })
    reconciliationPromise = current
    return current
  }

  function retryReconciliation(): Promise<void> {
    if (switchRequest.loading.value) return Promise.resolve()
    return runReconciliation()
  }

  async function switchAccount(accountId: string): Promise<AccountSwitchResult> {
    if (switching.value) throw new Error('Account switch is already in progress')
    assertAccountMutationAllowed()
    const previousContext = reconciliationContext
    const previousFailures = [...reconciliationFailedSteps.value]
    transitionPending.value = true
    reconciliationContext = null
    reconciliationFailedSteps.value = []
    listError.value = null
    let result: AccountSwitchResult
    try {
      result = await switchRequest.run(() =>
        tauriInvoke<AccountSwitchResult>('switch_account', {
          accountId,
          startRealtime: null,
        }),
      )
    } catch (error) {
      const switchRequiresRecovery = errorMessage(error)
        .includes(ACCOUNT_SWITCH_RECOVERY_REQUIRED_MARKER)
      const connectionStore = useConnectionStore()
      try {
        reconciliationContext = previousContext
        reconciliationFailedSteps.value = previousFailures
        const [apiStatus, websocketStatus] = await Promise.allSettled([
          connectionStore.refreshStatus(),
          connectionStore.refreshWsStatus(),
        ])
        const statusRefreshFailed = apiStatus.status === 'rejected'
          || websocketStatus.status === 'rejected'
        if (apiStatus.status === 'rejected') {
          connectionStore.setStatus('error')
        }
        if (websocketStatus.status === 'rejected') {
          connectionStore.setWsStatus('error')
        }
        if (switchRequiresRecovery || statusRefreshFailed) {
          recoveryRequired.value = true
          connectionStore.setWsStatus('disconnected')
        }
      } finally {
        transitionPending.value = false
      }
      if (!recoveryRequired.value) {
        try {
          await connectionStore.refreshWsStatus()
        } catch {
          recoveryRequired.value = true
          connectionStore.setWsStatus('disconnected')
        }
      }
      throw error
    }
    try {
      adoptSessionEpoch(result.sessionEpoch)
      const normalizedActiveAccountId = adoptActiveAccountId(result.activeAccountId)
      clearAccountBoundState()
      const connectionStore = useConnectionStore()
      connectionStore.setStatus(result.connected ? 'connected' : 'disconnected')
      await connectionStore.refreshWsStatus()
      reconciliationContext = {
        activeAccountId: normalizedActiveAccountId,
      }
    } catch (error) {
      recoveryRequired.value = true
      useConnectionStore().setWsStatus('disconnected')
      throw error
    } finally {
      transitionPending.value = false
    }
    await runReconciliation()
    return result
  }

  async function deleteAccount(accountId: string): Promise<void> {
    assertAccountMutationAllowed()
    await deleteRequest.run(() => tauriInvoke<void>('delete_account', { accountId }))
    void refreshProfiles().catch(() => undefined)
  }

  return {
    profiles,
    loading,
    listError,
    activeAccountId,
    sessionEpoch,
    recoveryRequired,
    recoveryError,
    reconciliationLoading,
    reconciliationFailedSteps,
    reconciliationError,
    saving: saveRequest.loading,
    saveError: saveRequest.error,
    switching,
    tradingBlocked,
    tradingBlockedMessage,
    switchError: switchRequest.error,
    deleting: deleteRequest.loading,
    deleteError: deleteRequest.error,
    mutating,
    accountMutationsBlocked,
    refreshProfiles,
    adoptSessionEpoch,
    handleSessionEvent,
    retryReconciliation,
    saveCredentials,
    switchAccount,
    deleteAccount,
  }
})
