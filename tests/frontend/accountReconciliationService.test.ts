import { describe, expect, it, vi } from 'vitest'
import {
  collectReconciliationFailures,
  type AccountReconciliationTask,
} from '../../src/services/accountReconciliationService'

const sessionMarker = {
  message: 'bootstrap failed',
  cause: {
    code: 'AUTH_SESSION_EXPIRED',
    message: '账户会话已失效',
    notificationId: 'committed-session-id',
  },
}

describe('account reconciliation delivery ownership', () => {
  it('does not turn a committed session marker into a reconciliation failure', async () => {
    const tasks: AccountReconciliationTask[] = [
      { step: 'bootstrap', run: vi.fn(() => Promise.reject(sessionMarker)) },
    ]

    await expect(collectReconciliationFailures(tasks)).resolves.toEqual([])
  })

  it('reports only independent ordinary failures when a session marker is mixed in', async () => {
    const tasks: AccountReconciliationTask[] = [
      { step: 'config', run: vi.fn(() => Promise.reject(new Error('ordinary'))) },
      { step: 'bootstrap', run: vi.fn(() => Promise.reject(sessionMarker)) },
    ]

    await expect(collectReconciliationFailures(tasks)).resolves.toEqual(['config'])
  })
})
