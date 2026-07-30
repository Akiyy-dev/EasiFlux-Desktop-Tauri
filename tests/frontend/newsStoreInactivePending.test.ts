import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import * as newsService from '../../src/services/newsService'
import { useNewsStore } from '../../src/stores/news'
import type { NewsMessageDto, NewsStatusKind, NewsStatusSnapshot } from '../../src/types/news'

vi.mock('../../src/services/newsService')

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((ok, fail) => { resolve = ok; reject = fail })
  return { promise, resolve, reject }
}

const item = (deliveryId: string): NewsMessageDto => ({
  deliveryId, createdAt: '2026-07-30T12:00:00Z', text: deliveryId,
})

function status(overrides: Partial<NewsStatusSnapshot> = {}): NewsStatusSnapshot {
  return {
    kind: 'live', initialSyncComplete: true, syncedCount: 1,
    latestDeliveryId: '10', unreadCount: 0, ...overrides,
  }
}

function committed(insertedCount: number, newestDeliveryId: string, unreadCount: number) {
  return { insertedCount, newestDeliveryId, unreadCount, initialSyncComplete: true }
}

function prepareAway(store: ReturnType<typeof useNewsStore>, pending: number): void {
  store.messages = [item('10')]
  store.pendingNewCount = pending
  store.isPageActive = true
  store.isAtLatest = false
  store.leavePage()
}

