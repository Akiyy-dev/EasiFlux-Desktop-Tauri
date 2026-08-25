import { beforeEach, describe, expect, it, vi } from 'vitest'
import { listen } from '@tauri-apps/api/event'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import {
  clearAccountNotifications,
  createClientNotification,
  decodeCommandError,
  deleteNotification,
  getNotificationSettings,
  getNotificationSummary,
  listNotifications,
  listenNotificationChanges,
  markNotificationRead,
  markVisibleNotificationsRead,
  updateNotificationSettings,
} from '../../src/services/notificationService'
import type {
  NotificationChangedEvent,
  NotificationPage,
  NotificationRecord,
  NotificationSettings,
} from '../../src/types/notification'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const record: NotificationRecord = {
  id: '00000000-0000-4000-8000-000000000001',
  scope: { type: 'account', accountId: 'primary' },
  category: 'trading',
  kind: 'orderFilled',
  severity: 'success',
  content: {
    messageKey: 'order.filled',
    params: { orderId: 'order-1', maker: true, price: 100.25 },
    fallbackTitle: '订单已完全成交',
    fallbackBody: '订单已完全成交，可前往交易页查看。',
  },
  entity: { type: 'order', id: 'order-1' },
  action: { type: 'openTrading', orderId: 'order-1' },
  sourceEventId: 'order:order-1:filled',
  dedupeKey: 'primary:order-1:filled',
  occurrenceCount: 1,
  createdAtMs: 1_700_000_000_000,
  updatedAtMs: 1_700_000_000_000,
}

const page: NotificationPage = {
  items: [record],
  nextCursor: 'n1.opaque-cursor-do-not-parse',
  unreadCount: 1,
  revision: '90071992547409931234567890',
}

const settings: NotificationSettings = {
  tradingToast: true,
  riskAccountToast: false,
  connectionSystemToast: true,
}

const changedEvent: NotificationChangedEvent = {
  previousRevision: '90071992547409931234567889',
  revision: '90071992547409931234567890',
  change: 'created',
  affectedScopes: [{ type: 'account', accountId: 'primary' }],
  notificationId: record.id,
  toastCandidate: {
    id: record.id,
    scope: record.scope,
    sessionEpoch: 7,
    category: record.category,
    severity: record.severity,
    content: record.content,
    action: record.action,
  },
}

describe('notification IPC service', () => {
  beforeEach(() => {
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(listen).mockReset()
  })

  it('sends the nested list request with adapter-owned default limit and passes opaque strings through', async () => {
    vi.mocked(tauriInvoke).mockResolvedValue(page)

    await expect(listNotifications({
      accountId: 'primary',
      filter: 'all',
      cursor: undefined,
    })).resolves.toBe(page)

    expect(tauriInvoke).toHaveBeenCalledWith('list_notifications', {
      request: { accountId: 'primary', filter: 'all', cursor: undefined, limit: 50 },
    })
  })

  it('preserves explicit Global-only context, opaque cursor, and limit', async () => {
    vi.mocked(tauriInvoke).mockResolvedValue(page)

    await listNotifications({
      accountId: null,
      filter: 'unread',
      cursor: 'n1.global-unread',
      limit: 10,
    })

    expect(tauriInvoke).toHaveBeenCalledWith('list_notifications', {
      request: {
        accountId: null,
        filter: 'unread',
        cursor: 'n1.global-unread',
        limit: 10,
      },
    })
  })

  it('uses exact envelopes for summary and all five mutation commands', async () => {
    vi.mocked(tauriInvoke)
      .mockResolvedValueOnce({ unreadCount: 2, revision: 'r-summary' })
      .mockResolvedValueOnce({ notification: record, unreadCount: 1, revision: 'r-read' })
      .mockResolvedValueOnce({
        affectedCount: 2,
        affectedScopes: [{ type: 'global' }, { type: 'account', accountId: 'primary' }],
        unreadCount: 0,
        revision: 'r-visible',
      })
      .mockResolvedValueOnce({ unreadCount: 0, revision: 'r-delete' })
      .mockResolvedValueOnce({ affectedCount: 3, unreadCount: 0, revision: 'r-clear' })

    await getNotificationSummary(null)
    await markNotificationRead('primary', record.id)
    await markVisibleNotificationsRead('primary')
    await deleteNotification(null, record.id)
    await clearAccountNotifications('primary')

    expect(vi.mocked(tauriInvoke).mock.calls).toEqual([
      ['get_notification_summary', { accountId: null }],
      ['mark_notification_read', { contextAccountId: 'primary', id: record.id }],
      ['mark_visible_notifications_read', { contextAccountId: 'primary' }],
      ['delete_notification', { contextAccountId: null, id: record.id }],
      ['clear_account_notifications', { accountId: 'primary' }],
    ])
  })

  it('preserves the Task 7 bridge request envelope while returning a full record', async () => {
    const receipt = { notification: record, unreadCount: 1, revision: 'r-client' }
    vi.mocked(tauriInvoke).mockResolvedValue(receipt)
    const request = {
      accountId: 'primary',
      sessionEpoch: 7,
      attemptId: '10000000-0000-4000-8000-000000000001',
      kind: 'accountRecoveryFailed',
      failedSteps: ['connection', 'bootstrap'],
    } as const

    await expect(createClientNotification(request)).resolves.toBe(receipt)
    expect(tauriInvoke).toHaveBeenCalledWith('create_client_notification', { request })
  })

  it('uses independent narrow notification settings commands', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(settings).mockResolvedValueOnce(settings)

    await expect(getNotificationSettings()).resolves.toBe(settings)
    await expect(updateNotificationSettings(settings)).resolves.toBe(settings)

    expect(vi.mocked(tauriInvoke).mock.calls).toEqual([
      ['get_notification_settings'],
      ['update_notification_settings', { settings }],
    ])
  })

  it('uses direct Tauri listening, forwards only the payload, and returns the real unlisten', async () => {
    const unlisten = vi.fn()
    let callback: ((event: { payload: NotificationChangedEvent }) => void) | undefined
    vi.mocked(listen).mockImplementation(async (_name, handler) => {
      callback = handler as (event: { payload: NotificationChangedEvent }) => void
      return unlisten
    })
    const handler = vi.fn()

    const stop = await listenNotificationChanges(handler)
    callback?.({ payload: changedEvent })
    stop()

    expect(listen).toHaveBeenCalledWith('notification:changed', expect.any(Function))
    expect(handler).toHaveBeenCalledOnce()
    expect(handler).toHaveBeenCalledWith(changedEvent)
    expect(unlisten).toHaveBeenCalledOnce()
  })

  it('rejects malformed command responses and ignores malformed event envelopes at the boundary', async () => {
    vi.mocked(tauriInvoke).mockResolvedValue({ ...page, revision: 42 })
    let callback: ((event: { payload: unknown }) => void) | undefined
    vi.mocked(listen).mockImplementation(async (_name, handler) => {
      callback = handler as (event: { payload: unknown }) => void
      return vi.fn()
    })
    const handler = vi.fn()

    await expect(listNotifications({ accountId: null, filter: 'all' }))
      .rejects.toThrow('INVALID_NOTIFICATION_RESPONSE')
    await listenNotificationChanges(handler)
    callback?.({ payload: { ...changedEvent, affectedScopes: [{ type: 'other' }] } })

    expect(handler).not.toHaveBeenCalled()
  })
})

