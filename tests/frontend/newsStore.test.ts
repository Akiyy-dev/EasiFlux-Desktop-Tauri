import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import * as newsService from '../../src/services/newsService'
import { useNewsStore } from '../../src/stores/news'
import type {
  NewsMessageDto,
  NewsMessagesCommittedEvent,
  NewsPage,
  NewsStatusSnapshot,
} from '../../src/types/news'

vi.mock('../../src/services/newsService')

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T) => void
  reject: (reason?: unknown) => void
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((ok, fail) => { resolve = ok; reject = fail })
  return { promise, resolve, reject }
}

const liveStatus = (overrides: Partial<NewsStatusSnapshot> = {}): NewsStatusSnapshot => ({
  kind: 'live', initialSyncComplete: true, syncedCount: 2,
  latestDeliveryId: '9007199254740995', unreadCount: 2, ...overrides,
})
const message = (deliveryId: string, text = deliveryId): NewsMessageDto => ({
  deliveryId, createdAt: '2026-07-30T12:00:00Z', text,
})
const page = (ids: string[], overrides: Partial<NewsPage> = {}): NewsPage => ({
  items: ids.map((id) => message(id)), hasMore: false,
  latestDeliveryId: ids[0], unreadCount: ids.length, ...overrides,
})
const committed = (
  overrides: Partial<NewsMessagesCommittedEvent> = {},
): NewsMessagesCommittedEvent => ({
  insertedCount: 1, newestDeliveryId: '9007199254740995', unreadCount: 1,
  initialSyncComplete: true, ...overrides,
})

async function flush(): Promise<void> {
  await Promise.resolve()
  await Promise.resolve()
}

