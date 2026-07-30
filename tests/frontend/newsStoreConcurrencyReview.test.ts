import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import * as newsService from '../../src/services/newsService'
import { useNewsStore } from '../../src/stores/news'
import type { NewsMessageDto, NewsPage, NewsStatusSnapshot } from '../../src/types/news'

vi.mock('../../src/services/newsService')

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((ok, fail) => { resolve = ok; reject = fail })
  return { promise, resolve, reject }
}

const status = (overrides: Partial<NewsStatusSnapshot> = {}): NewsStatusSnapshot => ({
  kind: 'live', initialSyncComplete: true, syncedCount: 1,
  latestDeliveryId: '10', unreadCount: 1, ...overrides,
})
const item = (deliveryId: string): NewsMessageDto => ({
  deliveryId, createdAt: '2026-07-30T12:00:00Z', text: deliveryId,
})
const page = (ids: string[], overrides: Partial<NewsPage> = {}): NewsPage => ({
  items: ids.map(item), hasMore: false, latestDeliveryId: ids[0], unreadCount: ids.length, ...overrides,
})
const event = (deliveryId: string, insertedCount = 1) => ({
  insertedCount, newestDeliveryId: deliveryId, unreadCount: insertedCount,
  initialSyncComplete: true,
})
async function flush(): Promise<void> {
  for (let index = 0; index < 6; index += 1) await Promise.resolve()
}

