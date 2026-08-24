import type { MessageApi } from 'naive-ui'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import {
  refreshSyncTask,
  startDataSync,
  stopDataSync,
  syncRunning,
} from '../../src/services/dataSyncService'
import { installMessageApi } from '../../src/services/errorService'
import { useLogStore } from '../../src/stores/log'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

function notifiedSessionError(): Error {
  return Object.assign(new Error('private refresh failed'), {
    cause: {
      code: 'AUTH_SESSION_EXPIRED',
      message: '账户会话已失效',
      notificationId: 'notification-data-sync-session',
    },
  })
}

describe('dataSyncService bridge', () => {
  let toastError: ReturnType<typeof vi.fn>

  beforeEach(() => {
    setActivePinia(createPinia())
    stopDataSync()
    vi.mocked(tauriInvoke).mockReset()
    toastError = vi.fn()
    installMessageApi({ error: toastError } as unknown as MessageApi)
  })

  it('does not start frontend interval scheduler', () => {
    startDataSync()
    expect(syncRunning()).toBe(false)
    stopDataSync()
  })

  it('owns one frontend log and Toast for a default swallowed ordinary failure', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('scheduler unavailable'))

    await expect(refreshSyncTask('privatePanels')).resolves.toBeUndefined()

    expect(useLogStore().entries).toHaveLength(1)
    expect(toastError).toHaveBeenCalledTimes(1)
  })

  it('swallows a valid notification marker without frontend redelivery', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(notifiedSessionError())

    await expect(refreshSyncTask('privatePanels')).resolves.toBeUndefined()

    expect(useLogStore().entries).toHaveLength(0)
    expect(toastError).toHaveBeenCalledTimes(0)
  })

  it.each([
    ['ordinary', new Error('account refresh failed'), 1, 1],
    ['notified', notifiedSessionError(), 0, 0],
  ])(
    'preserves rethrow and in-flight cleanup for %s failures',
    async (_label, error, expectedLogs, expectedToasts) => {
      vi.mocked(tauriInvoke).mockRejectedValueOnce(error).mockResolvedValueOnce(undefined)

      await expect(refreshSyncTask('account', true, true)).rejects.toBe(error)
      expect(useLogStore().entries).toHaveLength(expectedLogs)
      expect(toastError).toHaveBeenCalledTimes(expectedToasts)

      await expect(refreshSyncTask('account', true, true)).resolves.toBeUndefined()
      expect(tauriInvoke).toHaveBeenCalledTimes(2)
    },
  )
})