describe('news store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.resetAllMocks()
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(liveStatus())
    vi.mocked(newsService.listNewsMessages).mockResolvedValue(page(['9007199254740995']))
    vi.mocked(newsService.markNewsSeen).mockResolvedValue({ latestDeliveryId: '9007199254740995', unreadCount: 0 })
    vi.mocked(newsService.recheckNewsCredentials).mockResolvedValue(liveStatus())
    vi.mocked(newsService.retryNewsSync).mockResolvedValue(liveStatus())
  })

  it('reconciles an event received before an older status snapshot without listing while closed', async () => {
    const store = useNewsStore()
    const snapshot = deferred<NewsStatusSnapshot>()
    vi.mocked(newsService.getNewsStatus).mockReturnValueOnce(snapshot.promise)

    const initializing = store.initialize()
    await store.handleMessagesCommitted(committed({
      newestDeliveryId: '9007199254740997', unreadCount: 7,
    }))
    snapshot.resolve(liveStatus({ latestDeliveryId: '9007199254740993', unreadCount: 3 }))
    await initializing

    expect(store.status?.latestDeliveryId).toBe('9007199254740997')
    expect(store.unreadCount).toBe(7)
    expect(newsService.listNewsMessages).not.toHaveBeenCalled()
  })

  it('orders and deduplicates delivery IDs above Number.MAX_SAFE_INTEGER with BigInt semantics', async () => {
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce(page([
      '9007199254740993', '9007199254740995', '9007199254740994', '9007199254740995',
    ], { latestDeliveryId: '9007199254740995' }))

    const store = useNewsStore()
    await store.enterPage()

    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual([
      '9007199254740995', '9007199254740994', '9007199254740993',
    ])
  })

  it('hides partial history and starts one latest load when initial sync completes on an active page', async () => {
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(liveStatus({
      kind: 'initialSync', initialSyncComplete: false, syncedCount: 413,
      latestDeliveryId: '413', unreadCount: 0,
    }))
    const store = useNewsStore()
    await store.enterPage()
    expect(store.messages).toEqual([])
    expect(store.status?.syncedCount).toBe(413)
    expect(newsService.listNewsMessages).not.toHaveBeenCalled()

    await store.handleStatusChanged(liveStatus({ latestDeliveryId: '500', unreadCount: 0 }))

    expect(newsService.listNewsMessages).toHaveBeenCalledOnce()
    expect(store.messages).toHaveLength(1)
  })

  it.each(['notConfigured', 'retrying', 'credentialInvalid', 'contractError', 'storageError'] as const)(
    'loads a complete cache while status is %s',
    async (kind) => {
      vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(liveStatus({ kind }))
      const store = useNewsStore()
      await store.enterPage()
      expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['9007199254740995'])
    },
  )

  it('invalidates an outstanding page generation when the page closes', async () => {
    const first = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages).mockReturnValueOnce(first.promise)
    const store = useNewsStore()
    const entering = store.enterPage()
    await flush()
    store.leavePage()
    first.resolve(page(['999']))
    await entering
    expect(store.messages).toEqual([])
  })

  it('prevents an older latest request from overwriting showLatest', async () => {
    const old = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(old.promise)
      .mockResolvedValueOnce(page(['12']))
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(liveStatus({
      latestDeliveryId: '10', unreadCount: 0,
    }))
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = true

    const refreshing = store.refreshLatest()
    const showing = store.showLatest()
    old.resolve(page(['11']))
    await refreshing
    const shown = await showing

    expect(shown).toBe('12')
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['12'])
  })

  it('coalesces event bursts into one latest refresh and exactly one follow-up', async () => {
    const first = deferred<NewsPage>()
    const second = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = true

    const eventOne = store.handleMessagesCommitted(committed({ newestDeliveryId: '11' }))
    const eventTwo = store.handleMessagesCommitted(committed({ newestDeliveryId: '12' }))
    const eventThree = store.handleMessagesCommitted(committed({ newestDeliveryId: '13' }))
    await flush()
    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(1)
    first.resolve(page(['12', '11']))
    await flush()
    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(2)
    second.resolve(page(['13', '12', '11']))
    await Promise.all([eventOne, eventTwo, eventThree])

    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(2)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['13', '12', '11'])
  })

  it('merges refreshed rows at the top without losing older history or terminal hasMore', async () => {
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = true
    store.messages = [message('10'), message('9'), message('8')]
    store.hasMore = false
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce(page(['12', '11', '10'], { hasMore: true }))

    await store.refreshLatest()

    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['12', '11', '10', '9', '8'])
    expect(store.hasMore).toBe(false)
  })

  it('keeps the list stable and accumulates actual inserted counts while away from latest', async () => {
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = false
    store.messages = [message('10')]

    await store.handleMessagesCommitted(committed({ insertedCount: 3, newestDeliveryId: '13' }))
    await store.handleMessagesCommitted(committed({ insertedCount: 2, newestDeliveryId: '15' }))

    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10'])
    expect(store.pendingNewCount).toBe(5)
    expect(newsService.listNewsMessages).not.toHaveBeenCalled()
  })

  it('showLatest replaces the page and clears pending only after success', async () => {
    const store = useNewsStore()
    store.isPageActive = true
    store.isAtLatest = false
    store.pendingNewCount = 4
    store.messages = [message('5')]
    vi.mocked(newsService.listNewsMessages)
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce(page(['9', '8']))

    await expect(store.showLatest()).rejects.toThrow('offline')
    expect(store.pendingNewCount).toBe(4)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['5'])

    await expect(store.showLatest()).resolves.toBe('9')
    expect(store.pendingNewCount).toBe(0)
    expect(store.isAtLatest).toBe(true)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['9', '8'])
  })

  it('paginates once, deduplicates, preserves data on error, and retries until the terminal page', async () => {
    const store = useNewsStore()
    store.isPageActive = true
    store.messages = [message('10'), message('9')]
    store.hasMore = true
    const pending = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(pending.promise)
      .mockRejectedValueOnce(new Error('disk busy'))
      .mockResolvedValueOnce(page(['8', '7'], { hasMore: false }))

    const one = store.loadMore()
    const duplicate = store.loadMore()
    expect(newsService.listNewsMessages).toHaveBeenCalledOnce()
    expect(newsService.listNewsMessages).toHaveBeenCalledWith('9', 50)
    pending.resolve(page(['9', '8'], { hasMore: true }))
    await Promise.all([one, duplicate])
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10', '9', '8'])

    await expect(store.loadMore()).rejects.toThrow('disk busy')
    expect(store.loadMoreError).toBe('disk busy')
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10', '9', '8'])
    await store.loadMore()
    await store.loadMore()
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10', '9', '8', '7'])
    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(3)
  })

  it('marks only the expected rendered top and does not implicitly advance to a newer arrival', async () => {
    const seen = deferred<{ latestDeliveryId?: string; unreadCount: number }>()
    vi.mocked(newsService.markNewsSeen).mockReturnValueOnce(seen.promise)
    const store = useNewsStore()
    store.isPageActive = true
    store.isAtLatest = true
    store.messages = [message('10')]

    const marking = store.markLatestSeen('10')
    store.messages.unshift(message('11'))
    await store.handleMessagesCommitted(committed({ newestDeliveryId: '11', unreadCount: 1 }))
    seen.resolve({ latestDeliveryId: '11', unreadCount: 1 })
    await marking

    expect(newsService.markNewsSeen).toHaveBeenCalledWith('10')
    expect(store.unreadCount).toBe(1)
  })

  it('adopts recovery snapshots without discarding cached messages', async () => {
    const store = useNewsStore()
    store.messages = [message('10')]
    vi.mocked(newsService.recheckNewsCredentials).mockResolvedValueOnce(liveStatus({ unreadCount: 4 }))
    vi.mocked(newsService.retryNewsSync).mockResolvedValueOnce(liveStatus({ unreadCount: 5 }))

    await store.recheckCredentials()
    await store.retrySync()

    expect(newsService.recheckNewsCredentials).toHaveBeenCalledWith()
    expect(newsService.retryNewsSync).toHaveBeenCalledWith()
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10'])
    expect(store.unreadCount).toBe(4)
  })

  it('loads the latest page when a recovery snapshot completes initial sync', async () => {
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(liveStatus({
      kind: 'initialSync', initialSyncComplete: false, latestDeliveryId: undefined, unreadCount: 0,
    }))
    vi.mocked(newsService.recheckNewsCredentials).mockResolvedValueOnce(liveStatus())
    const store = useNewsStore()
    await store.enterPage()
    vi.mocked(newsService.listNewsMessages).mockClear()

    await store.recheckCredentials()

    expect(newsService.listNewsMessages).toHaveBeenCalledOnce()
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['9007199254740995'])
  })
})
