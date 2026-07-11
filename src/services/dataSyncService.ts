import { tauriInvoke } from '../composables/useTauriCommand'
import { useLogStore } from '../stores/log'

export type SyncTaskName =
  | 'market'
  | 'account'
  | 'privatePanels'
  | 'environment'
  | 'dailyPnl'

const TASK_LABELS: Record<SyncTaskName, string> = {
  market: '行情',
  account: '账户',
  privatePanels: '订单/持仓',
  environment: '环境',
  dailyPnl: '今日盈亏',
}

const inFlight = new Set<SyncTaskName>()

function formatError(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return '同步失败'
}

export function syncRunning(): boolean {
  return false
}

export async function refreshSyncTask(name: SyncTaskName, force = false): Promise<void> {
  if (inFlight.has(name)) {
    return
  }
  inFlight.add(name)
  try {
    await tauriInvoke('scheduler_run_task', { task: name, force })
  } catch (error) {
    useLogStore().setError(`${TASK_LABELS[name]}刷新失败: ${formatError(error)}`)
  } finally {
    inFlight.delete(name)
  }
}

export async function refreshAllSyncTasks(force = false): Promise<void> {
  for (const name of Object.keys(TASK_LABELS) as SyncTaskName[]) {
    await refreshSyncTask(name, force)
  }
}

export function startDataSync(): void {
  // Scheduler runs in Rust; frontend only bridges force refresh.
}

export function stopDataSync(): void {
  inFlight.clear()
}