describe('news store exact inactive pending recovery', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.resetAllMocks()
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status())
    vi.mocked(newsService.listNewsMessages).mockResolvedValue({
      items: [item('10')], hasMore: true, latestDeliveryId: '10', unreadCount: 0,
    })
  })

  it('adds exact inactive inserts when unread is capped from 99 to 100', async () => {
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status({ unreadCount: 99 }))
    const store = useNewsStore()
    await store.initialize()
    prepareAway(store, 99)

    await store.handleMessagesCommitted(committed(5, '15', 100))
    await store.enterPage()

    expect(store.pendingNewCount).toBe(104)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10'])
    expect(newsService.listNewsMessages).not.toHaveBeenCalled()
  })

  it('counts inserts after a 100 baseline across inactive status and event interleaving', async () => {
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status({ unreadCount: 100 }))
    const store = useNewsStore()
    await store.initialize()
    prepareAway(store, 100)

    await store.handleStatusChanged(status({
      kind: 'retrying' as NewsStatusKind, unreadCount: 100, message: 'retrying',
    }))
    await store.handleMessagesCommitted(committed(4, '14', 100))
    await store.handleStatusChanged(status({ latestDeliveryId: '14', unreadCount: 100 }))
    await store.handleMessagesCommitted(committed(3, '17', 100))
    await store.enterPage()

    expect(store.pendingNewCount).toBe(107)
    expect(store.status?.latestDeliveryId).toBe('17')
  })

  it('merges one inactive batch only once across repeated away re-entry', async () => {
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status({ unreadCount: 99 }))
    const store = useNewsStore()
    await store.initialize()
    prepareAway(store, 99)
    await store.handleMessagesCommitted(committed(5, '15', 100))

    await store.enterPage()
    expect(store.pendingNewCount).toBe(104)
    store.leavePage()
    await store.enterPage()

    expect(store.pendingNewCount).toBe(104)
  })

  it('excludes the initial-sync completion batch but counts later live inserts exactly', async () => {
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status({
      kind: 'initialSync', initialSyncComplete: false,
      latestDeliveryId: undefined, unreadCount: 0,
    }))
    const store = useNewsStore()
    await store.initialize()
    prepareAway(store, 0)

    await store.handleMessagesCommitted(committed(50, '50', 0))
    await store.handleMessagesCommitted(committed(5, '55', 100))
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status({
      latestDeliveryId: '55', unreadCount: 100,
    }))
    await store.enterPage()

    expect(store.pendingNewCount).toBe(5)
  })

  it('clears an inactive batch after top refresh and counts only the next away batch', async () => {
    const store = useNewsStore()
    await store.initialize()
    store.messages = [item('10')]
    store.isPageActive = true
    store.isAtLatest = true
    store.leavePage()
    await store.handleMessagesCommitted(committed(5, '15', 100))
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status({
      latestDeliveryId: '15', unreadCount: 100,
    }))
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce({
      items: [item('15')], hasMore: true, latestDeliveryId: '15', unreadCount: 100,
    })

    await store.enterPage()
    expect(store.pendingNewCount).toBe(0)
    store.isAtLatest = false
    store.leavePage()
    await store.handleMessagesCommitted(committed(2, '17', 100))
    await store.enterPage()

    expect(store.pendingNewCount).toBe(2)
  })

  it('clears away pending when top re-entry succeeds after showLatest failed', async () => {
    const store = useNewsStore()
    await store.initialize()
    store.messages = [item('10')]
    store.isPageActive = true
    store.isAtLatest = false
    await store.handleMessagesCommitted(committed(5, '15', 5))
    store.setAtLatest(true)
    vi.mocked(newsService.listNewsMessages).mockRejectedValueOnce(new Error('offline'))

    await expect(store.showLatest()).rejects.toThrow('offline')
    expect(store.pendingNewCount).toBe(5)
    store.leavePage()
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status({
      latestDeliveryId: '15', unreadCount: 5,
    }))
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce({
      items: [item('15')], hasMore: true, latestDeliveryId: '15', unreadCount: 5,
    })

    await store.enterPage()

    expect(store.messages[0]?.deliveryId).toBe('15')
    expect(store.pendingNewCount).toBe(0)
  })

  it('clears away pending after a stale showLatest leave and successful top re-entry', async () => {
    const store = useNewsStore()
    await store.initialize()
    store.messages = [item('10')]
    store.isPageActive = true
    store.isAtLatest = false
    await store.handleMessagesCommitted(committed(5, '15', 5))
    store.setAtLatest(true)
    const stale = deferred<{
      items: NewsMessageDto[]; hasMore: boolean; latestDeliveryId: string; unreadCount: number
    }>()
    vi.mocked(newsService.listNewsMessages).mockReturnValueOnce(stale.promise)

    const showing = store.showLatest()
    store.leavePage()
    stale.resolve({
      items: [item('15')], hasMore: true, latestDeliveryId: '15', unreadCount: 5,
    })
    await expect(showing).resolves.toBeUndefined()
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status({
      latestDeliveryId: '15', unreadCount: 5,
    }))
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce({
      items: [item('15')], hasMore: true, latestDeliveryId: '15', unreadCount: 5,
    })

    await store.enterPage()

    expect(store.messages[0]?.deliveryId).toBe('15')
    expect(store.pendingNewCount).toBe(0)
  })

  it('clears both pending sources when the existing top already covers metadata', async () => {
    const store = useNewsStore()
    await store.initialize()
    store.messages = [item('10')]
    store.pendingNewCount = 5
    store.isPageActive = true
    store.isAtLatest = true
    store.leavePage()

    await store.enterPage()

    expect(newsService.listNewsMessages).not.toHaveBeenCalled()
    expect(store.pendingNewCount).toBe(0)
  })

  it('retains pending for a stale refresh and clears it only after a covering successor', async () => {
    const store = useNewsStore()
    await store.initialize()
    store.messages = [item('10')]
    store.isPageActive = true
    store.isAtLatest = false
    await store.handleMessagesCommitted(committed(5, '15', 5))
    store.setAtLatest(true)
    const stale = deferred<{
      items: NewsMessageDto[]; hasMore: boolean; latestDeliveryId: string; unreadCount: number
    }>()
    vi.mocked(newsService.listNewsMessages).mockReturnValueOnce(stale.promise)
    const refreshing = store.refreshLatest()
    await store.handleStatusChanged(status({ latestDeliveryId: '17', unreadCount: 7 }))
    stale.resolve({
      items: [item('15')], hasMore: true, latestDeliveryId: '15', unreadCount: 5,
    })
    await refreshing

    expect(store.pendingNewCount).toBe(5)
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce({
      items: [item('17')], hasMore: true, latestDeliveryId: '17', unreadCount: 7,
    })
    await store.refreshLatest()

    expect(store.messages[0]?.deliveryId).toBe('17')
    expect(store.pendingNewCount).toBe(0)
  })
})
