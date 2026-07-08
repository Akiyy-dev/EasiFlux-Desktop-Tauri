import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { AccountSummary, Balance } from '../types/models'
import { useAsyncState } from '../composables/useAsyncState'

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

  function setBalance(balance: Balance): void {
    const idx = balances.value.findIndex((b) => b.asset === balance.asset)
    if (idx >= 0) {
      balances.value[idx] = balance
    } else {
      balances.value.push(balance)
    }
    summary.value = syncSummaryBalances(summary.value, balances.value)
  }

  async function refreshAccount(): Promise<void> {
    summary.value = await request.run(() => tauriInvoke<AccountSummary>('refresh_account'))
    balances.value = summary.value.balances
  }

  return {
    summary,
    balances,
    loading: request.loading,
    error: request.error,
    status: request.status,
    setBalance,
    refreshAccount,
  }
})
