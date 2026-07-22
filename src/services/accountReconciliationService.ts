export type AccountReconciliationStep =
  | 'config'
  | 'profiles'
  | 'connection'
  | 'bootstrap'

export interface AccountReconciliationTask {
  step: AccountReconciliationStep
  run: () => Promise<unknown>
}

const FAILURE_LABELS: Record<AccountReconciliationStep, string> = {
  config: 'Configuration refresh failed',
  profiles: 'Account profile refresh failed',
  connection: 'Connection status refresh failed',
  bootstrap: 'Account data bootstrap failed',
}

export async function collectReconciliationFailures(
  tasks: AccountReconciliationTask[],
): Promise<AccountReconciliationStep[]> {
  const results = await Promise.allSettled(tasks.map(({ run }) => run()))
  return results.flatMap((result, index) =>
    result.status === 'rejected' ? [tasks[index].step] : [],
  )
}

export function formatReconciliationError(
  steps: AccountReconciliationStep[],
): string | null {
  if (steps.length === 0) return null
  const details = steps.map((step) => FAILURE_LABELS[step]).join('; ')
  return `Account switched, but synchronization is incomplete: ${details}. Retry synchronization.`
}
