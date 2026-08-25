import { flushPromises, shallowMount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  NotificationChangedEvent,
  NotificationPage,
  NotificationRecord,
  NotificationSettings,
  NotificationToastCandidate,
} from '../../src/types/notification'

const mocks = vi.hoisted(() => ({
  listen: vi.fn(),
  summary: vi.fn(),
  list: vi.fn(),
  markRead: vi.fn(),
  markAllRead: vi.fn(),
  remove: vi.fn(),
  clear: vi.fn(),
  createClient: vi.fn(),
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
  shouldToast: vi.fn(),
  showToast: vi.fn(),
  invoke: vi.fn(),
  whenListenersReady: vi.fn(),
  useTauriEvent: vi.fn(),
  useAccountSessionEvent: vi.fn(),
  closeGuard: vi.fn(),
  reportError: vi.fn(),
  showBackendError: vi.fn(),
  listener: undefined as ((event: NotificationChangedEvent) => void) | undefined,
}))

vi.mock('../../src/services/notificationService', () => ({
  listenNotificationChanges: mocks.listen,
  getNotificationSummary: mocks.summary,
  listNotifications: mocks.list,
  markNotificationRead: mocks.markRead,
  markVisibleNotificationsRead: mocks.markAllRead,
  deleteNotification: mocks.remove,
  clearAccountNotifications: mocks.clear,
  createClientNotification: mocks.createClient,
  getNotificationSettings: mocks.getSettings,
  updateNotificationSettings: mocks.updateSettings,
  decodeCommandError: (error: unknown) => ({
    message: typeof error === 'object' && error !== null && 'message' in error
      ? String(error.message)
      : String(error),
  }),
}))

vi.mock('../../src/services/notificationToastService', () => ({
  shouldToast: mocks.shouldToast,
  showNotificationToast: mocks.showToast,
}))

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: mocks.invoke }))
vi.mock('../../src/composables/useTauriEvent', () => ({
  whenTauriListenersReady: mocks.whenListenersReady,
  useTauriEvent: mocks.useTauriEvent,
}))
vi.mock('../../src/composables/useAccountSessionEvent', () => ({
  useAccountSessionEvent: mocks.useAccountSessionEvent,
}))
vi.mock('../../src/composables/useChartWorkspaceCloseGuard', () => ({
  useChartWorkspaceCloseGuard: mocks.closeGuard,
}))
vi.mock('../../src/services/errorService', () => ({
  reportError: mocks.reportError,
  showBackendError: mocks.showBackendError,
  notifyWarning: vi.fn(),
}))
vi.mock('../../src/components/layout/AppShell.vue', () => ({
  default: { template: '<div />' },
}))
vi.mock('../../src/components/common/ErrorToastBridge.vue', () => ({
  default: { template: '<div />' },
}))
vi.mock('../../src/components/settings/QuickSetupDialog.vue', () => ({
  default: { template: '<div />' },
}))

import App from '../../src/App.vue'
import { useConfigStore } from '../../src/stores/config'
import { useNotificationStore } from '../../src/stores/notification'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((done, fail) => {
    resolve = done
    reject = fail
  })
  return { promise, resolve, reject }
}

let idCounter = 0

function record(
  id = `notification-${++idCounter}`,
  scope: NotificationRecord['scope'] = { type: 'account', accountId: 'primary' },
): NotificationRecord {
  return {
    id,
    scope,
    category: 'trading',
    kind: 'orderFilled',
    severity: 'success',
    content: {
      messageKey: 'order.filled',
      params: { orderId: id },
      fallbackTitle: '订单已完全成交',
      fallbackBody: '订单已完全成交，可前往交易页查看。',
    },
    action: { type: 'openTrading', orderId: id },
    sourceEventId: `source:${id}`,
    dedupeKey: `dedupe:${id}`,
    occurrenceCount: 1,
    createdAtMs: 1_700_000_000_000 + idCounter,
    updatedAtMs: 1_700_000_000_000 + idCounter,
  }
}

function page(
  items: NotificationRecord[],
  revision = 'revision-1',
  nextCursor?: string,
  unreadCount = items.filter((item) => item.readAtMs === undefined).length,
): NotificationPage {
  return { items, nextCursor, unreadCount, revision }
}

const enabledSettings: NotificationSettings = {
  tradingToast: true,
  riskAccountToast: true,
  connectionSystemToast: true,
}

function candidate(
  id = `candidate-${++idCounter}`,
  scope: NotificationToastCandidate['scope'] = { type: 'account', accountId: 'primary' },
  category: NotificationToastCandidate['category'] = 'trading',
): NotificationToastCandidate {
  return {
    id,
    scope,
    category,
    severity: 'success',
    content: {
      messageKey: 'order.filled',
      params: { orderId: id },
      fallbackTitle: '订单已完全成交',
      fallbackBody: '订单已完全成交，可前往交易页查看。',
    },
  }
}

function changed(
  previousRevision: string,
  revision: string,
  affectedScopes: NotificationChangedEvent['affectedScopes'],
  options: Partial<NotificationChangedEvent> = {},
): NotificationChangedEvent {
  return {
    previousRevision,
    revision,
    change: 'updated',
    affectedScopes,
    ...options,
  }
}

async function tick(): Promise<void> {
  await Promise.resolve()
  await Promise.resolve()
}

function emit(event: NotificationChangedEvent): void {
  if (!mocks.listener) throw new Error('notification listener not installed')
  mocks.listener(event)
}

describe('notification store lifecycle', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.resetAllMocks()
    mocks.listener = undefined
    mocks.listen.mockImplementation(async (handler: (event: NotificationChangedEvent) => void) => {
      mocks.listener = handler
      return vi.fn()
    })
    mocks.summary.mockResolvedValue({ unreadCount: 2, revision: 'revision-1' })
    mocks.list.mockResolvedValue(page([], 'revision-1'))
    mocks.getSettings.mockResolvedValue(enabledSettings)
    mocks.updateSettings.mockImplementation(async (settings: NotificationSettings) => settings)
    mocks.shouldToast.mockImplementation((toastCandidate, settings, activeAccountId) => {
      const visible = toastCandidate.scope.type === 'global'
        || (activeAccountId !== null && toastCandidate.scope.accountId === activeAccountId)
      if (!visible) return false
      if (toastCandidate.category === 'trading') return settings.tradingToast
      if (toastCandidate.category === 'riskAccount') return settings.riskAccountToast
      return settings.connectionSystemToast
    })
    mocks.whenListenersReady.mockResolvedValue(undefined)
  })

  it('starts once, installs the listener before Global summary/settings, and loads no history', async () => {
    const order: string[] = []
    const listen = deferred<() => void>()
    mocks.listen.mockImplementation((handler: (event: NotificationChangedEvent) => void) => {
      order.push('listen')
      mocks.listener = handler
      return listen.promise
    })
    mocks.summary.mockImplementation(async (accountId: string | null) => {
      order.push(`summary:${String(accountId)}`)
      return { unreadCount: 2, revision: 'revision-1' }
    })
    mocks.getSettings.mockImplementation(async () => {
      order.push('settings')
      return enabledSettings
    })
    const store = useNotificationStore()

    const first = store.start()
    const second = store.start()
    expect(order).toEqual(['listen'])
    expect(mocks.listen).toHaveBeenCalledOnce()
    listen.resolve(vi.fn())
    await Promise.all([first, second])

    expect(order[0]).toBe('listen')
    expect(order.slice(1).sort()).toEqual(['settings', 'summary:null'])
    expect(mocks.list).not.toHaveBeenCalled()
    expect(store.accountId).toBeNull()
    expect(store.unreadCount).toBe(2)
    expect(store.observedRevision).toBe('revision-1')
    expect(store.settingsDraft).toEqual(enabledSettings)
    expect(store.settingsCommitted).toEqual(enabledSettings)
    expect(store.settingsDraft).not.toBe(store.settingsCommitted)
  })

  it('unlistens exactly once across repeated stop calls', async () => {
    const unlisten = vi.fn()
    mocks.listen.mockResolvedValue(unlisten)
    const store = useNotificationStore()
    await store.start()

    store.stop()
    store.stop()

    expect(unlisten).toHaveBeenCalledOnce()
  })

  it('cleans a listener that resolves after stop without installing it or starting queries', async () => {
    const pending = deferred<() => void>()
    const unlisten = vi.fn()
    mocks.listen.mockReturnValue(pending.promise)
    const store = useNotificationStore()
    const starting = store.start()

    store.stop()
    pending.resolve(unlisten)
    await starting

    expect(unlisten).toHaveBeenCalledOnce()
    expect(mocks.summary).not.toHaveBeenCalled()
    expect(mocks.getSettings).not.toHaveBeenCalled()
  })

  it('lets a new lifecycle start while the stopped listener is still resolving and ignores its callback', async () => {
    const firstListen = deferred<() => void>()
    const firstUnlisten = vi.fn()
    const secondUnlisten = vi.fn()
    let oldHandler: ((event: NotificationChangedEvent) => void) | undefined
    mocks.listen
      .mockImplementationOnce((handler: (event: NotificationChangedEvent) => void) => {
        oldHandler = handler
        return firstListen.promise
      })
      .mockImplementationOnce(async (handler: (event: NotificationChangedEvent) => void) => {
        mocks.listener = handler
        return secondUnlisten
      })
    const store = useNotificationStore()
    const oldStart = store.start()
    store.stop()

    const newStart = store.start()
    expect(mocks.listen).toHaveBeenCalledTimes(2)
    await newStart
    firstListen.resolve(firstUnlisten)
    await oldStart
    oldHandler?.(changed('revision-1', 'stale-revision', [{ type: 'global' }]))

    expect(firstUnlisten).toHaveBeenCalledOnce()
    expect(store.observedRevision).toBe('revision-1')
    store.stop()
    expect(secondUnlisten).toHaveBeenCalledOnce()
  })

  it('keeps listener failure local, initializes summary/settings, and retries only listening', async () => {
    mocks.listen.mockRejectedValueOnce(new Error('listen failed'))
    const store = useNotificationStore()

    await expect(store.start()).resolves.toBeUndefined()

    expect(store.error).toBe('listen failed')
    expect(store.unreadCount).toBe(2)
    expect(store.settingsCommitted).toEqual(enabledSettings)
    expect(mocks.summary).toHaveBeenCalledOnce()
    expect(mocks.getSettings).toHaveBeenCalledOnce()

    const unlisten = vi.fn()
    mocks.listen.mockResolvedValueOnce(unlisten)
    await store.start()

    expect(mocks.listen).toHaveBeenCalledTimes(2)
    expect(mocks.summary).toHaveBeenCalledOnce()
    expect(mocks.getSettings).toHaveBeenCalledOnce()
    store.stop()
    expect(unlisten).toHaveBeenCalledOnce()
  })

  it('invalidates pending summary and settings completions on stop', async () => {
    const summary = deferred<{ unreadCount: number; revision: string }>()
    const settings = deferred<NotificationSettings>()
    mocks.summary.mockReturnValue(summary.promise)
    mocks.getSettings.mockReturnValue(settings.promise)
    const store = useNotificationStore()
    const starting = store.start()
    await tick()

    store.stop()
    summary.resolve({ unreadCount: 9, revision: 'stale' })
    settings.resolve(enabledSettings)
    await starting

    expect(store.unreadCount).toBeNull()
    expect(store.observedRevision).toBeNull()
    expect(store.settingsCommitted).toBeNull()
  })

  it('restarts the same Store instance from fresh Global-only state while retaining replay IDs', async () => {
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    await store.setFilter('unread')
    const replayed = candidate('seen-across-restart')
    emit(changed('revision-1', 'revision-2', [replayed.scope], {
      change: 'created', notificationId: replayed.id, toastCandidate: replayed,
    }))
    expect(mocks.showToast).toHaveBeenCalledOnce()

    store.stop()
    const summary = deferred<{ unreadCount: number; revision: string }>()
    const settings = deferred<NotificationSettings>()
    mocks.summary.mockReturnValueOnce(summary.promise)
    mocks.getSettings.mockReturnValueOnce(settings.promise)
    const restarted = store.start()
    await tick()

    expect(mocks.summary).toHaveBeenLastCalledWith(null)
    expect(store.accountId).toBeNull()
    expect(store.filter).toBe('all')
    expect(store.items).toEqual([])
    expect(store.unreadCount).toBeNull()
    expect(store.observedRevision).toBeNull()
    expect(store.settingsDraft).toBeNull()
    expect(store.settingsCommitted).toBeNull()

    summary.resolve({ unreadCount: 0, revision: 'fresh-global' })
    settings.resolve(enabledSettings)
    await restarted
    await store.setAccount('primary')
    emit(changed('fresh-global', 'fresh-replay', [replayed.scope], {
      change: 'created', notificationId: replayed.id, toastCandidate: replayed,
    }))

    expect(mocks.showToast).toHaveBeenCalledOnce()
  })
})

