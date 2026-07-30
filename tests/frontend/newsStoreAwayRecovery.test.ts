import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import * as newsService from '../../src/services/newsService'
import { useNewsStore } from '../../src/stores/news'
import type { NewsMessageDto, NewsPage, NewsStatusSnapshot } from '../../src/types/news'

vi.mock('../../src/services/newsService')

const item = (deliveryId: string): NewsMessageDto => ({
  deliveryId, createdAt: '2026-07-30T12:00:00Z', text: deliveryId,
})
const page = (ids: string[], hasMore: boolean, latestDeliveryId = ids[0]): NewsPage => ({
  items: ids.map(item), hasMore, latestDeliveryId, unreadCount: Math.min(ids.length, 100),
})
const status = (latestDeliveryId: string, unreadCount = 0): NewsStatusSnapshot => ({
  kind: 'live', initialSyncComplete: true, syncedCount: 1, latestDeliveryId, unreadCount,
})

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  const promise = new Promise<T>((ok) => { resolve = ok })
  return { promise, resolve }
}

async function flush(): Promise<void> {
  for (let index = 0; index < 8; index += 1) await Promise.resolve()
}

async function prepareAway(pendingNewCount = 0) {
  const store = useNewsStore()
  await store.initialize()
  store.messages = [item('10')]
  store.hasMore = true
  store.pendingNewCount = pendingNewCount
  store.isPageActive = true
  store.isAtLatest = false
  store.leavePage()
  return store
}

describe('news store away SQLite recovery', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.resetAllMocks()
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status('10'))
  })

  it('recovers one entirely missed inactive batch without mutating the away view', async () => {
    const store = await prepareAway()
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status('15', 5))
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce(page(
      ['15', '14', '13', '12', '11', '10'], false, '15',
    ))

    await store.enterPage()

    expect(newsService.listNewsMessages).toHaveBeenCalledWith(undefined, 50)
    expect(store.pendingNewCount).toBe(5)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10'])
    expect(store.hasMore).toBe(true)
    expect(store.isAtLatest).toBe(false)
  })

  it('counts missed rows exactly across more than one local page', async () => {
    const store = await prepareAway()
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status('70', 100))
    const first = Array.from({ length: 50 }, (_, index) => String(70 - index))
    const second = Array.from({ length: 11 }, (_, index) => String(20 - index))
    vi.mocked(newsService.listNewsMessages)
      .mockResolvedValueOnce(page(first, true, '70'))
      .mockResolvedValueOnce(page(second, false, '70'))

    await store.enterPage()

    expect(newsService.listNewsMessages.mock.calls).toEqual([[undefined, 50], ['21', 50]])
    expect(store.pendingNewCount).toBe(60)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10'])
  })

  it('restarts a scan advanced by a concurrent event and does not double count it', async () => {
    const store = await prepareAway()
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status('15', 5))
    const stale = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(stale.promise)
      .mockResolvedValueOnce(page(['17', '16', '15', '14', '13', '12', '11', '10'], false, '17'))

    const entering = store.enterPage()
    await flush()
    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(1)
    await store.handleMessagesCommitted({
      insertedCount: 2, newestDeliveryId: '17', unreadCount: 7, initialSyncComplete: true,
    })
    stale.resolve(page(['15', '14', '13', '12', '11', '10'], false, '15'))
    await entering

    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(2)
    expect(store.pendingNewCount).toBe(7)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10'])
  })

  it('preserves cached state and existing pending when the local scan fails', async () => {
    const store = await prepareAway(3)
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status('15', 5))
    vi.mocked(newsService.listNewsMessages).mockRejectedValueOnce(new Error('disk busy'))

    await expect(store.enterPage()).rejects.toThrow('disk busy')

    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10'])
    expect(store.hasMore).toBe(true)
    expect(store.pendingNewCount).toBe(3)
    expect(store.initialError).toBe('disk busy')
  })

  it('does not apply a completed scan after its page session leaves', async () => {
    const store = await prepareAway(3)
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status('15', 5))
    const pending = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages).mockReturnValueOnce(pending.promise)
    const entering = store.enterPage()
    await flush()
    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(1)

    store.leavePage()
    pending.resolve(page(['15', '14', '13', '12', '11', '10'], false, '15'))
    await entering

    expect(store.pendingNewCount).toBe(3)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10'])
    expect(store.isPageActive).toBe(false)
  })

  it('fails safely after three cache snapshots remain behind metadata', async () => {
    const store = await prepareAway(3)
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status('15', 5))
    vi.mocked(newsService.listNewsMessages).mockResolvedValue(page(['10'], false, '10'))

    await expect(store.enterPage()).rejects.toThrow('新闻缓存快照持续落后于同步状态')

    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(3)
    expect(store.pendingNewCount).toBe(3)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10'])
  })
})
