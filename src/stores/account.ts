import { defineStore } from 'pinia'

import { computed, ref } from 'vue'

import type { AccountSummary, Balance, DailyPnlSnapshot, FundingBalance } from '../types/models'

import { useAsyncState } from '../composables/useAsyncState'

import { tauriInvoke } from '../composables/useTauriCommand'

import { refreshSyncTask } from '../services/dataSyncService'



function computeTotalEquity(balances: Balance[]): string {

  const total = balances

    .map((b) => parseFloat(b.total))

    .filter((n) => !Number.isNaN(n))

    .reduce((sum, n) => sum + n, 0)

  return total.toString()

}



function syncSummaryBalances(

  summary: AccountSummary | null,

  balances: Balance[],

): AccountSummary {

  const accountId = summary?.accountId ?? 'default'

  return {

    accountId,

    balances: [...balances],

    totalEquity: computeTotalEquity(balances),

  }

}

const SNAPSHOT_EVENT_TIMEOUT_MS = 5_000

interface PendingRevisionWaiter {
  resolve: () => void
  reject: (error: Error) => void
}

function createRevisionWaiter(timeoutMessage: string) {
  let revision = 0
  const waiters = new Set<PendingRevisionWaiter>()

  function current(): number {
    return revision
  }

  function advance(): void {
    revision += 1
    const pending = [...waiters]
    waiters.clear()
    for (const waiter of pending) waiter.resolve()
  }

  function waitForAdvance(startingRevision: number): Promise<void> {
    if (revision !== startingRevision) return Promise.resolve()
    return new Promise<void>((resolve, reject) => {
      let timeoutId: ReturnType<typeof setTimeout> | null = null
      const clearTimer = () => {
        if (timeoutId !== null) clearTimeout(timeoutId)
        timeoutId = null
      }
      const waiter: PendingRevisionWaiter = {
        resolve: () => {
          clearTimer()
          resolve()
        },
        reject: (error) => {
          clearTimer()
          reject(error)
        },
      }
      timeoutId = setTimeout(() => {
        if (waiters.delete(waiter)) {
          waiter.reject(new Error(timeoutMessage))
        }
      }, SNAPSHOT_EVENT_TIMEOUT_MS)
      waiters.add(waiter)
    })
  }

  return { current, advance, waitForAdvance }
}



export const useAccountStore = defineStore('account', () => {

  const summary = ref<AccountSummary | null>(null)

  const balances = ref<Balance[]>([])

  const request = useAsyncState<AccountSummary>((value) => value.balances.length === 0)

  const dailyPnlRequest = useAsyncState<DailyPnlSnapshot>((value) => value.recordCount === 0)

  const fundingRequest = useAsyncState<FundingBalance[]>((value) => value.length === 0)

  const accountEvents = createRevisionWaiter('账户快照等待超时')

  const dailyPnlEvents = createRevisionWaiter('每日盈亏快照等待超时')

  let accountRefreshOperation: Promise<void> | null = null

  let dailyPnlRefreshOperation: Promise<void> | null = null

  const fundingBalances = computed(() => fundingRequest.state.value.data ?? [])



  function setBalance(balance: Balance): void {

    const idx = balances.value.findIndex((b) => b.asset === balance.asset)

    if (idx >= 0) {

      balances.value[idx] = balance

    } else {

      balances.value.push(balance)

    }

    summary.value = syncSummaryBalances(summary.value, balances.value)
    request.setData(summary.value)
    accountEvents.advance()

  }



  function applySnapshot(next: AccountSummary): void {

    summary.value = next

    balances.value = [...next.balances]

    request.setData(next)
    accountEvents.advance()

  }



  function applyDailyPnlSnapshot(snapshot: DailyPnlSnapshot): void {

    dailyPnlRequest.setData(snapshot)
    dailyPnlEvents.advance()

  }



  function runAccountRefresh(): Promise<void> {
    if (accountRefreshOperation) return accountRefreshOperation
    const startingRevision = accountEvents.current()
    const operation = request
      .run(async () => {
        await refreshSyncTask('account', true, true)
        await accountEvents.waitForAdvance(startingRevision)
        if (!summary.value) {
          throw new Error('账户快照尚未到达')
        }
        return summary.value
      })
      .then(() => undefined)
      .finally(() => {
        if (accountRefreshOperation === operation) accountRefreshOperation = null
      })
    accountRefreshOperation = operation
    return operation
  }

  async function refreshAccount(rethrow = false): Promise<void> {
    try {
      await runAccountRefresh()
    } catch (error) {
      if (rethrow) throw error
    }

  }



  function runDailyPnlRefresh(): Promise<void> {
    if (dailyPnlRefreshOperation) return dailyPnlRefreshOperation
    const startingRevision = dailyPnlEvents.current()
    const operation = dailyPnlRequest
      .run(async () => {
        await refreshSyncTask('dailyPnl', true, true)
        await dailyPnlEvents.waitForAdvance(startingRevision)
        const snapshot = dailyPnlRequest.state.value.data
        if (!snapshot) {
          throw new Error('每日盈亏快照尚未到达')
        }
        return snapshot
      })
      .then(() => undefined)
      .finally(() => {
        if (dailyPnlRefreshOperation === operation) dailyPnlRefreshOperation = null
      })
    dailyPnlRefreshOperation = operation
    return operation
  }

  async function refreshDailyPnl(rethrow = false): Promise<void> {
    try {
      await runDailyPnlRefresh()
    } catch (error) {
      if (rethrow) throw error
    }

  }

  async function refreshFundingBalances(): Promise<void> {

    await fundingRequest.run(() => tauriInvoke<FundingBalance[]>('fetch_funding_balances'))

  }

  function clearAccountData(): void {
    accountRefreshOperation = null
    dailyPnlRefreshOperation = null
    summary.value = null
    balances.value = []
    request.reset()
    dailyPnlRequest.reset()
    fundingRequest.reset()
    accountEvents.advance()
    dailyPnlEvents.advance()
  }



  return {

    summary,

    balances,

    loading: request.loading,

    error: request.error,

    status: request.status,

    updatedAt: computed(() => request.state.value.updatedAt),

    dailyPnl: dailyPnlRequest.state,

    dailyPnlLoading: dailyPnlRequest.loading,

    dailyPnlError: dailyPnlRequest.error,

    dailyPnlStatus: dailyPnlRequest.status,

    dailyPnlUpdatedAt: computed(() => dailyPnlRequest.state.value.updatedAt),

    fundingBalances,

    fundingLoading: fundingRequest.loading,

    fundingError: fundingRequest.error,

    fundingStatus: fundingRequest.status,

    fundingUpdatedAt: computed(() => fundingRequest.state.value.updatedAt),

    setBalance,

    applySnapshot,

    applyDailyPnlSnapshot,

    refreshAccount,

    refreshDailyPnl,
    refreshFundingBalances,
    clearAccountData,

  }

})