describe('notification store context, paging, revisions, and Toasts', () => {
  beforeEach(async () => {
    setActivePinia(createPinia())
    vi.resetAllMocks()
    mocks.listener = undefined
    mocks.listen.mockImplementation(async (handler: (event: NotificationChangedEvent) => void) => {
      mocks.listener = handler
      return vi.fn()
    })
    mocks.summary.mockResolvedValue({ unreadCount: 0, revision: 'revision-1' })
    mocks.list.mockResolvedValue(page([], 'revision-1'))
    mocks.getSettings.mockResolvedValue(enabledSettings)
    mocks.updateSettings.mockImplementation(async (settings: NotificationSettings) => settings)
    mocks.shouldToast.mockImplementation((toastCandidate, settings, activeAccountId) => {
      const visible = toastCandidate.scope.type === 'global'
        || (activeAccountId !== null && toastCandidate.scope.accountId === activeAccountId)
      const enabled = toastCandidate.category === 'trading'
        ? settings.tradingToast
        : toastCandidate.category === 'riskAccount'
          ? settings.riskAccountToast
          : settings.connectionSystemToast
      return visible && enabled
    })
  })

  it('opens the first 50 only on demand and appends opaque-cursor pages with ID dedupe', async () => {
    const first = record('first')
    const second = record('second')
    mocks.list
      .mockResolvedValueOnce(page([first], 'revision-2', 'opaque:cursor:do-not-parse', 2))
      .mockResolvedValueOnce(page([first, second], 'revision-2', undefined, 2))
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')

    await store.open()
    expect(mocks.list).toHaveBeenNthCalledWith(1, {
      accountId: 'primary', filter: 'all', cursor: undefined, limit: 50,
    })

    await store.loadMore()
    expect(mocks.list).toHaveBeenNthCalledWith(2, {
      accountId: 'primary', filter: 'all', cursor: 'opaque:cursor:do-not-parse', limit: 50,
    })
    expect(store.items.map((item) => item.id)).toEqual(['first', 'second'])
    expect(store.nextCursor).toBeUndefined()
  })

  it('retains page and exact cursor after load-more failure and retries the same cursor', async () => {
    const first = record('first-page')
    mocks.list
      .mockResolvedValueOnce(page([first], 'revision-2', 'opaque-retry', 1))
      .mockRejectedValueOnce(new Error('next page failed'))
      .mockResolvedValueOnce(page([record('second-page')], 'revision-2'))
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    await store.open()

    await store.loadMore()
    expect(store.items).toEqual([first])
    expect(store.nextCursor).toBe('opaque-retry')
    expect(store.pageError).toBe('next page failed')

    await store.loadMore()
    expect(mocks.list.mock.calls.slice(1).map(([request]) => request.cursor)).toEqual([
      'opaque-retry', 'opaque-retry',
    ])
    expect(store.items.map((item) => item.id)).toEqual(['first-page', 'second-page'])
  })

  it.each(['account', 'filter', 'gap'] as const)(
    'releases stale load-more ownership after a %s invalidation so new pagination can start',
    async (invalidation) => {
      const oldMore = deferred<NotificationPage>()
      const newMore = deferred<NotificationPage>()
      mocks.list.mockResolvedValueOnce(page(
        [record(`initial-${invalidation}`)],
        'revision-1',
        `old-cursor-${invalidation}`,
        2,
      ))
      const store = useNotificationStore()
      await store.start()
      await store.setAccount('primary')
      await store.open()
      mocks.list.mockReturnValueOnce(oldMore.promise)
      const staleLoading = store.loadMore()
      expect(store.loadingMore).toBe(true)

      mocks.list.mockResolvedValueOnce(page(
        [record(`new-first-${invalidation}`)],
        invalidation === 'gap' ? 'revision-5' : 'revision-2',
        `new-cursor-${invalidation}`,
        1,
      ))
      if (invalidation === 'account') await store.setAccount('backup')
      if (invalidation === 'filter') await store.setFilter('unread')
      if (invalidation === 'gap') {
        emit(changed('missing', 'revision-5', [{ type: 'account', accountId: 'backup' }]))
        await tick()
      }

      expect(store.loadingMore).toBe(false)
      mocks.list.mockReturnValueOnce(newMore.promise)
      const currentLoading = store.loadMore()
      expect(store.loadingMore).toBe(true)
      expect(mocks.list).toHaveBeenLastCalledWith({
        accountId: invalidation === 'account' ? 'backup' : 'primary',
        filter: invalidation === 'filter' ? 'unread' : 'all',
        cursor: `new-cursor-${invalidation}`,
        limit: 50,
      })

      oldMore.resolve(page([record(`stale-more-${invalidation}`)], 'stale-revision'))
      await staleLoading
      expect(store.loadingMore).toBe(true)
      newMore.resolve(page([record(`new-more-${invalidation}`)], store.observedRevision!))
      await currentLoading

      expect(store.loadingMore).toBe(false)
      expect(store.items.map((item) => item.id)).toEqual([
        `new-first-${invalidation}`,
        `new-more-${invalidation}`,
      ])
    },
  )

  it('hard-resets filter/context, ignores late page success/error, and keeps unread unknown on failure', async () => {
    const store = useNotificationStore()
    mocks.list.mockResolvedValueOnce(page([record('primary')], 'revision-2', 'old-cursor', 3))
    await store.start()
    await store.setAccount('primary')
    await store.open()

    const staleFilter = deferred<NotificationPage>()
    const currentFilter = deferred<NotificationPage>()
    mocks.list.mockReturnValueOnce(staleFilter.promise).mockReturnValueOnce(currentFilter.promise)
    const unreadLoad = store.setFilter('unread')
    expect(store.items).toEqual([])
    mocks.summary.mockRejectedValueOnce(new Error('backup summary failed'))
    const accountLoad = store.setAccount('backup')
    expect(store.accountId).toBe('backup')
    expect(store.filter).toBe('unread')
    expect(store.items).toEqual([])
    expect(store.nextCursor).toBeUndefined()
    expect(store.unreadCount).toBeNull()

    staleFilter.resolve(page([record('stale-filter')], 'revision-3', undefined, 8))
    currentFilter.reject(new Error('backup failed'))
    await Promise.all([unreadLoad, accountLoad])

    expect(store.items).toEqual([])
    expect(store.unreadCount).toBeNull()
    expect(store.error).toBe('backup failed')
  })

  it('keeps page one authoritative when an older same-context summary resolves later', async () => {
    const store = useNotificationStore()
    mocks.list.mockResolvedValueOnce(page([record('primary-before-switch')], 'revision-1'))
    await store.start()
    await store.setAccount('primary')
    await store.open()
    const olderSummary = deferred<{ unreadCount: number; revision: string }>()
    const newerPage = deferred<NotificationPage>()
    mocks.summary.mockReturnValueOnce(olderSummary.promise)
    mocks.list.mockReturnValueOnce(newerPage.promise)

    const switching = store.setAccount('backup')
    const backup = record('backup-newer', { type: 'account', accountId: 'backup' })
    newerPage.resolve(page([backup], 'revision-page-newer', undefined, 7))
    await tick()
    expect(store.items).toEqual([backup])
    expect(store.unreadCount).toBe(7)
    expect(store.observedRevision).toBe('revision-page-newer')

    olderSummary.resolve({ unreadCount: 99, revision: 'revision-summary-older' })
    await switching

    expect(store.items).toEqual([backup])
    expect(store.unreadCount).toBe(7)
    expect(store.observedRevision).toBe('revision-page-newer')
  })

  it('preserves a failed account page retry after its concurrent summary succeeds', async () => {
    const store = useNotificationStore()
    mocks.list.mockResolvedValueOnce(page([record('primary-before-failure')], 'revision-1'))
    await store.start()
    await store.setAccount('primary')
    await store.open()
    const backupSummary = deferred<{ unreadCount: number; revision: string }>()
    mocks.summary.mockReturnValueOnce(backupSummary.promise)
    mocks.list.mockRejectedValueOnce(new Error('backup page failed'))

    const switching = store.setAccount('backup')
    await tick()
    expect(store.error).toBe('backup page failed')

    backupSummary.resolve({ unreadCount: 4, revision: 'revision-summary' })
    await switching
    expect(store.error).toBe('backup page failed')

    const backup = record('backup-after-retry', { type: 'account', accountId: 'backup' })
    mocks.list.mockResolvedValueOnce(page([backup], 'revision-page', undefined, 4))
    await store.open()

    expect(mocks.list).toHaveBeenCalledTimes(3)
    expect(store.items).toEqual([backup])
    expect(store.unreadCount).toBe(4)
    expect(store.observedRevision).toBe('revision-page')
    expect(store.error).toBeNull()
  })

  it('keeps an opened page authoritative when the older startup summary resolves later', async () => {
    const olderSummary = deferred<{ unreadCount: number; revision: string }>()
    const newerPage = deferred<NotificationPage>()
    mocks.summary.mockReturnValueOnce(olderSummary.promise)
    mocks.list.mockReturnValueOnce(newerPage.promise)
    const store = useNotificationStore()

    const starting = store.start()
    await tick()
    const opening = store.open()
    const current = record('startup-page-newer', { type: 'global' })
    newerPage.resolve(page([current], 'revision-page-newer', undefined, 7))
    await opening
    expect(store.items).toEqual([current])
    expect(store.unreadCount).toBe(7)
    expect(store.observedRevision).toBe('revision-page-newer')

    olderSummary.resolve({ unreadCount: 99, revision: 'revision-summary-older' })
    await starting

    expect(store.items).toEqual([current])
    expect(store.unreadCount).toBe(7)
    expect(store.observedRevision).toBe('revision-page-newer')
  })

  it('retains a successful cache on continuous relevant soft refresh failure', async () => {
    const cached = record('cached')
    mocks.list.mockResolvedValueOnce(page([cached], 'revision-1', 'cursor', 1))
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    await store.open()
    mocks.list.mockRejectedValueOnce(new Error('soft refresh failed'))

    emit(changed('revision-1', 'revision-2', [{ type: 'global' }]))
    await tick()

    expect(store.observedRevision).toBe('revision-2')
    expect(store.items).toEqual([cached])
    expect(store.nextCursor).toBe('cursor')
    expect(store.error).toBe('soft refresh failed')
  })

  it('advances continuous unrelated revisions without list, unread, or Toast mutation', async () => {
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    vi.clearAllMocks()
    const toast = candidate('unrelated', { type: 'account', accountId: 'backup' })

    emit(changed('revision-1', 'revision-2', [{ type: 'account', accountId: 'backup' }], {
      change: 'created', notificationId: toast.id, toastCandidate: toast,
    }))
    await tick()

    expect(store.observedRevision).toBe('revision-2')
    expect(mocks.list).not.toHaveBeenCalled()
    expect(mocks.summary).not.toHaveBeenCalled()
    expect(mocks.showToast).not.toHaveBeenCalled()
  })

  it('hard-clears and reloads on a gap or Reset even when the visible scope looks unrelated', async () => {
    const cached = record('cached-before-gap')
    mocks.list.mockResolvedValueOnce(page([cached], 'revision-1', 'cursor', 1))
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    await store.open()
    const gapReload = deferred<NotificationPage>()
    mocks.list.mockReturnValueOnce(gapReload.promise)

    emit(changed('missing-revision', 'revision-5', [{ type: 'account', accountId: 'backup' }]))
    expect(store.items).toEqual([])
    expect(store.nextCursor).toBeUndefined()
    gapReload.resolve(page([record('after-gap')], 'revision-5'))
    await tick()
    expect(store.items.map((item) => item.id)).toEqual(['after-gap'])

    const resetReload = deferred<NotificationPage>()
    mocks.list.mockReturnValueOnce(resetReload.promise)
    emit(changed('revision-5', 'revision-6', [{ type: 'account', accountId: 'backup' }], {
      change: 'reset',
    }))
    expect(store.items).toEqual([])
    resetReload.resolve(page([record('after-reset')], 'revision-6'))
    await tick()
    expect(store.items.map((item) => item.id)).toEqual(['after-reset'])
  })

  it('allows a captured-context page response without rewinding a later unrelated revision', async () => {
    const pending = deferred<NotificationPage>()
    mocks.list.mockReturnValueOnce(pending.promise)
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    const opening = store.open()

    emit(changed('revision-1', 'revision-2', [{ type: 'account', accountId: 'backup' }]))
    pending.resolve(page([record('captured-primary')], 'revision-1', undefined, 4))
    await opening

    expect(store.items.map((item) => item.id)).toEqual(['captured-primary'])
    expect(store.unreadCount).toBe(4)
    expect(store.observedRevision).toBe('revision-2')
  })

  it('ignores a relevant event refresh completion after its captured account/generation changes', async () => {
    const primary = record('primary-before-event')
    mocks.list.mockResolvedValueOnce(page([primary], 'revision-1', undefined, 1))
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    await store.open()
    const eventReload = deferred<NotificationPage>()
    mocks.list.mockReturnValueOnce(eventReload.promise)

    emit(changed('revision-1', 'revision-2', [{ type: 'account', accountId: 'primary' }]))
    await tick()
    mocks.summary.mockResolvedValueOnce({ unreadCount: 1, revision: 'revision-3' })
    mocks.list.mockResolvedValueOnce(page([record('backup-current')], 'revision-3', undefined, 1))
    await store.setAccount('backup')
    eventReload.resolve(page([record('primary-stale-event')], 'revision-2', undefined, 9))
    await tick()

    expect(store.accountId).toBe('backup')
    expect(store.items.map((item) => item.id)).toEqual(['backup-current'])
    expect(store.unreadCount).toBe(1)
    expect(store.observedRevision).toBe('revision-3')
  })

  it('Toasts current Account and Global candidates once, using callback-captured context', async () => {
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    const accountToast = candidate('account-once')
    const globalToast = candidate('global-once', { type: 'global' })

    emit(changed('revision-1', 'revision-2', [{ type: 'account', accountId: 'primary' }], {
      change: 'created', notificationId: accountToast.id, toastCandidate: accountToast,
    }))
    emit(changed('revision-2', 'revision-3', [{ type: 'global' }], {
      change: 'created', notificationId: globalToast.id, toastCandidate: globalToast,
    }))
    emit(changed('revision-3', 'revision-4', [{ type: 'global' }], {
      change: 'created', notificationId: globalToast.id, toastCandidate: globalToast,
    }))
    await tick()

    expect(mocks.showToast.mock.calls.map(([value]) => value.id)).toEqual([
      'account-once', 'global-once',
    ])
    expect(mocks.shouldToast).toHaveBeenCalledWith(accountToast, enabledSettings, 'primary')
  })

  it('marks candidates seen before all gates so later readiness, scope, or toggle changes cannot replay', async () => {
    const settingsLoad = deferred<NotificationSettings>()
    mocks.getSettings.mockReturnValue(settingsLoad.promise)
    const store = useNotificationStore()
    const starting = store.start()
    await tick()
    const beforeSettings = candidate('before-settings', { type: 'global' })
    emit(changed('revision-1', 'revision-2', [{ type: 'global' }], {
      change: 'created', notificationId: beforeSettings.id, toastCandidate: beforeSettings,
    }))
    settingsLoad.resolve({ ...enabledSettings, riskAccountToast: false })
    await starting

    const beforeAccount = candidate('before-account', { type: 'global' })
    emit(changed('revision-2', 'revision-3', [{ type: 'global' }], {
      change: 'created', notificationId: beforeAccount.id, toastCandidate: beforeAccount,
    }))
    await store.setAccount('primary')
    const unrelated = candidate('before-scope', { type: 'account', accountId: 'backup' })
    const disabled = candidate('before-toggle', { type: 'account', accountId: 'primary' }, 'riskAccount')
    emit(changed('revision-3', 'revision-4', [{ type: 'account', accountId: 'backup' }], {
      change: 'created', notificationId: unrelated.id, toastCandidate: unrelated,
    }))
    emit(changed('revision-4', 'revision-5', [{ type: 'account', accountId: 'primary' }], {
      change: 'created', notificationId: disabled.id, toastCandidate: disabled,
    }))
    await store.setAccount('backup')
    await store.updateSettings(enabledSettings)

    for (const [prior, next, value] of [
      ['revision-5', 'revision-6', beforeSettings],
      ['revision-6', 'revision-7', beforeAccount],
      ['revision-7', 'revision-8', unrelated],
      ['revision-8', 'revision-9', disabled],
    ] as const) {
      emit(changed(prior, next, [value.scope], {
        change: 'created', notificationId: value.id, toastCandidate: value,
      }))
    }
    await tick()

    expect(mocks.showToast).not.toHaveBeenCalled()
  })

  it('treats a response-first same-revision event as already applied without a second reload', async () => {
    const item = record('response-first')
    mocks.list.mockResolvedValueOnce(page([item], 'revision-1'))
    mocks.markRead.mockResolvedValue({
      notification: { ...item, readAtMs: item.createdAtMs + 1 },
      unreadCount: 0,
      revision: 'revision-2',
    })
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    await store.open()
    vi.clearAllMocks()

    await expect(store.markRead(item.id)).resolves.toBe(true)
    emit(changed('revision-1', 'revision-2', [{ type: 'account', accountId: 'primary' }]))
    await tick()

    expect(store.observedRevision).toBe('revision-2')
    expect(mocks.list).not.toHaveBeenCalled()
    expect(mocks.summary).not.toHaveBeenCalled()
  })
})

