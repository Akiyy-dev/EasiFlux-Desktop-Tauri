import type { MessageApi } from 'naive-ui'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { reportError } from '../../src/services/errorService'
import {
  installNotificationToastApi,
  shouldToast,
  showNotificationToast,
} from '../../src/services/notificationToastService'
import { useLogStore } from '../../src/stores/log'
import type {
  NotificationSettings,
  NotificationToastCandidate,
} from '../../src/types/notification'

const mocks = vi.hoisted(() => ({
  setError: vi.fn(),
}))

vi.mock('../../src/stores/log', () => ({
  useLogStore: vi.fn(() => ({ setError: mocks.setError })),
}))
vi.mock('../../src/services/errorService', () => ({ reportError: vi.fn() }))
vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

const settings: NotificationSettings = {
  tradingToast: true,
  riskAccountToast: false,
  connectionSystemToast: true,
}

const candidate: NotificationToastCandidate = {
  id: '00000000-0000-4000-8000-000000000001',
  scope: { type: 'account', accountId: 'primary' },
  sessionEpoch: 7,
  category: 'trading',
  severity: 'success',
  content: {
    messageKey: 'order.filled',
    params: { orderId: 'order-1' },
    fallbackTitle: '订单已完全成交',
    fallbackBody: '订单已完全成交，可前往交易页查看。',
  },
  action: { type: 'openTrading', orderId: 'order-1' },
}

describe('notification Toast gating', () => {
  it('applies the committed category toggle for current Account and Global candidates', () => {
    expect(shouldToast(candidate, settings, 'primary')).toBe(true)
    expect(shouldToast({ ...candidate, scope: { type: 'global' } }, settings, 'primary')).toBe(true)
    expect(shouldToast({ ...candidate, scope: { type: 'global' } }, settings, null)).toBe(true)
    expect(shouldToast({ ...candidate, scope: { type: 'account', accountId: 'backup' } }, settings, 'primary')).toBe(false)
    expect(shouldToast(candidate, settings, null)).toBe(false)
  })

  it('maps every category to only its own committed setting', () => {
    expect(shouldToast({ ...candidate, category: 'trading' }, settings, 'primary')).toBe(true)
    expect(shouldToast({ ...candidate, category: 'riskAccount' }, settings, 'primary')).toBe(false)
    expect(shouldToast({ ...candidate, category: 'connectionSystem' }, settings, 'primary')).toBe(true)
  })

  it('keeps Critical distinct while still obeying its disabled category', () => {
    expect(shouldToast({
      ...candidate,
      category: 'riskAccount',
      severity: 'critical',
    }, settings, 'primary')).toBe(false)
  })
})

describe('notification Toast provider', () => {
  const message = {
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
  } as unknown as MessageApi

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('is a safe no-op before a MessageApi is installed', () => {
    expect(() => showNotificationToast(candidate)).not.toThrow()
    expect(message.success).not.toHaveBeenCalled()
  })

  it.each([
    ['success', 'success'],
    ['info', 'info'],
    ['warning', 'warning'],
    ['error', 'error'],
    ['critical', 'error'],
  ] as const)('maps %s severity to MessageApi.%s', (severity, method) => {
    installNotificationToastApi(message)

    showNotificationToast({ ...candidate, severity })

    expect(message[method]).toHaveBeenCalledWith(
      '订单已完全成交：订单已完全成交，可前往交易页查看。',
    )
  })

  it('never logs, reports generic errors, invokes commands, navigates, or changes focus', () => {
    const input = document.createElement('input')
    document.body.appendChild(input)
    input.focus()
    const pushState = vi.spyOn(window.history, 'pushState')
    installNotificationToastApi(message)

    showNotificationToast(candidate)

    expect(document.activeElement).toBe(input)
    expect(useLogStore).not.toHaveBeenCalled()
    expect(mocks.setError).not.toHaveBeenCalled()
    expect(reportError).not.toHaveBeenCalled()
    expect(tauriInvoke).not.toHaveBeenCalled()
    expect(pushState).not.toHaveBeenCalled()
    input.remove()
    pushState.mockRestore()
  })
})