describe('news store review concurrency cases', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.resetAllMocks()
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status())
    vi.mocked(newsService.listNewsMessages).mockResolvedValue(page(['10']))
    vi.mocked(newsService.recheckNewsCredentials).mockResolvedValue(status())
    vi.mocked(newsService.retryNewsSync).mockResolvedValue(status())
  })

  it('does not let an older seen snapshot lower unread metadata after a newer commit', async () => {
    const seen = deferred<{ latestDeliveryId?: string; unreadCount: number }>()
    vi.mocked(newsService.markNewsSeen).mockReturnValueOnce(seen.promise)
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce(page(['11'], { unreadCount: 1 }))
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = true
    store.messages = [item('10')]

    const marking = store.markLatestSeen('10')
    await store.handleMessagesCommitted(event('11'))
    seen.resolve({ latestDeliveryId: '10', unreadCount: 0 })
    await marking

    expect(store.unreadCount).toBe(1)
    expect(store.status?.latestDeliveryId).toBe('11')
    expect(store.status?.unreadCount).toBe(1)
  })

  it('preserves away-from-latest history and pending count across re-entry', async () => {
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = false
    store.messages = [item('10')]
    await store.handleMessagesCommitted(event('12', 2))
    store.leavePage()
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status({ latestDeliveryId: '12', unreadCount: 2 }))

    await store.enterPage()

    expect(newsService.listNewsMessages).not.toHaveBeenCalled()
    expect(store.pendingNewCount).toBe(2)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10'])
    expect(store.isAtLatest).toBe(false)
  })

  it('retains pending count when a re-entry replacement fails', async () => {
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = false
    await store.handleMessagesCommitted(event('12', 2))
    store.leavePage()
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status({ latestDeliveryId: '12', unreadCount: 2 }))
    vi.mocked(newsService.listNewsMessages).mockRejectedValueOnce(new Error('read failed'))

    await expect(store.enterPage()).rejects.toThrow('read failed')

    expect(store.pendingNewCount).toBe(2)
  })

  it('prevents a stale enter rejection and finally from contaminating a newer enter', async () => {
    const oldStatus = deferred<NewsStatusSnapshot>()
    const newStatus = deferred<NewsStatusSnapshot>()
    vi.mocked(newsService.getNewsStatus)
      .mockReturnValueOnce(oldStatus.promise).mockReturnValueOnce(newStatus.promise)
    const store = useNewsStore()
    const oldEnter = store.enterPage()
    store.leavePage()
    const newEnter = store.enterPage()

    oldStatus.reject(new Error('old enter failed'))
    await expect(oldEnter).rejects.toThrow('old enter failed')
    expect(store.initialError).toBeNull()
    expect(store.initialLoading).toBe(true)

    newStatus.resolve(status({ kind: 'initialSync', initialSyncComplete: false }))
    await newEnter
    expect(store.initialLoading).toBe(false)
  })

  it('prevents a stale load-more rejection/finally from mutating a new session flight', async () => {
    const oldMore = deferred<NewsPage>()
    const newMore = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(oldMore.promise)
      .mockReturnValueOnce(newMore.promise)
    const store = useNewsStore()
    store.isPageActive = true
    store.messages = [item('10'), item('9')]
    store.hasMore = true
    const oldCall = store.loadMore()
    store.leavePage()
    await store.enterPage()
    const newCall = store.loadMore()
    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(2)

    oldMore.reject(new Error('old load failed'))
    await expect(oldCall).rejects.toThrow('old load failed')
    expect(store.loadMoreError).toBeNull()
    expect(store.loadingMore).toBe(true)

    newMore.resolve(page(['18'], { hasMore: false }))
    await newCall
    expect(store.loadingMore).toBe(false)
  })

  it('prevents a stale latest finally from clearing a newer show-latest loading flag', async () => {
    const oldLatest = deferred<NewsPage>()
    const newShow = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(oldLatest.promise)
      .mockResolvedValueOnce(page(['20']))
      .mockReturnValueOnce(newShow.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    const oldCall = store.refreshLatest()
    store.leavePage()
    await store.enterPage()
    const showCall = store.showLatest()

    oldLatest.reject(new Error('old latest failed'))
    await expect(oldCall).rejects.toThrow('old latest failed')
    expect(store.refreshingLatest).toBe(true)

    newShow.resolve(page(['21']))
    await expect(showCall).resolves.toBe('21')
    expect(store.refreshingLatest).toBe(false)
  })

  it('serializes an automatic follow-up before showLatest without making show stale', async () => {
    const first = deferred<NewsPage>()
    const followUp = deferred<NewsPage>()
    const show = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(first.promise).mockReturnValueOnce(followUp.promise).mockReturnValueOnce(show.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.pendingNewCount = 4
    const refreshing = store.refreshLatest()
    const committed = store.handleMessagesCommitted(event('12'))
    const showing = store.showLatest()
    expect(newsService.listNewsMessages).toHaveBeenCalledOnce()

    first.resolve(page(['11']))
    await flush()
    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(2)
    followUp.resolve(page(['12', '11']))
    await flush()
    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(3)
    show.resolve(page(['13', '12']))

    await Promise.all([refreshing, committed])
    await expect(showing).resolves.toBe('13')
    expect(store.pendingNewCount).toBe(0)
    expect(store.refreshingLatest).toBe(false)
  })

  it('continues a queued showLatest after an automatic follow-up fails', async () => {
    const first = deferred<NewsPage>()
    const failedFollowUp = deferred<NewsPage>()
    const show = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(first.promise).mockReturnValueOnce(failedFollowUp.promise).mockReturnValueOnce(show.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.pendingNewCount = 2
    const refreshing = store.refreshLatest().catch((error: unknown) => error)
    const committed = store.handleMessagesCommitted(event('12')).catch((error: unknown) => error)
    const showing = store.showLatest()

    first.resolve(page(['11']))
    await flush()
    failedFollowUp.reject(new Error('follow-up failed'))
    await flush()
    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(3)
    show.resolve(page(['13']))

    expect(await refreshing).toBeInstanceOf(Error)
    expect(await committed).toBeInstanceOf(Error)
    await expect(showing).resolves.toBe('13')
    expect(store.pendingNewCount).toBe(0)
    expect(store.refreshingLatest).toBe(false)
  })

  it('does not append an older load-more result after showLatest replaces the view', async () => {
    const oldMore = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(oldMore.promise).mockResolvedValueOnce(page(['20', '19']))
    const store = useNewsStore()
    store.isPageActive = true
    store.messages = [item('10'), item('9')]
    store.hasMore = true

    const loading = store.loadMore()
    await expect(store.showLatest()).resolves.toBe('20')
    oldMore.resolve(page(['8']))
    await loading

    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['20', '19'])
  })

  it('does not expose an older load-more error after showLatest replaces the view', async () => {
    const oldMore = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(oldMore.promise).mockResolvedValueOnce(page(['20']))
    const store = useNewsStore()
    store.isPageActive = true
    store.messages = [item('10')]
    store.hasMore = true

    const loading = store.loadMore()
    await store.showLatest()
    oldMore.reject(new Error('obsolete more failed'))
    await expect(loading).rejects.toThrow('obsolete more failed')

    expect(store.loadMoreError).toBeNull()
    expect(store.loadingMore).toBe(false)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['20'])
  })

  it('queues an event arriving during showLatest and refreshes it afterward', async () => {
    const show = deferred<NewsPage>()
    const afterEvent = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(show.promise).mockReturnValueOnce(afterEvent.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.pendingNewCount = 2

    const showing = store.showLatest()
    const committed = store.handleMessagesCommitted(event('12'))
    expect(newsService.listNewsMessages).toHaveBeenCalledOnce()
    show.resolve(page(['11']))
    await flush()
    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(2)
    afterEvent.resolve(page(['12', '11']))

    await expect(showing).resolves.toBe('12')
    await committed
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['12', '11'])
    expect(store.pendingNewCount).toBe(0)
    expect(store.refreshingLatest).toBe(false)
  })

  it('keeps a queued event recoverable when showLatest fails', async () => {
    const show = deferred<NewsPage>()
    const afterEvent = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(show.promise).mockReturnValueOnce(afterEvent.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.pendingNewCount = 2

    const showing = store.showLatest()
    const committed = store.handleMessagesCommitted(event('12'))
    show.reject(new Error('show failed'))
    await expect(showing).rejects.toThrow('show failed')
    await flush()
    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(2)
    afterEvent.resolve(page(['12']))
    await committed

    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['12'])
    expect(store.pendingNewCount).toBe(0)
    expect(store.refreshingLatest).toBe(false)
  })
})