describe('notification authoritative mutations and settings', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.resetAllMocks()
    mocks.listener = undefined
    mocks.listen.mockImplementation(async (handler: (event: NotificationChangedEvent) => void) => {
      mocks.listener = handler
      return vi.fn()
    })
    mocks.summary.mockResolvedValue({ unreadCount: 1, revision: 'revision-1' })
    mocks.getSettings.mockResolvedValue(enabledSettings)
    mocks.shouldToast.mockImplementation((toastCandidate, settings, activeAccountId) => {
      const visible = toastCandidate.scope.type === 'global'
        || (activeAccountId !== null && toastCandidate.scope.accountId === activeAccountId)
      const enabled = toastCandidate.category === 'trading'
        ? settings.tradingToast
        : toastCandidate.category === 'riskAccount'
          ? settings.riskAccountToast
          : settings.connectionSystemToast
      return visible && enabled
    })
    mocks.updateSettings.mockImplementation(async (settings: NotificationSettings) => settings)
  })

  async function openedStore(item = record()): Promise<ReturnType<typeof useNotificationStore>> {
    mocks.list.mockResolvedValueOnce(page([item], 'revision-1', undefined, 1))
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    await store.open()
    return store
  }

  it('preserves accepted mark-read authority against an older same-context page and matching event', async () => {
    const item = record('mark-read-before-old-page')
    const store = await openedStore(item)
    const olderPage = deferred<NotificationPage>()
    const repairPage = deferred<NotificationPage>()
    mocks.list
      .mockReturnValueOnce(olderPage.promise)
      .mockReturnValueOnce(repairPage.promise)
    const reloading = store.reload()
    await tick()
    const marked = { ...item, readAtMs: item.createdAtMs + 1 }
    mocks.markRead.mockResolvedValueOnce({
      notification: marked,
      unreadCount: 0,
      revision: 'revision-2',
    })

    await expect(store.markRead(item.id)).resolves.toBe(true)
    const repairOwnsLoadingAfterMutation = store.initialLoading
    emit(changed('revision-1', 'revision-2', [{ type: 'account', accountId: 'primary' }]))
    olderPage.resolve(page([item], 'revision-1', undefined, 1))
    await reloading
    repairPage.resolve(page([marked], 'revision-2', undefined, 0))
    await tick()

    expect(repairOwnsLoadingAfterMutation).toBe(true)
    expect(mocks.list).toHaveBeenCalledTimes(3)
    expect(store.items).toEqual([marked])
    expect(store.unreadCount).toBe(0)
    expect(store.observedRevision).toBe('revision-2')
    expect(store.initialLoading).toBe(false)
    expect(store.error).toBeNull()
  })

  it('preserves accepted remove authority against an older same-context page and matching event', async () => {
    const item = record('remove-before-old-page')
    const store = await openedStore(item)
    const olderPage = deferred<NotificationPage>()
    const repairPage = deferred<NotificationPage>()
    mocks.list
      .mockReturnValueOnce(olderPage.promise)
      .mockReturnValueOnce(repairPage.promise)
    const reloading = store.reload()
    await tick()
    mocks.remove.mockResolvedValueOnce({ unreadCount: 0, revision: 'revision-2' })

    await expect(store.remove(item.id)).resolves.toBe(true)
    const repairOwnsLoadingAfterMutation = store.initialLoading
    emit(changed('revision-1', 'revision-2', [{ type: 'account', accountId: 'primary' }]))
    olderPage.resolve(page([item], 'revision-1', undefined, 1))
    await reloading
    repairPage.resolve(page([], 'revision-2', undefined, 0))
    await tick()

    expect(repairOwnsLoadingAfterMutation).toBe(true)
    expect(mocks.list).toHaveBeenCalledTimes(3)
    expect(store.items).toEqual([])
    expect(store.unreadCount).toBe(0)
    expect(store.observedRevision).toBe('revision-2')
    expect(store.initialLoading).toBe(false)
    expect(store.error).toBeNull()
  })

  it('repairs an event refresh canceled by an accepted mark-read response without merging its old page', async () => {
    const itemA = record('mark-read-repair-a')
    const itemB = record('mark-read-repair-b')
    const store = await openedStore(itemA)
    const olderEventPage = deferred<NotificationPage>()
    const repairPage = deferred<NotificationPage>()
    mocks.list
      .mockReturnValueOnce(olderEventPage.promise)
      .mockReturnValueOnce(repairPage.promise)
    const toastB = candidate(itemB.id, itemB.scope)

    emit(changed('revision-1', 'revision-2', [itemB.scope], {
      change: 'created',
      notificationId: itemB.id,
      toastCandidate: toastB,
    }))
    await tick()
    const markedA = { ...itemA, readAtMs: itemA.createdAtMs + 1 }
    mocks.markRead.mockResolvedValueOnce({
      notification: markedA,
      unreadCount: 1,
      revision: 'revision-3',
    })

    await expect(store.markRead(itemA.id)).resolves.toBe(true)
    emit(changed('revision-2', 'revision-3', [itemA.scope]))
    await tick()
    olderEventPage.resolve(page([itemA, itemB], 'revision-2', undefined, 2))
    await tick()

    expect(store.items).toEqual([markedA])
    expect(store.unreadCount).toBe(1)
    expect(store.observedRevision).toBe('revision-3')

    repairPage.resolve(page([markedA, itemB], 'revision-3', undefined, 1))
    await tick()

    expect(mocks.list).toHaveBeenCalledTimes(3)
    expect(store.items).toEqual([markedA, itemB])
    expect(store.unreadCount).toBe(1)
    expect(store.observedRevision).toBe('revision-3')
    expect(store.error).toBeNull()
  })

  it('repairs an event refresh canceled by an accepted remove response without merging its old page', async () => {
    const itemA = record('remove-repair-a')
    const itemB = record('remove-repair-b')
    const store = await openedStore(itemA)
    const olderEventPage = deferred<NotificationPage>()
    const repairPage = deferred<NotificationPage>()
    mocks.list
      .mockReturnValueOnce(olderEventPage.promise)
      .mockReturnValueOnce(repairPage.promise)
    const toastB = candidate(itemB.id, itemB.scope)

    emit(changed('revision-1', 'revision-2', [itemB.scope], {
      change: 'created',
      notificationId: itemB.id,
      toastCandidate: toastB,
    }))
    await tick()
    mocks.remove.mockResolvedValueOnce({ unreadCount: 1, revision: 'revision-3' })

    await expect(store.remove(itemA.id)).resolves.toBe(true)
    emit(changed('revision-2', 'revision-3', [itemA.scope]))
    await tick()
    olderEventPage.resolve(page([itemA, itemB], 'revision-2', undefined, 2))
    await tick()

    expect(store.items).toEqual([])
    expect(store.unreadCount).toBe(1)
    expect(store.observedRevision).toBe('revision-3')

    repairPage.resolve(page([itemB], 'revision-3', undefined, 1))
    await tick()

    expect(mocks.list).toHaveBeenCalledTimes(3)
    expect(store.items).toEqual([itemB])
    expect(store.unreadCount).toBe(1)
    expect(store.observedRevision).toBe('revision-3')
    expect(store.error).toBeNull()
  })

  it('repairs a failed soft event page after an accepted mark-read without hiding its retry error', async () => {
    const itemA = record('mark-read-dirty-a')
    const itemB = record('mark-read-dirty-b')
    const store = await openedStore(itemA)
    mocks.list.mockRejectedValueOnce(new Error('soft mark-read refresh failed'))
    const toastB = candidate(itemB.id, itemB.scope)

    emit(changed('revision-1', 'revision-2', [itemB.scope], {
      change: 'created',
      notificationId: itemB.id,
      toastCandidate: toastB,
    }))
    await tick()
    expect(store.items).toEqual([itemA])
    expect(store.error).toBe('soft mark-read refresh failed')

    const repairPage = deferred<NotificationPage>()
    mocks.list.mockReturnValueOnce(repairPage.promise)
    const markedA = { ...itemA, readAtMs: itemA.createdAtMs + 1 }
    mocks.markRead.mockResolvedValueOnce({
      notification: markedA,
      unreadCount: 1,
      revision: 'revision-3',
    })

    await expect(store.markRead(itemA.id)).resolves.toBe(true)
    const errorWhileRepairPending = store.error
    emit(changed('revision-2', 'revision-3', [itemA.scope]))
    await tick()

    expect(errorWhileRepairPending).toBe('soft mark-read refresh failed')
    expect(mocks.list).toHaveBeenCalledTimes(3)
    expect(store.items).toEqual([markedA])
    expect(store.observedRevision).toBe('revision-3')

    repairPage.resolve(page([markedA, itemB], 'revision-3', undefined, 1))
    await tick()

    expect(store.items).toEqual([markedA, itemB])
    expect(store.unreadCount).toBe(1)
    expect(store.observedRevision).toBe('revision-3')
    expect(store.error).toBeNull()
  })

  it('repairs a failed soft event page after an accepted remove without hiding its retry error', async () => {
    const itemA = record('remove-dirty-a')
    const itemB = record('remove-dirty-b')
    const store = await openedStore(itemA)
    mocks.list.mockRejectedValueOnce(new Error('soft remove refresh failed'))
    const toastB = candidate(itemB.id, itemB.scope)

    emit(changed('revision-1', 'revision-2', [itemB.scope], {
      change: 'created',
      notificationId: itemB.id,
      toastCandidate: toastB,
    }))
    await tick()
    expect(store.items).toEqual([itemA])
    expect(store.error).toBe('soft remove refresh failed')

    const repairPage = deferred<NotificationPage>()
    mocks.list.mockReturnValueOnce(repairPage.promise)
    mocks.remove.mockResolvedValueOnce({ unreadCount: 1, revision: 'revision-3' })

    await expect(store.remove(itemA.id)).resolves.toBe(true)
    const errorWhileRepairPending = store.error
    emit(changed('revision-2', 'revision-3', [itemA.scope]))
    await tick()

    expect(errorWhileRepairPending).toBe('soft remove refresh failed')
    expect(mocks.list).toHaveBeenCalledTimes(3)
    expect(store.items).toEqual([])
    expect(store.observedRevision).toBe('revision-3')

    repairPage.resolve(page([itemB], 'revision-3', undefined, 1))
    await tick()

    expect(store.items).toEqual([itemB])
    expect(store.unreadCount).toBe(1)
    expect(store.observedRevision).toBe('revision-3')
    expect(store.error).toBeNull()
  })

  it('keeps a failed soft page error visible through deferred mark-all and its repair', async () => {
    const itemA = record('mark-all-dirty-a')
    const itemB = record('mark-all-dirty-b')
    const store = await openedStore(itemA)
    mocks.list.mockRejectedValueOnce(new Error('soft mark-all refresh failed'))

    emit(changed('revision-1', 'revision-2', [itemB.scope], {
      change: 'created',
      notificationId: itemB.id,
      toastCandidate: candidate(itemB.id, itemB.scope),
    }))
    await tick()
    expect(store.error).toBe('soft mark-all refresh failed')

    const response = deferred<{
      affectedCount: number; affectedScopes: NotificationRecord['scope'][]
      unreadCount: number; revision: string
    }>()
    const repairPage = deferred<NotificationPage>()
    mocks.markAllRead.mockReturnValueOnce(response.promise)
    mocks.list.mockReturnValueOnce(repairPage.promise)
    const marking = store.markAllRead()

    expect(store.markAllReadPending).toBe(true)
    expect(store.error).toBe('soft mark-all refresh failed')

    const readA = { ...itemA, readAtMs: itemA.createdAtMs + 1 }
    const readB = { ...itemB, readAtMs: itemB.createdAtMs + 1 }
    response.resolve({
      affectedCount: 2,
      affectedScopes: [{ type: 'account', accountId: 'primary' }],
      unreadCount: 0,
      revision: 'revision-3',
    })
    await tick()

    expect(store.markAllReadPending).toBe(true)
    expect(store.error).toBe('soft mark-all refresh failed')

    repairPage.resolve(page([readA, readB], 'revision-3', undefined, 0))
    await expect(marking).resolves.toBe(true)

    expect(store.markAllReadPending).toBe(false)
    expect(store.items).toEqual([readA, readB])
    expect(store.error).toBeNull()
  })

  it('keeps a failed soft page error visible through deferred account clear and its reload', async () => {
    const itemA = record('clear-dirty-a')
    const itemB = record('clear-dirty-b')
    const store = await openedStore(itemA)
    mocks.list.mockRejectedValueOnce(new Error('soft clear refresh failed'))

    emit(changed('revision-1', 'revision-2', [itemB.scope], {
      change: 'created',
      notificationId: itemB.id,
      toastCandidate: candidate(itemB.id, itemB.scope),
    }))
    await tick()
    expect(store.error).toBe('soft clear refresh failed')

    const response = deferred<{ affectedCount: number; unreadCount: number; revision: string }>()
    const reloadPage = deferred<NotificationPage>()
    mocks.clear.mockReturnValueOnce(response.promise)
    mocks.list.mockReturnValueOnce(reloadPage.promise)
    const clearing = store.clearCurrentAccount()

    expect(store.clearCurrentAccountPending).toBe(true)
    expect(store.error).toBe('soft clear refresh failed')

    const global = record('clear-dirty-global', { type: 'global' })
    response.resolve({ affectedCount: 2, unreadCount: 1, revision: 'revision-3' })
    await tick()

    expect(store.clearCurrentAccountPending).toBe(true)
    expect(store.items).toEqual([])
    expect(store.error).toBe('soft clear refresh failed')

    reloadPage.resolve(page([global], 'revision-3', undefined, 1))
    await expect(clearing).resolves.toBe(true)

    expect(store.clearCurrentAccountPending).toBe(false)
    expect(store.items).toEqual([global])
    expect(store.error).toBeNull()
  })

  it('does not query after an ordinary remove without active or failed first-page work', async () => {
    const item = record('remove-without-page-repair')
    const store = await openedStore(item)
    mocks.list.mockClear()
    mocks.remove.mockResolvedValueOnce({ unreadCount: 0, revision: 'revision-2' })

    await expect(store.remove(item.id)).resolves.toBe(true)
    emit(changed('revision-1', 'revision-2', [item.scope]))
    await tick()

    expect(mocks.list).not.toHaveBeenCalled()
    expect(store.items).toEqual([])
    expect(store.unreadCount).toBe(0)
    expect(store.observedRevision).toBe('revision-2')
    expect(store.error).toBeNull()
  })

  it('preserves accepted mark-read authority against an older same-context summary', async () => {
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    const olderSummary = deferred<{ unreadCount: number; revision: string }>()
    mocks.summary.mockReturnValueOnce(olderSummary.promise)
    emit(changed('revision-1', 'revision-event', [{ type: 'account', accountId: 'primary' }]))
    await tick()
    const item = record('mark-read-before-old-summary')
    const marked = { ...item, readAtMs: item.createdAtMs + 1 }
    mocks.markRead.mockResolvedValueOnce({
      notification: marked,
      unreadCount: 0,
      revision: 'revision-mutation',
    })

    await expect(store.markRead(item.id)).resolves.toBe(true)
    emit(changed(
      'revision-event',
      'revision-mutation',
      [{ type: 'account', accountId: 'primary' }],
    ))
    olderSummary.resolve({ unreadCount: 99, revision: 'revision-event' })
    await tick()

    expect(store.unreadCount).toBe(0)
    expect(store.observedRevision).toBe('revision-mutation')
    expect(store.error).toBeNull()
  })

  it('preserves accepted remove authority against an older same-context summary', async () => {
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    const olderSummary = deferred<{ unreadCount: number; revision: string }>()
    mocks.summary.mockReturnValueOnce(olderSummary.promise)
    emit(changed('revision-1', 'revision-event', [{ type: 'account', accountId: 'primary' }]))
    await tick()
    const item = record('remove-before-old-summary')
    mocks.remove.mockResolvedValueOnce({
      unreadCount: 0,
      revision: 'revision-mutation',
    })

    await expect(store.remove(item.id)).resolves.toBe(true)
    emit(changed(
      'revision-event',
      'revision-mutation',
      [{ type: 'account', accountId: 'primary' }],
    ))
    olderSummary.resolve({ unreadCount: 99, revision: 'revision-event' })
    await tick()

    expect(store.unreadCount).toBe(0)
    expect(store.observedRevision).toBe('revision-mutation')
    expect(store.error).toBeNull()
  })

  it('preserves accepted mark-read authority against an older same-context load-more', async () => {
    const item = record('mark-read-before-old-more')
    mocks.list.mockResolvedValueOnce(page([item], 'revision-1', 'cursor-1', 1))
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    await store.open()
    const olderMore = deferred<NotificationPage>()
    mocks.list.mockReturnValueOnce(olderMore.promise)
    const loading = store.loadMore()
    await tick()
    const marked = { ...item, readAtMs: item.createdAtMs + 1 }
    mocks.markRead.mockResolvedValueOnce({
      notification: marked,
      unreadCount: 0,
      revision: 'revision-2',
    })

    await expect(store.markRead(item.id)).resolves.toBe(true)
    const loadingReleasedBeforeOldMore = !store.loadingMore
    emit(changed('revision-1', 'revision-2', [{ type: 'account', accountId: 'primary' }]))
    olderMore.resolve(page(
      [item, record('stale-mark-read-more')],
      'revision-1',
      'cursor-stale',
      2,
    ))
    await loading

    expect(loadingReleasedBeforeOldMore).toBe(true)
    expect(store.items).toEqual([marked])
    expect(store.nextCursor).toBe('cursor-1')
    expect(store.unreadCount).toBe(0)
    expect(store.observedRevision).toBe('revision-2')
    expect(store.pageError).toBeNull()
  })

  it('preserves accepted remove authority against an older same-context load-more', async () => {
    const item = record('remove-before-old-more')
    mocks.list.mockResolvedValueOnce(page([item], 'revision-1', 'cursor-1', 1))
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    await store.open()
    const olderMore = deferred<NotificationPage>()
    mocks.list.mockReturnValueOnce(olderMore.promise)
    const loading = store.loadMore()
    await tick()
    mocks.remove.mockResolvedValueOnce({ unreadCount: 0, revision: 'revision-2' })

    await expect(store.remove(item.id)).resolves.toBe(true)
    const loadingReleasedBeforeOldMore = !store.loadingMore
    emit(changed('revision-1', 'revision-2', [{ type: 'account', accountId: 'primary' }]))
    olderMore.resolve(page(
      [item, record('stale-remove-more')],
      'revision-1',
      'cursor-stale',
      2,
    ))
    await loading

    expect(loadingReleasedBeforeOldMore).toBe(true)
    expect(store.items).toEqual([])
    expect(store.nextCursor).toBe('cursor-1')
    expect(store.unreadCount).toBe(0)
    expect(store.observedRevision).toBe('revision-2')
    expect(store.pageError).toBeNull()
  })

  it('releases older read loading and error ownership when mutation authority is accepted', async () => {
    const item = record('mutation-before-old-errors')
    mocks.list.mockResolvedValueOnce(page([item], 'revision-1', 'cursor-1', 1))
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    await store.open()
    const olderPage = deferred<NotificationPage>()
    const repairPage = deferred<NotificationPage>()
    mocks.list
      .mockReturnValueOnce(olderPage.promise)
      .mockReturnValueOnce(repairPage.promise)
    const reloading = store.reload()
    await tick()
    const marked = { ...item, readAtMs: item.createdAtMs + 1 }
    mocks.markRead.mockResolvedValueOnce({
      notification: marked,
      unreadCount: 0,
      revision: 'revision-2',
    })
    await store.markRead(item.id)
    olderPage.reject(new Error('stale page failed'))
    await reloading
    repairPage.resolve(page([marked], 'revision-2', 'cursor-1', 0))
    await tick()
    const pageLoadingReleased = !store.initialLoading

    const olderMore = deferred<NotificationPage>()
    mocks.list.mockReturnValueOnce(olderMore.promise)
    const loading = store.loadMore()
    await tick()
    mocks.remove.mockResolvedValueOnce({ unreadCount: 0, revision: 'revision-3' })
    await store.remove(item.id)
    const moreLoadingReleased = !store.loadingMore
    olderMore.reject(new Error('stale load-more failed'))
    await loading

    expect(pageLoadingReleased).toBe(true)
    expect(moreLoadingReleased).toBe(true)
    expect(store.error).toBeNull()
    expect(store.pageError).toBeNull()
    expect(store.observedRevision).toBe('revision-3')
  })

  it('keeps mark-read immutable before response and applies the returned record only on success', async () => {
    const item = record('mark-read')
    const response = deferred<{
      notification: NotificationRecord; unreadCount: number; revision: string
    }>()
    mocks.markRead.mockReturnValue(response.promise)
    const store = await openedStore(item)

    const marking = store.markRead(item.id)
    expect(store.items[0]).toEqual(item)
    expect(store.unreadCount).toBe(1)
    expect(store.observedRevision).toBe('revision-1')
    response.resolve({
      notification: { ...item, readAtMs: item.createdAtMs + 1 },
      unreadCount: 0,
      revision: 'opaque-read-revision',
    })

    await expect(marking).resolves.toBe(true)
    expect(store.items[0].readAtMs).toBe(item.createdAtMs + 1)
    expect(store.unreadCount).toBe(0)
    expect(store.observedRevision).toBe('opaque-read-revision')
    expect(mocks.showToast).not.toHaveBeenCalled()
  })

  it('removes a successfully read item from the unread filter', async () => {
    const item = record('read-unread-filter')
    mocks.list
      .mockResolvedValueOnce(page([item], 'revision-1', undefined, 1))
      .mockResolvedValueOnce(page([item], 'revision-1', undefined, 1))
    mocks.markRead.mockResolvedValue({
      notification: { ...item, readAtMs: item.createdAtMs + 1 },
      unreadCount: 0,
      revision: 'revision-2',
    })
    const store = await openedStore(item)
    await store.setFilter('unread')

    await store.markRead(item.id)

    expect(store.items).toEqual([])
  })

  it('retains authoritative fields and returns false with a decoded local error on mutation failure', async () => {
    const item = record('failed-read')
    mocks.markRead.mockRejectedValue({ message: 'read failed', notificationId: 'owned' })
    const store = await openedStore(item)
    const before = {
      items: [...store.items], unreadCount: store.unreadCount, revision: store.observedRevision,
    }

    await expect(store.markRead(item.id)).resolves.toBe(false)

    expect(store.items).toEqual(before.items)
    expect(store.unreadCount).toBe(before.unreadCount)
    expect(store.observedRevision).toBe(before.revision)
    expect(store.error).toBe('read failed')
    expect(mocks.reportError).not.toHaveBeenCalled()
    expect(mocks.showBackendError).not.toHaveBeenCalled()
    expect(mocks.showToast).not.toHaveBeenCalled()
  })

  it('does not remove an item before the authoritative delete response', async () => {
    const item = record('remove-success')
    const response = deferred<{ unreadCount: number; revision: string }>()
    mocks.remove.mockReturnValue(response.promise)
    const store = await openedStore(item)

    const removing = store.remove(item.id)
    expect(store.items).toEqual([item])
    expect(store.unreadCount).toBe(1)
    response.resolve({ unreadCount: 0, revision: 'revision-2' })
    await expect(removing).resolves.toBe(true)

    expect(store.items).toEqual([])
    expect(store.unreadCount).toBe(0)
    expect(store.observedRevision).toBe('revision-2')
  })

  it('does not cancel an in-flight mutation during an ordinary same-context reload', async () => {
    const item = record('mutation-survives-reload')
    const mutation = deferred<{
      notification: NotificationRecord; unreadCount: number; revision: string
    }>()
    mocks.markRead.mockReturnValue(mutation.promise)
    const store = await openedStore(item)
    const marking = store.markRead(item.id)
    expect(store.markReadPendingIds.has(item.id)).toBe(true)
    mocks.list.mockResolvedValueOnce(page([item], 'revision-1', undefined, 1))

    await store.reload()

    expect(store.markReadPendingIds.has(item.id)).toBe(true)
    const readItem = { ...item, readAtMs: item.createdAtMs + 1 }
    mutation.resolve({ notification: readItem, unreadCount: 0, revision: 'revision-2' })
    await expect(marking).resolves.toBe(true)
    expect(store.items).toEqual([readItem])
    expect(store.unreadCount).toBe(0)
  })

  it.each(['remove', 'markAllRead', 'clearCurrentAccount'] as const)(
    'retains every authoritative field when %s fails',
    async (action) => {
      const item = record(`failure-${action}`)
      const store = await openedStore(item)
      const before = {
        items: [...store.items],
        unreadCount: store.unreadCount,
        revision: store.observedRevision,
      }
      if (action === 'remove') mocks.remove.mockRejectedValueOnce(new Error('remove failed'))
      if (action === 'markAllRead') mocks.markAllRead.mockRejectedValueOnce(new Error('mark all failed'))
      if (action === 'clearCurrentAccount') mocks.clear.mockRejectedValueOnce(new Error('clear failed'))

      const ok = action === 'remove'
        ? await store.remove(item.id)
        : action === 'markAllRead'
          ? await store.markAllRead()
          : await store.clearCurrentAccount()

      expect(ok).toBe(false)
      expect(store.items).toEqual(before.items)
      expect(store.unreadCount).toBe(before.unreadCount)
      expect(store.observedRevision).toBe(before.revision)
      expect(store.error).toContain('failed')
      expect(mocks.reportError).not.toHaveBeenCalled()
      expect(mocks.showBackendError).not.toHaveBeenCalled()
      expect(mocks.showToast).not.toHaveBeenCalled()
    },
  )

  it('removes only after delete success and ignores a late response after an event wins', async () => {
    const item = record('remove-race')
    const removal = deferred<{ unreadCount: number; revision: string }>()
    mocks.remove.mockReturnValue(removal.promise)
    const store = await openedStore(item)
    const deleting = store.remove(item.id)
    expect(store.items).toEqual([item])
    mocks.list.mockResolvedValueOnce(page([], 'revision-2', undefined, 0))

    emit(changed('revision-1', 'revision-2', [{ type: 'account', accountId: 'primary' }], {
      change: 'removed', notificationId: item.id,
    }))
    removal.resolve({ unreadCount: 99, revision: 'stale-command-revision' })
    await expect(deleting).resolves.toBe(false)
    await tick()

    expect(store.items).toEqual([])
    expect(store.unreadCount).toBe(0)
    expect(store.observedRevision).toBe('revision-2')
  })

  it('does not let an old lifecycle per-ID mutation finally clear the restarted owner', async () => {
    const item = record('same-id-after-restart')
    const oldResponse = deferred<{
      notification: NotificationRecord; unreadCount: number; revision: string
    }>()
    const newResponse = deferred<{
      notification: NotificationRecord; unreadCount: number; revision: string
    }>()
    mocks.markRead
      .mockReturnValueOnce(oldResponse.promise)
      .mockReturnValueOnce(newResponse.promise)
    const store = await openedStore(item)
    const oldMutation = store.markRead(item.id)
    store.stop()
    await store.start()
    await store.setAccount('primary')
    mocks.list.mockResolvedValueOnce(page([item], 'revision-1', undefined, 1))
    await store.open()

    const newMutation = store.markRead(item.id)
    expect(store.markReadPendingIds.has(item.id)).toBe(true)
    oldResponse.resolve({ notification: item, unreadCount: 9, revision: 'stale' })
    await oldMutation
    expect(store.markReadPendingIds.has(item.id)).toBe(true)

    const readItem = { ...item, readAtMs: item.createdAtMs + 1 }
    newResponse.resolve({ notification: readItem, unreadCount: 0, revision: 'revision-2' })
    await newMutation
    expect(store.markReadPendingIds.has(item.id)).toBe(false)
    expect(store.items).toEqual([readItem])
  })

  it.each(['account', 'filter', 'gap'] as const)(
    'releases per-ID pending ownership on %s invalidation without old-finally ABA',
    async (invalidation) => {
      const item = record(`pending-${invalidation}`)
      const oldResponse = deferred<{
        notification: NotificationRecord; unreadCount: number; revision: string
      }>()
      const newResponse = deferred<{
        notification: NotificationRecord; unreadCount: number; revision: string
      }>()
      mocks.markRead
        .mockReturnValueOnce(oldResponse.promise)
        .mockReturnValueOnce(newResponse.promise)
      const store = await openedStore(item)
      const oldMutation = store.markRead(item.id)
      expect(store.markReadPendingIds.has(item.id)).toBe(true)

      mocks.list.mockResolvedValueOnce(page([item], 'revision-2', undefined, 1))
      if (invalidation === 'account') {
        mocks.summary.mockResolvedValueOnce({ unreadCount: 1, revision: 'revision-2' })
        await store.setAccount('backup')
      }
      if (invalidation === 'filter') await store.setFilter('unread')
      if (invalidation === 'gap') {
        emit(changed('missing', 'revision-2', [{ type: 'account', accountId: 'backup' }]))
        await tick()
      }
      expect(store.markReadPendingIds.has(item.id)).toBe(false)

      const newMutation = store.markRead(item.id)
      expect(store.markReadPendingIds.has(item.id)).toBe(true)
      oldResponse.resolve({ notification: item, unreadCount: 9, revision: 'stale' })
      await oldMutation
      expect(store.markReadPendingIds.has(item.id)).toBe(true)

      const readItem = { ...item, readAtMs: item.createdAtMs + 1 }
      newResponse.resolve({ notification: readItem, unreadCount: 0, revision: 'revision-3' })
      await newMutation
      expect(store.markReadPendingIds.has(item.id)).toBe(false)
    },
  )

  it('releases mark-all and clear owners on account invalidation without old-finally ABA', async () => {
    const item = record('aggregate-pending-account')
    const oldMarkAll = deferred<{
      affectedCount: number; affectedScopes: NotificationRecord['scope'][]
      unreadCount: number; revision: string
    }>()
    const newMarkAll = deferred<{
      affectedCount: number; affectedScopes: NotificationRecord['scope'][]
      unreadCount: number; revision: string
    }>()
    const oldClear = deferred<{ affectedCount: number; unreadCount: number; revision: string }>()
    const newClear = deferred<{ affectedCount: number; unreadCount: number; revision: string }>()
    mocks.markAllRead
      .mockReturnValueOnce(oldMarkAll.promise)
      .mockReturnValueOnce(newMarkAll.promise)
    mocks.clear
      .mockReturnValueOnce(oldClear.promise)
      .mockReturnValueOnce(newClear.promise)
    const store = await openedStore(item)
    const oldMarking = store.markAllRead()
    const oldClearing = store.clearCurrentAccount()
    expect(store.markAllReadPending).toBe(true)
    expect(store.clearCurrentAccountPending).toBe(true)

    mocks.summary.mockResolvedValueOnce({ unreadCount: 1, revision: 'revision-2' })
    mocks.list.mockResolvedValueOnce(page([item], 'revision-2', undefined, 1))
    await store.setAccount('backup')
    expect(store.markAllReadPending).toBe(false)
    expect(store.clearCurrentAccountPending).toBe(false)

    const newMarking = store.markAllRead()
    const newClearing = store.clearCurrentAccount()
    expect(store.markAllReadPending).toBe(true)
    expect(store.clearCurrentAccountPending).toBe(true)
    oldMarkAll.resolve({
      affectedCount: 1,
      affectedScopes: [{ type: 'account', accountId: 'primary' }],
      unreadCount: 0,
      revision: 'stale-mark-all',
    })
    oldClear.resolve({ affectedCount: 1, unreadCount: 0, revision: 'stale-clear' })
    await Promise.all([oldMarking, oldClearing])
    expect(store.markAllReadPending).toBe(true)
    expect(store.clearCurrentAccountPending).toBe(true)

    mocks.list.mockResolvedValue(page([], 'revision-3', undefined, 0))
    newMarkAll.resolve({
      affectedCount: 1,
      affectedScopes: [{ type: 'account', accountId: 'backup' }],
      unreadCount: 0,
      revision: 'revision-3',
    })
    newClear.resolve({ affectedCount: 1, unreadCount: 0, revision: 'revision-3' })
    await Promise.all([newMarking, newClearing])
    expect(store.markAllReadPending).toBe(false)
    expect(store.clearCurrentAccountPending).toBe(false)
  })

  it('does not let an old lifecycle mark-all finally clear the restarted owner', async () => {
    const item = record('mark-all-after-restart')
    const oldResponse = deferred<{
      affectedCount: number; affectedScopes: NotificationRecord['scope'][]
      unreadCount: number; revision: string
    }>()
    const newResponse = deferred<{
      affectedCount: number; affectedScopes: NotificationRecord['scope'][]
      unreadCount: number; revision: string
    }>()
    mocks.markAllRead
      .mockReturnValueOnce(oldResponse.promise)
      .mockReturnValueOnce(newResponse.promise)
    const store = await openedStore(item)
    const oldMutation = store.markAllRead()
    store.stop()
    await store.start()
    await store.setAccount('primary')
    mocks.list.mockResolvedValueOnce(page([item], 'revision-1', undefined, 1))
    await store.open()

    const newMutation = store.markAllRead()
    expect(store.markAllReadPending).toBe(true)
    oldResponse.resolve({
      affectedCount: 1,
      affectedScopes: [{ type: 'account', accountId: 'primary' }],
      unreadCount: 0,
      revision: 'stale',
    })
    await oldMutation
    expect(store.markAllReadPending).toBe(true)

    const readItem = { ...item, readAtMs: item.createdAtMs + 1 }
    mocks.list.mockResolvedValueOnce(page([readItem], 'revision-2', undefined, 0))
    newResponse.resolve({
      affectedCount: 1,
      affectedScopes: [{ type: 'account', accountId: 'primary' }],
      unreadCount: 0,
      revision: 'revision-2',
    })
    await newMutation
    expect(store.markAllReadPending).toBe(false)
    expect(store.items).toEqual([readItem])
  })

  it('does not let an old lifecycle clear finally clear the restarted owner', async () => {
    const item = record('clear-after-restart')
    const oldResponse = deferred<{ affectedCount: number; unreadCount: number; revision: string }>()
    const newResponse = deferred<{ affectedCount: number; unreadCount: number; revision: string }>()
    mocks.clear
      .mockReturnValueOnce(oldResponse.promise)
      .mockReturnValueOnce(newResponse.promise)
    const store = await openedStore(item)
    const oldMutation = store.clearCurrentAccount()
    store.stop()
    await store.start()
    await store.setAccount('primary')
    mocks.list.mockResolvedValueOnce(page([item], 'revision-1', undefined, 1))
    await store.open()

    const newMutation = store.clearCurrentAccount()
    expect(store.clearCurrentAccountPending).toBe(true)
    oldResponse.resolve({ affectedCount: 1, unreadCount: 9, revision: 'stale' })
    await oldMutation
    expect(store.clearCurrentAccountPending).toBe(true)

    const global = record('global-after-restart', { type: 'global' })
    mocks.list.mockResolvedValueOnce(page([global], 'revision-2', undefined, 1))
    newResponse.resolve({ affectedCount: 1, unreadCount: 1, revision: 'revision-2' })
    await newMutation
    expect(store.clearCurrentAccountPending).toBe(false)
    expect(store.items).toEqual([global])
  })

  it('adopts mark-all authority then soft reloads exact timestamps', async () => {
    const item = record('mark-all')
    const readItem = { ...item, readAtMs: item.createdAtMs + 5 }
    mocks.markAllRead.mockResolvedValue({
      affectedCount: 1,
      affectedScopes: [{ type: 'account', accountId: 'primary' }],
      unreadCount: 0,
      revision: 'revision-2',
    })
    const store = await openedStore(item)
    mocks.list.mockResolvedValueOnce(page([readItem], 'revision-2', undefined, 0))

    await expect(store.markAllRead()).resolves.toBe(true)

    expect(mocks.markAllRead).toHaveBeenCalledWith('primary')
    expect(store.items).toEqual([readItem])
    expect(store.unreadCount).toBe(0)
    expect(store.observedRevision).toBe('revision-2')
  })

  it.each(['markAllRead', 'clearCurrentAccount'] as const)(
    'does not cancel an unrelated per-item mutation during %s authoritative reload',
    async (aggregateAction) => {
      const item = record(`parallel-${aggregateAction}`)
      const removal = deferred<{ unreadCount: number; revision: string }>()
      mocks.remove.mockReturnValue(removal.promise)
      const store = await openedStore(item)
      const removing = store.remove(item.id)
      expect(store.removePendingIds.has(item.id)).toBe(true)

      if (aggregateAction === 'markAllRead') {
        mocks.markAllRead.mockResolvedValueOnce({
          affectedCount: 1,
          affectedScopes: [{ type: 'account', accountId: 'primary' }],
          unreadCount: 0,
          revision: 'revision-2',
        })
      } else {
        mocks.clear.mockResolvedValueOnce({
          affectedCount: 1,
          unreadCount: 0,
          revision: 'revision-2',
        })
      }
      mocks.list.mockResolvedValueOnce(page([item], 'revision-2', undefined, 0))
      const aggregateOk = aggregateAction === 'markAllRead'
        ? await store.markAllRead()
        : await store.clearCurrentAccount()
      expect(aggregateOk).toBe(true)
      expect(store.removePendingIds.has(item.id)).toBe(true)

      removal.resolve({ unreadCount: 0, revision: 'revision-3' })
      await expect(removing).resolves.toBe(true)
      expect(store.removePendingIds.has(item.id)).toBe(false)
      expect(store.items).toEqual([])
    },
  )

  it('keeps mark-all state immutable until its response arrives', async () => {
    const item = record('mark-all-deferred')
    const response = deferred<{
      affectedCount: number; affectedScopes: NotificationRecord['scope'][]
      unreadCount: number; revision: string
    }>()
    mocks.markAllRead.mockReturnValue(response.promise)
    const store = await openedStore(item)
    const marking = store.markAllRead()

    expect(store.items).toEqual([item])
    expect(store.unreadCount).toBe(1)
    expect(store.observedRevision).toBe('revision-1')
    const readItem = { ...item, readAtMs: item.createdAtMs + 1 }
    mocks.list.mockResolvedValueOnce(page([readItem], 'revision-2', undefined, 0))
    response.resolve({
      affectedCount: 1,
      affectedScopes: [{ type: 'account', accountId: 'primary' }],
      unreadCount: 0,
      revision: 'revision-2',
    })
    await marking

    expect(store.items).toEqual([readItem])
    expect(store.unreadCount).toBe(0)
  })

  it('rejects Global-only clear locally and hard reloads surviving Global records after success', async () => {
    const store = useNotificationStore()
    mocks.list.mockResolvedValue(page([], 'revision-1'))
    await store.start()
    await expect(store.clearCurrentAccount()).resolves.toBe(false)
    expect(mocks.clear).not.toHaveBeenCalled()
    expect(store.error).not.toBeNull()

    await store.setAccount('primary')
    await store.open()
    const global = record('global-survivor', { type: 'global' })
    mocks.clear.mockResolvedValue({ affectedCount: 3, unreadCount: 1, revision: 'revision-2' })
    mocks.list.mockResolvedValueOnce(page([global], 'revision-2', undefined, 1))

    await expect(store.clearCurrentAccount()).resolves.toBe(true)

    expect(mocks.clear).toHaveBeenCalledWith('primary')
    expect(store.items).toEqual([global])
    expect(store.observedRevision).toBe('revision-2')
  })

  it('keeps clear state immutable until its response then hard reloads Global survivors', async () => {
    const item = record('clear-deferred')
    const response = deferred<{ affectedCount: number; unreadCount: number; revision: string }>()
    mocks.clear.mockReturnValue(response.promise)
    const store = await openedStore(item)
    const clearing = store.clearCurrentAccount()

    expect(store.items).toEqual([item])
    expect(store.unreadCount).toBe(1)
    expect(store.observedRevision).toBe('revision-1')
    const global = record('clear-deferred-global', { type: 'global' })
    mocks.list.mockResolvedValueOnce(page([global], 'revision-2', undefined, 1))
    response.resolve({ affectedCount: 1, unreadCount: 1, revision: 'revision-2' })
    await clearing

    expect(store.items).toEqual([global])
    expect(store.unreadCount).toBe(1)
    expect(store.observedRevision).toBe('revision-2')
  })

  it('serializes settings saves, coalesces intermediate drafts, and commits only Rust responses', async () => {
    const firstSave = deferred<NotificationSettings>()
    const secondSave = deferred<NotificationSettings>()
    mocks.updateSettings
      .mockReturnValueOnce(firstSave.promise)
      .mockReturnValueOnce(secondSave.promise)
    const store = useNotificationStore()
    await store.start()
    const first = { ...enabledSettings, tradingToast: false }
    const intermediate = { ...first, riskAccountToast: false }
    const latest = { ...intermediate, connectionSystemToast: false }

    const savingFirst = store.updateSettings(first)
    const savingIntermediate = store.updateSettings(intermediate)
    const savingLatest = store.updateSettings(latest)
    expect(store.settingsDraft).toEqual(latest)
    expect(store.settingsCommitted).toEqual(enabledSettings)
    expect(mocks.updateSettings).toHaveBeenCalledTimes(1)
    firstSave.resolve({ ...first })
    await tick()
    expect(mocks.updateSettings).toHaveBeenCalledTimes(2)
    expect(mocks.updateSettings).toHaveBeenNthCalledWith(2, latest)
    expect(store.settingsCommitted).toEqual(first)
    secondSave.resolve({ ...latest })
    await Promise.all([savingFirst, savingIntermediate, savingLatest])

    expect(store.settingsCommitted).toEqual(latest)
    expect(store.settingsDraft).toEqual(latest)
    expect(store.settingsSaveStatus).toBe('saved')
  })

  it('retains failed draft and prior commit, then retries through the same action', async () => {
    const store = useNotificationStore()
    await store.start()
    const draft = { ...enabledSettings, tradingToast: false }
    mocks.updateSettings.mockRejectedValueOnce(new Error('save failed'))

    await store.updateSettings(draft)

    expect(store.settingsDraft).toEqual(draft)
    expect(store.settingsCommitted).toEqual(enabledSettings)
    expect(store.settingsSaveStatus).toBe('error')
    expect(store.settingsError).toBe('save failed')

    const committed = { ...draft, riskAccountToast: false }
    mocks.updateSettings.mockResolvedValueOnce(committed)
    await store.updateSettings(store.settingsDraft!)

    expect(mocks.updateSettings).toHaveBeenLastCalledWith(draft)
    expect(store.settingsCommitted).toEqual(committed)
    expect(store.settingsSaveStatus).toBe('saved')
  })

  it('stops a failed drain on the newest retained draft and retries that exact snapshot', async () => {
    const failing = deferred<NotificationSettings>()
    mocks.updateSettings.mockReturnValueOnce(failing.promise)
    const store = useNotificationStore()
    await store.start()
    const first = { ...enabledSettings, tradingToast: false }
    const latest = { ...first, riskAccountToast: false }
    const firstCall = store.updateSettings(first)
    const latestCall = store.updateSettings(latest)
    failing.reject(new Error('first snapshot failed'))
    await Promise.all([firstCall, latestCall])

    expect(mocks.updateSettings).toHaveBeenCalledTimes(1)
    expect(store.settingsDraft).toEqual(latest)
    expect(store.settingsCommitted).toEqual(enabledSettings)
    expect(store.settingsSaveStatus).toBe('error')

    mocks.updateSettings.mockResolvedValueOnce(latest)
    await store.updateSettings(store.settingsDraft!)
    expect(mocks.updateSettings).toHaveBeenLastCalledWith(latest)
    expect(store.settingsCommitted).toEqual(latest)
  })

  it('uses committed settings for realtime Toast while a newer draft is pending or failed', async () => {
    const initiallyDisabled = { ...enabledSettings, tradingToast: false }
    mocks.getSettings.mockResolvedValue(initiallyDisabled)
    const save = deferred<NotificationSettings>()
    mocks.updateSettings.mockReturnValue(save.promise)
    const store = useNotificationStore()
    await store.start()
    await store.setAccount('primary')
    const pendingDraft = { ...initiallyDisabled, tradingToast: true }
    const saving = store.updateSettings(pendingDraft)
    const realtime = candidate('committed-only')

    emit(changed('revision-1', 'revision-2', [realtime.scope], {
      change: 'created', notificationId: realtime.id, toastCandidate: realtime,
    }))
    expect(mocks.shouldToast).toHaveBeenCalledWith(realtime, initiallyDisabled, 'primary')
    save.reject(new Error('save failed'))
    await saving

    expect(store.settingsCommitted).toEqual(initiallyDisabled)
    expect(mocks.showToast).not.toHaveBeenCalled()
  })

  it('does not let a late initial settings load overwrite a newer save owner', async () => {
    const initial = deferred<NotificationSettings>()
    mocks.getSettings.mockReturnValue(initial.promise)
    const store = useNotificationStore()
    const starting = store.start()
    await tick()
    const draft = { ...enabledSettings, tradingToast: false }
    mocks.updateSettings.mockResolvedValue(draft)

    await store.updateSettings(draft)
    initial.resolve(enabledSettings)
    await starting

    expect(store.settingsDraft).toEqual(draft)
    expect(store.settingsCommitted).toEqual(draft)
  })

  it('ignores a load started during save when the save resolves first', async () => {
    const store = useNotificationStore()
    await store.start()
    const save = deferred<NotificationSettings>()
    const load = deferred<NotificationSettings>()
    mocks.updateSettings.mockReturnValueOnce(save.promise)
    mocks.getSettings.mockReturnValueOnce(load.promise)
    const draft = { ...enabledSettings, tradingToast: false }
    const staleLoaded = { ...enabledSettings, riskAccountToast: false }

    const saving = store.updateSettings(draft)
    const loading = store.loadSettings()
    const statusWhileSaveOwnsAuthority = store.settingsSaveStatus
    save.resolve(draft)
    await saving
    load.resolve(staleLoaded)
    await loading

    expect(statusWhileSaveOwnsAuthority).toBe('saving')
    expect(store.settingsDraft).toEqual(draft)
    expect(store.settingsCommitted).toEqual(draft)
    expect(store.settingsSaveStatus).toBe('saved')
    expect(store.settingsError).toBeNull()
  })

  it('ignores a load started during save when the load resolves first', async () => {
    const store = useNotificationStore()
    await store.start()
    const save = deferred<NotificationSettings>()
    const load = deferred<NotificationSettings>()
    mocks.updateSettings.mockReturnValueOnce(save.promise)
    mocks.getSettings.mockReturnValueOnce(load.promise)
    const draft = { ...enabledSettings, tradingToast: false }
    const staleLoaded = { ...enabledSettings, riskAccountToast: false }

    const saving = store.updateSettings(draft)
    const loading = store.loadSettings()
    load.resolve(staleLoaded)
    await loading

    expect(store.settingsDraft).toEqual(draft)
    expect(store.settingsCommitted).toEqual(enabledSettings)
    expect(store.settingsSaveStatus).toBe('saving')

    save.resolve(draft)
    await saving
    expect(store.settingsDraft).toEqual(draft)
    expect(store.settingsCommitted).toEqual(draft)
    expect(store.settingsSaveStatus).toBe('saved')
    expect(store.settingsError).toBeNull()
  })

  it('does not let an old settings drain own a save started after stop and restart', async () => {
    const oldSave = deferred<NotificationSettings>()
    const newDraft = { ...enabledSettings, connectionSystemToast: false }
    mocks.updateSettings
      .mockReturnValueOnce(oldSave.promise)
      .mockResolvedValueOnce(newDraft)
    const store = useNotificationStore()
    await store.start()
    const oldDrain = store.updateSettings({ ...enabledSettings, tradingToast: false })
    store.stop()
    await store.start()

    const newDrain = store.updateSettings(newDraft)
    expect(mocks.updateSettings).toHaveBeenCalledTimes(2)
    oldSave.resolve(enabledSettings)
    await Promise.all([oldDrain, newDrain])

    expect(store.settingsCommitted).toEqual(newDraft)
  })
})

