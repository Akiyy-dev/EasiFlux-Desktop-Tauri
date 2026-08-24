import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import ErrorToastBridge from '../../src/components/common/ErrorToastBridge.vue'
import {
  installMessageApi,
  reportError,
  reportFrontendError,
  showBackendError,
} from '../../src/services/errorService'
import {
  showNotificationToast,
} from '../../src/services/notificationToastService'
import { useLogStore } from '../../src/stores/log'
import type { NotificationToastCandidate } from '../../src/types/notification'

const mocks = vi.hoisted(() => ({
  message: {
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
  },
}))

vi.mock('naive-ui', async (importOriginal) => ({
  ...await importOriginal<typeof import('naive-ui')>(),
  useMessage: () => mocks.message,
}))

const candidate: NotificationToastCandidate = {
  id: '00000000-0000-4000-8000-000000000001',
  scope: { type: 'global' },
  category: 'connectionSystem',
  severity: 'warning',
  content: {
    messageKey: 'connection.unavailable',
    params: { channel: 'api' },
    fallbackTitle: '连接不可用',
    fallbackBody: '交易连接暂时不可用，请检查网络或稍后重试。',
  },
}

describe('error delivery ownership', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    installMessageApi(mocks.message as never)
  })

  it('reports a frontend error with exactly one frontend log and one Toast', () => {
    const message = reportFrontendError(new Error('disk full'), '保存失败')

    expect(message).toBe('保存失败: disk full')
    expect(useLogStore().entries).toEqual([
      expect.objectContaining({ level: 'error', message: '保存失败: disk full' }),
    ])
    expect(mocks.message.error).toHaveBeenCalledOnce()
    expect(mocks.message.error).toHaveBeenCalledWith('保存失败: disk full')
  })

  it('keeps reportError as frontend-owned compatibility behavior', () => {
    reportError('legacy command failed', '旧调用')

    expect(useLogStore().entries).toHaveLength(1)
    expect(mocks.message.error).toHaveBeenCalledOnce()
  })

  it('shows an already-logged backend error once per opaque event ID without adding a log', () => {
    const event = { eventId: 'backend:event:opaque-1', message: '后台任务失败' }

    showBackendError(event)
    showBackendError(event)

    expect(mocks.message.error).toHaveBeenCalledOnce()
    expect(mocks.message.error).toHaveBeenCalledWith('后台任务失败')
    expect(useLogStore().entries).toEqual([])
  })

  it('drops malformed backend envelopes at the adapter boundary', () => {
    showBackendError({ eventId: 7, message: 'untrusted' } as never)
    showBackendError({ eventId: 'valid', message: null } as never)

    expect(mocks.message.error).not.toHaveBeenCalled()
    expect(useLogStore().entries).toEqual([])
  })

  it('notification Toast delivery never enters the generic error channel or log store', () => {
    mount(ErrorToastBridge)

    showNotificationToast(candidate)

    expect(mocks.message.warning).toHaveBeenCalledOnce()
    expect(mocks.message.error).not.toHaveBeenCalled()
    expect(useLogStore().entries).toEqual([])
  })

  it('the provider bridge installs the same MessageApi into both adapters', () => {
    mount(ErrorToastBridge)

    reportFrontendError('frontend')
    showNotificationToast({ ...candidate, severity: 'success' })

    expect(mocks.message.error).toHaveBeenCalledWith('frontend')
    expect(mocks.message.success).toHaveBeenCalledWith(
      '连接不可用：交易连接暂时不可用，请检查网络或稍后重试。',
    )
  })
})
