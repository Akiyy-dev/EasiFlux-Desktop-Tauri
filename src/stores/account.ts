import { defineStore } from 'pinia'
import { ref } from 'vue'
import { tauriInvoke } from '../composables/useTauriCommand'
import type { AccountSummary, Balance } from '../types/models'

export const useAccountStore = defineStore('account', () => {
  const summary = ref<AccountSummary | null>(null)
  const balances = ref<Balance[]>([])

  function setBalance(balance: Balance): void {
    const idx = balances.value.findIndex((b) => b.asset === balance.asset)
    if (idx >= 0) {
      balances.value[idx] = balance
    } else {
      balances.value.push(balance)
    }
  }

  async function refreshAccount(): Promise<void> {
    summary.value = await tauriInvoke<AccountSummary>('refresh_account')
    balances.value = summary.value.balances
  }

  return { summary, balances, setBalance, refreshAccount }
})
