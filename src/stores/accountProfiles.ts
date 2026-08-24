import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { tauriInvoke } from '../composables/useTauriCommand'
import { useAsyncState } from '../composables/useAsyncState'
import type {
  AccountProfile,
  AccountSessionEvent,
  AccountSwitchResult,
  DeleteAccountResult,
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
  normalizeReconciliationSteps,
  type AccountReconciliationStep,
  type AccountReconciliationTask,
} from '../services/accountReconciliationService'
import { createClientNotification } from '../services/notificationService'
import type {
  ClientNotificationFailedStep,
  ClientNotificationKind,
} from '../types/notification'
import { notifyWarning, reportError } from '../services/errorService'
import {
  ACCOUNT_SWITCHING_MESSAGE,
  ACCOUNT_SYNCING_MESSAGE,
  ACCOUNT_SYNC_FAILED_MESSAGE,
} from '../utils/tradingMutationGuard'

const ACCOUNT_SWITCH_RECOVERY_REQUIRED_MARKER = 'ACCOUNT_SWITCH_RECOVERY_REQUIRED'
const ACCOUNT_SWITCH_RECOVERY_MESSAGE =
  '账户切换需要恢复。交易和账户变更已禁用，请重启应用后继续。'
const ACCOUNT_PROFILE_REFRESH_MESSAGE =
  '请先刷新账户列表，再进行其他账户操作。'
const ACCOUNT_MUTATION_BLOCKED_MESSAGE =
  '账户状态正在同步，暂时无法更改账户。'
const NOTIFICATION_CLEANUP_PENDING_MESSAGE =
  '账户已删除，通知清理将在稍后重试。'
const UUID_V4_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/

interface AccountFailureRun {
  accountId: string
  sessionEpoch: number
  attemptId: string
  generation: number
}

interface ReconciliationAuthority {
  accountId: string
  sessionEpoch: number
}

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

