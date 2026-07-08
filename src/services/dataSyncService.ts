import { refreshPrivatePanels } from '../stores/privatePanels'
import { useAccountStore } from '../stores/account'
import { useAppStore } from '../stores/app'
import { useConfigStore } from '../stores/config'
import { useLogStore } from '../stores/log'
import { useMarketStore } from '../stores/market'

export type SyncTaskName = 'market' | 'account' | 'privatePanels' | 'environment'

type SyncTask = {
  label: string
  run: () => Promise<void>
}

const tasks: Record<SyncTaskName, SyncTask> = {
  market: {
    label: '行情',
    run: async () => {
      await useMarketStore().refreshMarket()
    },
  },
  account: {
    label: '账户',
    run: async () => {
      await useAccountStore().refreshAccount()
    },
  },
  privatePanels: {
    label: '订单/持仓',
    run: async () => {
      await refreshPrivatePanels()
    },
  },
  environment: {
    label: '环境',
    run: async () => {
      await useAppStore().refreshEnvironment()
    },
  },
}

let syncTimer: ReturnType<typeof globalThis.setInterval> | null = null
let syncing = false
const inFlight = new Set<SyncTaskName>()

function formatError(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return '同步失败'
}

function intervalMs(): number {
  const seconds = useConfigStore().config?.tickerPollInterval ?? 1
  return Math.max(1, seconds) * 1000
}

export function syncRunning(): boolean {
  return syncTimer != null
}

export async function refreshSyncTask(name: SyncTaskName): Promise<void> {
  if (inFlight.has(name)) {
    return
  }
  inFlight.add(name)
  const task = tasks[name]
  try {
    await task.run()
  } catch (error) {
    useLogStore().setError(`${task.label}刷新失败: ${formatError(error)}`)
  } finally {
    inFlight.delete(name)
  }
}

export async function refreshAllSyncTasks(): Promise<void> {
  if (syncing) {
    return
  }
  syncing = true
  try {
    for (const name of Object.keys(tasks) as SyncTaskName[]) {
      await refreshSyncTask(name)
    }
  } finally {
    syncing = false
  }
}

export function startDataSync(): void {
  stopDataSync()
  void refreshAllSyncTasks()
  syncTimer = globalThis.setInterval(() => {
    void refreshAllSyncTasks()
  }, intervalMs())
}

export function stopDataSync(): void {
  if (syncTimer) {
    globalThis.clearInterval(syncTimer)
    syncTimer = null
  }
  syncing = false
  inFlight.clear()
}