describe('decodeCommandError', () => {
  it('preserves structured fields and opaque marker strings exactly', () => {
    expect(decodeCommandError({
      code: 'ORDER_REJECTED',
      message: '订单被拒绝',
      eventId: ' event:id:01 ',
      notificationId: '00000000-0000-4000-8000-000000000001',
      ignored: 'value',
    })).toEqual({
      code: 'ORDER_REJECTED',
      message: '订单被拒绝',
      eventId: ' event:id:01 ',
      notificationId: '00000000-0000-4000-8000-000000000001',
    })
  })

  it('keeps legacy string errors marker-free without matching prose', () => {
    expect(decodeCommandError('legacy backend failure')).toEqual({
      message: 'legacy backend failure',
    })
  })

  it('drops untrusted non-string markers instead of inventing typed ownership', () => {
    expect(decodeCommandError({
      code: 'BACKEND_FAILURE',
      message: 'failed',
      eventId: 7,
      notificationId: null,
    })).toEqual({ code: 'BACKEND_FAILURE', message: 'failed' })
  })

  it('preserves controlled markers attached to an Error object', () => {
    const error = Object.assign(new Error('structured error'), {
      code: 'STRUCTURED_FAILURE',
      eventId: 'event-1',
      notificationId: 'notification-1',
    })

    expect(decodeCommandError(error)).toEqual({
      code: 'STRUCTURED_FAILURE',
      message: 'structured error',
      eventId: 'event-1',
      notificationId: 'notification-1',
    })
  })

  it('follows wrapped causes to the first structured notification marker', () => {
    const notificationFailure = {
      code: 'CONNECTION_UNAVAILABLE',
      message: '连接服务暂时不可用',
      notificationId: 'notification-connection-1',
    }
    const middle = new Error('middle wrapper') as Error & { cause?: unknown }
    middle.cause = notificationFailure
    const outer = new Error('outer wrapper') as Error & { cause?: unknown }
    outer.cause = middle

    expect(decodeCommandError(outer)).toEqual(notificationFailure)
  })

  it('does not let an outer non-notification code hide a notified cause', () => {
    const notificationFailure = {
      code: 'CONNECTION_UNAVAILABLE',
      message: '连接服务暂时不可用',
      notificationId: 'notification-connection-2',
    }
    const outer = Object.assign(new Error('outer wrapper'), {
      code: 'WRAPPED_CONNECTION_FAILURE',
      cause: notificationFailure,
    })

    expect(decodeCommandError(outer)).toEqual(notificationFailure)
  })

  it('keeps cyclic and unstructured cause chains ordinary', () => {
    const cyclic = new Error('ordinary connection failure') as Error & { cause?: unknown }
    cyclic.cause = { message: 'nested ordinary failure', cause: cyclic }

    expect(decodeCommandError(cyclic)).toEqual({ message: 'ordinary connection failure' })
    expect(decodeCommandError({
      message: 'invalid marker',
      notificationId: '',
    })).toEqual({ message: 'invalid marker' })
  })
})
