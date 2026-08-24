import { decodeCommandError } from './notificationService'

export type AccountReconciliationStep =
  | 'config'
  | 'profiles'
  | 'connection'
  | 'bootstrap'

export interface AccountReconciliationTask {
  step: AccountReconciliationStep
  run: () => Promise<unknown>
}

const RECONCILIATION_STEP_ORDER: readonly AccountReconciliationStep[] = [
  'config',
  'profiles',
  'connection',
  'bootstrap',
]

export function normalizeReconciliationSteps(
  steps: readonly AccountReconciliationStep[],
): AccountReconciliationStep[] {
  const included = new Set(steps)
  return RECONCILIATION_STEP_ORDER.filter((step) => included.has(step))
}

const FAILURE_LABELS: Record<AccountReconciliationStep, string> = {
  config: '配置刷新失败',
  profiles: '账户配置刷新失败',
  connection: '连接状态刷新失败',
  bootstrap: '账户数据初始化失败',
}

export async function collectReconciliationFailures(
  tasks: AccountReconciliationTask[],
): Promise<AccountReconciliationStep[]> {
  const results = await Promise.allSettled(tasks.map(({ run }) => run()))
  return normalizeReconciliationSteps(results.flatMap((result, index) =>
    result.status === 'rejected' && !decodeCommandError(result.reason).notificationId
      ? [tasks[index].step]
      : [],
  ))
}

export function formatReconciliationError(
  steps: AccountReconciliationStep[],
): string | null {
  if (steps.length === 0) return null
  const details = steps.map((step) => FAILURE_LABELS[step]).join('；')
  return `账户已切换，但同步未完成：${details}。请重试同步。`
}
