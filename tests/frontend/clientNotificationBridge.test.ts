import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { createClientNotification } from '../../src/services/notificationService'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

describe('client notification bridge service', () => {
  beforeEach(() => {
    vi.mocked(tauriInvoke).mockReset()
  })

  it('owns the exact one-request invoke envelope', async () => {
    vi.mocked(tauriInvoke).mockResolvedValue({
      notification: { id: '00000000-0000-4000-8000-000000000001' },
      unreadCount: 1,
      revision: '1',
    })
    const request = {
      accountId: 'primary',
      sessionEpoch: 7,
      attemptId: '10000000-0000-4000-8000-000000000001',
      kind: 'accountRecoveryFailed',
      failedSteps: ['connection'],
    } as const

    await expect(createClientNotification(request)).resolves.toMatchObject({
      notification: { id: '00000000-0000-4000-8000-000000000001' },
      unreadCount: 1,
      revision: '1',
    })
    expect(tauriInvoke).toHaveBeenCalledTimes(1)
    expect(tauriInvoke).toHaveBeenCalledWith('create_client_notification', { request })
  })
})
