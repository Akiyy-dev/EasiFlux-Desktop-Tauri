import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import * as newsService from '../../src/services/newsService'
import { useNewsStore } from '../../src/stores/news'
import type { NewsMessageDto, NewsPage, NewsStatusSnapshot, NewsUnreadSnapshot } from '../../src/types/news'

vi.mock('../../src/services/newsService')

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  const promise = new Promise<T>((ok) => { resolve = ok })
  return { promise, resolve }
}

const item = (deliveryId: string): NewsMessageDto => ({
  deliveryId, createdAt: '2026-07-30T12:00:00Z', text: deliveryId,
})
const status = (unreadCount = 5): NewsStatusSnapshot => ({
  kind: 'live', initialSyncComplete: true, syncedCount: 10,
  latestDeliveryId: '10', unreadCount,
})
const page = (ids: string[], unreadCount = 5, hasMore = false): NewsPage => ({
  items: ids.map(item), hasMore, latestDeliveryId: ids[0], unreadCount,
})

describe('news store causal unread and latest-page races', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.resetAllMocks()
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status())
    vi.mocked(newsService.listNewsMessages).mockResolvedValue(page(['10']))
    vi.mocked(newsService.markNewsSeen).mockResolvedValue({ latestDeliveryId: '10', unreadCount: 0 })
  })

  it('does not let an older equal-ID status response undo a later seen result', async () => {
    const oldStatus = deferred<NewsStatusSnapshot>()
    vi.mocked(newsService.getNewsStatus)
      .mockReturnValueOnce(oldStatus.promise)
      .mockResolvedValueOnce(status())
    const store = useNewsStore()
    const initializing = store.initialize()
    await store.enterPage()
    await store.markLatestSeen('10')

    oldStatus.resolve(status(5))
    await initializing

    expect(store.unreadCount).toBe(0)
    expect(store.status?.unreadCount).toBe(0)
  })

  it('does not let an older equal-ID page response undo a later seen result', async () => {
    const oldPage = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages).mockReturnValueOnce(oldPage.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = true
    store.messages = [item('10')]

    const showing = store.showLatest()
    await store.markLatestSeen('10')
    oldPage.resolve(page(['10'], 5))
    await showing

    expect(store.unreadCount).toBe(0)
    expect(store.status?.unreadCount).toBe(0)
  })

  it('keeps equal-ID unread monotonic across a fresh re-entry status', async () => {
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = true
    store.messages = [item('10')]
    await store.markLatestSeen('10')
    store.setAtLatest(false)
    store.leavePage()
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status(5))

    await store.enterPage()

    expect(store.unreadCount).toBe(0)
    expect(store.status?.unreadCount).toBe(0)
  })

  it('keeps equal-ID unread monotonic across a fresh latest-page result', async () => {
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = true
    store.messages = [item('10')]
    await store.markLatestSeen('10')
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce(page(['10'], 5))

    await store.showLatest()

    expect(store.unreadCount).toBe(0)
    expect(store.status?.unreadCount).toBe(0)
  })

  it('does not let an older equal-ID mark response undo a newer mark response', async () => {
    const older = deferred<NewsUnreadSnapshot>()
    const newer = deferred<NewsUnreadSnapshot>()
    vi.mocked(newsService.markNewsSeen)
      .mockReturnValueOnce(older.promise)
      .mockReturnValueOnce(newer.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = true
    store.messages = [item('10')]

    const first = store.markLatestSeen('10')
    const second = store.markLatestSeen('10')
    newer.resolve({ latestDeliveryId: '10', unreadCount: 0 })
    await second
    older.resolve({ latestDeliveryId: '10', unreadCount: 4 })
    await first

    expect(store.unreadCount).toBe(0)
    expect(store.status?.unreadCount).toBe(0)
  })

  it('keeps committed inserts pending when the user scrolls away during refresh', async () => {
    const latest = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages).mockReturnValueOnce(latest.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = true
    store.messages = [item('10')]

    const committed = store.handleMessagesCommitted({
      insertedCount: 2, newestDeliveryId: '12', unreadCount: 2, initialSyncComplete: true,
    })
    store.setAtLatest(false)
    latest.resolve(page(['12', '11', '10'], 2))
    await committed

    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10'])
    expect(store.pendingNewCount).toBe(2)
  })

  it('does not merge a disjoint latest page into terminal history', async () => {
    const newest = Array.from({ length: 50 }, (_, index) => String(110 - index))
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce(page(newest, 100, true))
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = true
    store.messages = Array.from({ length: 10 }, (_, index) => item(String(10 - index)))
    store.hasMore = false

    await store.handleMessagesCommitted({
      insertedCount: 100, newestDeliveryId: '110', unreadCount: 100, initialSyncComplete: true,
    })

    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(newest)
    expect(store.hasMore).toBe(true)
  })
})