function parseEventAccountId(value: unknown): string | null {
  if (typeof value !== 'string') return null
  const normalized = value.trim()
  if (normalized.length === 0 || normalized !== value) return null
  return normalized
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
  let latestReconciliationListRequest = 0
  let latestProfileResultGeneration = 0
  let reconciliationPromise: Promise<void> | null = null
  let reconciliationAuthority: ReconciliationAuthority | null = null
  let failureRunGeneration = 0
  let transitionGeneration = 0
  const saveRequest = useAsyncState<void>()
  const switchRequest = useAsyncState<AccountSwitchResult>()
  const deleteRequest = useAsyncState<DeleteAccountResult>()
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
    if (nextEpoch <= sessionEpoch.value) return
    failureRunGeneration += 1
    reconciliationPromise = null
    reconciliationLoading.value = false
    reconciliationFailedSteps.value = []
    sessionEpoch.value = nextEpoch
    if (reconciliationAuthority?.accountId === activeAccountId.value) {
      reconciliationAuthority = {
        accountId: reconciliationAuthority.accountId,
        sessionEpoch: nextEpoch,
      }
    }
  }

  function createFailureRun(
    accountId: string,
    epoch: number,
    generation = ++failureRunGeneration,
  ): AccountFailureRun {
    if (!Number.isSafeInteger(epoch) || epoch < 0) {
      throw new Error('客户端通知会话代次无效')
    }
    const attemptId = crypto.randomUUID().toLowerCase()
    if (!UUID_V4_PATTERN.test(attemptId)) {
      throw new Error('客户端通知尝试标识无效')
    }
    return {
      accountId: normalizeAccountId(accountId),
      sessionEpoch: epoch,
      attemptId,
      generation,
    }
  }

  function ownsFailureRun(run: AccountFailureRun): boolean {
    return run.generation === failureRunGeneration
      && run.accountId === activeAccountId.value
      && run.sessionEpoch === sessionEpoch.value
  }

  async function publishAccountFailure(
    run: AccountFailureRun,
    kind: ClientNotificationKind,
    failedSteps: readonly ClientNotificationFailedStep[],
    fallbackContext: string,
  ): Promise<void> {
    if (!ownsFailureRun(run)) return
    try {
      await createClientNotification({
        accountId: run.accountId,
        sessionEpoch: run.sessionEpoch,
        attemptId: run.attemptId,
        kind,
        failedSteps,
      })
      if (!ownsFailureRun(run)) return
    } catch {
      if (!ownsFailureRun(run)) return
      reportError(new Error('客户端通知提交失败'), fallbackContext)
    }
  }

  function handleSessionEvent<T>(
    event: AccountSessionEvent<T>,
    handler: (payload: T) => void,
  ): boolean {
    if (transitionPending.value || recoveryRequired.value) return false
    const eventAccountId = parseEventAccountId(event.accountId)
    if (eventAccountId === null || eventAccountId !== activeAccountId.value) return false
    if (!Number.isSafeInteger(event.sessionEpoch) || event.sessionEpoch < 0) return false
    const decision = decideAccountSessionEpoch(sessionEpoch.value, event.sessionEpoch)
    if (decision === 'reject') return false
    if (decision === 'advance') {
      adoptSessionEpoch(event.sessionEpoch)
      clearAccountBoundState()
    }
    handler(event.payload)
    return true
  }

  async function refreshProfiles(): Promise<AccountProfile[]> {
    const requestId = ++latestListRequest
    const resultGeneration = ++latestProfileResultGeneration
    loading.value = true
    listError.value = null
    try {
      const result = await tauriInvoke<AccountProfile[]>('list_account_profiles')
      const sanitized = alignActiveProfile(result.map(sanitizeProfile))
      if (
        requestId === latestListRequest
        && resultGeneration === latestProfileResultGeneration
      ) {
        profiles.value = sanitized
      }
      if (requestId === latestListRequest) loading.value = false
      return sanitized
    } catch (error) {
      if (requestId === latestListRequest) {
        if (resultGeneration === latestProfileResultGeneration) {
          listError.value = errorMessage(error)
        }
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

  function runReconciliation(existingRun?: AccountFailureRun): Promise<void> {
    if (reconciliationPromise) return reconciliationPromise
    const authority = reconciliationAuthority
    if (!authority) return Promise.resolve()
    const context = existingRun ?? createFailureRun(
      authority.accountId,
      authority.sessionEpoch,
    )
    if (!ownsFailureRun(context)) return Promise.resolve()
    const configStore = useConfigStore()
    const connectionStore = useConnectionStore()
    const tasks: AccountReconciliationTask[] = [
      {
        step: 'config',
        run: () => configStore.fetchConfigForCompletion(() => ownsFailureRun(context)),
      },
      { step: 'profiles', run: () => refreshProfilesForRun(context) },
      {
        step: 'connection',
        run: async () => {
          await Promise.all([
            connectionStore.refreshStatus(() => ownsFailureRun(context)),
            connectionStore.refreshWsStatus(() => ownsFailureRun(context)),
          ])
          if (!ownsFailureRun(context)) return
        },
      },
    ]
    tasks.push({
      step: 'bootstrap',
      run: () => tauriInvoke('scheduler_run_task', { task: 'bootstrap', force: true }),
    })
    reconciliationLoading.value = true
    reconciliationFailedSteps.value = []
    const current = collectReconciliationFailures(tasks)
      .then(async (failures) => {
        if (!ownsFailureRun(context)) return
        const normalizedFailures = normalizeReconciliationSteps(failures)
        if (normalizedFailures.length > 0) {
          await publishAccountFailure(
            context,
            'accountReconciliationFailed',
            normalizedFailures,
            '账户同步失败通知提交失败',
          )
          if (!ownsFailureRun(context)) return
        }
        reconciliationFailedSteps.value = normalizedFailures
        adoptActiveAccountId(context.accountId)
      })
      .finally(() => {
        if (ownsFailureRun(context)) reconciliationLoading.value = false
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
    if (switching.value) throw new Error('账户切换正在进行中')
    assertAccountMutationAllowed()
    const previousAuthority = reconciliationAuthority
    const previousFailures = [...reconciliationFailedSteps.value]
    const switchRun = createFailureRun(activeAccountId.value, sessionEpoch.value)
    const transitionToken = ++transitionGeneration
    transitionPending.value = true
    reconciliationAuthority = null
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
      if (!ownsFailureRun(switchRun)) {
        if (transitionToken === transitionGeneration) transitionPending.value = false
        throw error
      }
      const switchRequiresRecovery = errorMessage(error)
        .includes(ACCOUNT_SWITCH_RECOVERY_REQUIRED_MARKER)
      const connectionStore = useConnectionStore()
      try {
        const [apiStatus, websocketStatus] = await Promise.allSettled([
          connectionStore.refreshStatus(() => ownsFailureRun(switchRun)),
          connectionStore.refreshWsStatus(() => ownsFailureRun(switchRun)),
        ])
        if (!ownsFailureRun(switchRun)) throw error
        reconciliationAuthority = previousAuthority
        reconciliationFailedSteps.value = previousFailures
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
        if (!recoveryRequired.value) {
          if (transitionToken === transitionGeneration) transitionPending.value = false
          try {
            await connectionStore.refreshWsStatus(() => ownsFailureRun(switchRun))
            if (!ownsFailureRun(switchRun)) throw error
          } catch {
            if (ownsFailureRun(switchRun)) {
              recoveryRequired.value = true
              connectionStore.setWsStatus('disconnected')
            }
          }
        }
        if (recoveryRequired.value && ownsFailureRun(switchRun)) {
          await publishAccountFailure(
            switchRun,
            'accountRecoveryFailed',
            ['connection'],
            '账户恢复失败通知提交失败',
          )
        }
      } finally {
        if (transitionToken === transitionGeneration) transitionPending.value = false
      }
      throw error
    }
    let reconciliationRun: AccountFailureRun | null = null
    let recoveryFailedStep: ClientNotificationFailedStep = 'bootstrap'
    try {
      if (!ownsFailureRun(switchRun)) return result
      if (result.sessionEpoch > sessionEpoch.value) sessionEpoch.value = result.sessionEpoch
      const normalizedActiveAccountId = adoptActiveAccountId(result.activeAccountId)
      reconciliationRun = createFailureRun(
        normalizedActiveAccountId,
        sessionEpoch.value,
        switchRun.generation,
      )
      clearAccountBoundState()
      recoveryFailedStep = 'connection'
      const connectionStore = useConnectionStore()
      connectionStore.setStatus(result.connected ? 'connected' : 'disconnected')
      await connectionStore.refreshWsStatus(() => (
        reconciliationRun !== null && ownsFailureRun(reconciliationRun)
      ))
      if (!ownsFailureRun(reconciliationRun)) return result
      reconciliationAuthority = {
        accountId: normalizedActiveAccountId,
        sessionEpoch: sessionEpoch.value,
      }
    } catch (error) {
      if (reconciliationRun && ownsFailureRun(reconciliationRun)) {
        recoveryRequired.value = true
        useConnectionStore().setWsStatus('disconnected')
        await publishAccountFailure(
          reconciliationRun,
          'accountRecoveryFailed',
          [recoveryFailedStep],
          '账户恢复失败通知提交失败',
        )
      }
      throw error
    } finally {
      if (transitionToken === transitionGeneration) transitionPending.value = false
    }
    if (reconciliationRun) await runReconciliation(reconciliationRun)
    return result
  }

  async function refreshProfilesForRun(run: AccountFailureRun): Promise<void> {
    const requestId = ++latestReconciliationListRequest
    const resultGeneration = ++latestProfileResultGeneration
    if (
      requestId === latestReconciliationListRequest
      && resultGeneration === latestProfileResultGeneration
      && ownsFailureRun(run)
    ) {
      listError.value = null
    }
    let result: AccountProfile[]
    try {
      result = await tauriInvoke<AccountProfile[]>('list_account_profiles')
    } catch (error) {
      if (
        requestId !== latestReconciliationListRequest
        || resultGeneration !== latestProfileResultGeneration
        || !ownsFailureRun(run)
      ) return
      listError.value = errorMessage(error)
      throw error
    }
    if (
      requestId !== latestReconciliationListRequest
      || resultGeneration !== latestProfileResultGeneration
      || !ownsFailureRun(run)
    ) return
    profiles.value = alignActiveProfile(result.map(sanitizeProfile))
    listError.value = null
  }

  async function deleteAccount(accountId: string): Promise<DeleteAccountResult> {
    assertAccountMutationAllowed()
    const result = await deleteRequest.run(() =>
      tauriInvoke<DeleteAccountResult>('delete_account', { accountId }),
    )
    if (result?.notificationCleanupPending === true
      && result.warningCode === 'NOTIFICATION_CLEANUP_PENDING') {
      notifyWarning(NOTIFICATION_CLEANUP_PENDING_MESSAGE)
    }
    void refreshProfiles().catch(() => undefined)
    return result
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
