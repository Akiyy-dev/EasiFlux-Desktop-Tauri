import { defineStore } from 'pinia'

import { ref } from 'vue'

import type { AccountSummary, Balance, DailyPnlSnapshot } from '../types/models'

import { useAsyncState } from '../composables/useAsyncState'

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



export const useAccountStore = defineStore('account', () => {

  const summary = ref<AccountSummary | null>(null)

  const balances = ref<Balance[]>([])

  const request = useAsyncState<AccountSummary>((value) => value.balances.length === 0)

  const dailyPnlRequest = useAsyncState<DailyPnlSnapshot>()



  function setBalance(balance: Balance): void {

    const idx = balances.value.findIndex((b) => b.asset === balance.asset)

    if (idx >= 0) {

      balances.value[idx] = balance

    } else {

      balances.value.push(balance)

    }

    summary.value = syncSummaryBalances(summary.value, balances.value)

  }



  function applySnapshot(next: AccountSummary): void {

    summary.value = next

    balances.value = [...next.balances]

    request.setData(next)

  }



  function applyDailyPnlSnapshot(snapshot: DailyPnlSnapshot): void {

    dailyPnlRequest.setData(snapshot)

  }



  async function refreshAccount(): Promise<void> {

    await refreshSyncTask('account', true)

    if (summary.value) {

      request.setData(summary.value)

    }

  }



  async function refreshDailyPnl(): Promise<void> {

    await refreshSyncTask('dailyPnl', true)

  }



  return {

    summary,

    balances,

    loading: request.loading,

    error: request.error,

    status: request.status,

    dailyPnl: dailyPnlRequest.state,

    dailyPnlLoading: dailyPnlRequest.loading,

    dailyPnlError: dailyPnlRequest.error,

    dailyPnlStatus: dailyPnlRequest.status,

    setBalance,

    applySnapshot,

    applyDailyPnlSnapshot,

    refreshAccount,

    refreshDailyPnl,

  }

})