describe('App notification lifecycle ownership', () => {
  const appConfig = {
    activeSymbol: 'BTCUSDT', activeAccountId: 'primary', watchlistSymbols: ['BTCUSDT'],
    theme: 'dark', klineInterval: '15', useWebsocket: false,
    wsPublicUrl: '', wsPrivateUrl: '', tickerPollInterval: 1000,
    windowWidth: 1200, windowHeight: 800, accounts: ['primary', 'backup'],
    riskEnabled: true, riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5',
    riskMaxDailyOrders: 100, tradingDayTimezone: 'Asia/Shanghai',
  } as const

  beforeEach(() => {
    setActivePinia(createPinia())
    vi.resetAllMocks()
    mocks.listener = undefined
    mocks.whenListenersReady.mockResolvedValue(undefined)
    mocks.listen.mockImplementation(async (handler: (event: NotificationChangedEvent) => void) => {
      mocks.listener = handler
      return vi.fn()
    })
    mocks.summary.mockResolvedValue({ unreadCount: 0, revision: 'revision-app' })
    mocks.getSettings.mockResolvedValue(enabledSettings)
    mocks.list.mockResolvedValue(page([], 'revision-app'))
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_version') return Promise.resolve('test-version')
      if (command === 'get_config') return Promise.resolve({ ...appConfig })
      if (command === 'list_account_profiles') {
        return Promise.resolve([{
          accountId: 'primary', label: 'Primary', baseUrl: 'https://trade.example',
          credentialState: 'missing', active: true,
        }])
      }
      if (command === 'fetch_instruments') return Promise.resolve([])
      if (command === 'get_environment_status') return Promise.resolve({})
      return Promise.resolve(undefined)
    })
  })

  it('waits for existing listeners, starts notifications Global-only, then adopts verified profile and watches later authority', async () => {
    const order: string[] = []
    mocks.whenListenersReady.mockImplementation(async () => { order.push('listeners-ready') })
    mocks.listen.mockImplementation(async (handler: (event: NotificationChangedEvent) => void) => {
      order.push('notification-listen')
      mocks.listener = handler
      return vi.fn()
    })
    mocks.summary.mockImplementation(async (accountId: string | null) => {
      order.push(`notification-summary:${String(accountId)}`)
      return { unreadCount: 0, revision: 'revision-app' }
    })
    mocks.invoke.mockImplementation((command: string) => {
      order.push(command)
      if (command === 'get_version') return Promise.resolve('test-version')
      if (command === 'get_config') return Promise.resolve({ ...appConfig })
      if (command === 'list_account_profiles') {
        return Promise.resolve([{
          accountId: 'primary', label: 'Primary', baseUrl: 'https://trade.example',
          credentialState: 'missing', active: true,
        }])
      }
      if (command === 'fetch_instruments') return Promise.resolve([])
      if (command === 'get_environment_status') return Promise.resolve({})
      return Promise.resolve(undefined)
    })

    const wrapper = shallowMount(App)
    await flushPromises()
    await flushPromises()
    const store = useNotificationStore()

    expect(order.indexOf('listeners-ready')).toBeLessThan(order.indexOf('notification-listen'))
    expect(order.indexOf('notification-listen')).toBeLessThan(order.indexOf('get_version'))
    expect(mocks.summary.mock.calls[0]).toEqual([null])
    expect(store.accountId).toBe('primary')

    useConfigStore().adoptActiveAccountId('backup')
    await flushPromises()
    expect(store.accountId).toBe('backup')
    expect(mocks.summary).toHaveBeenLastCalledWith('backup')
    wrapper.unmount()
  })

  it('does not forward the fabricated pre-config default while config/profile validation is pending', async () => {
    const config = deferred<typeof appConfig>()
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_version') return Promise.resolve('test-version')
      if (command === 'get_config') return config.promise
      return Promise.resolve(undefined)
    })
    const wrapper = shallowMount(App)
    await flushPromises()
    const store = useNotificationStore()

    expect(store.accountId).toBeNull()
    expect(mocks.summary).toHaveBeenCalledWith(null)
    expect(mocks.summary).not.toHaveBeenCalledWith('default')
    wrapper.unmount()
    config.resolve(appConfig)
    await flushPromises()
  })

  it('stops once on unmount and prevents late listener readiness from continuing startup', async () => {
    const ready = deferred<void>()
    mocks.whenListenersReady.mockReturnValue(ready.promise)
    const wrapper = shallowMount(App)
    const store = useNotificationStore()
    const stop = vi.spyOn(store, 'stop')
    const start = vi.spyOn(store, 'start')

    wrapper.unmount()
    ready.resolve()
    await flushPromises()

    expect(stop).toHaveBeenCalledOnce()
    expect(start).not.toHaveBeenCalled()
    expect(mocks.invoke).not.toHaveBeenCalledWith('get_config')
  })
})
