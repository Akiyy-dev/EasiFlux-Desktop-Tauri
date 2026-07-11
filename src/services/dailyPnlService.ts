import { tauriInvoke } from '../composables/useTauriCommand'
import type { DailyPnlSnapshot } from '../types/models'

export type { DailyPnlSnapshot }

export function applyDailyPnlSnapshot(snapshot: DailyPnlSnapshot): DailyPnlSnapshot {
  return snapshot
}

export async function fetchTodayRealizedPnl(): Promise<DailyPnlSnapshot> {
  await tauriInvoke('scheduler_run_task', { task: 'dailyPnl', force: true })
  throw new Error('今日盈亏由后端快照事件推送')
}
