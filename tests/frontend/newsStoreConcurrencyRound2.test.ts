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
const status = (latestDeliveryId = '10'): NewsStatusSnapshot => ({
  kind: 'live', initialSyncComplete: true, syncedCount: 1, latestDeliveryId, unreadCount: 1,
})
const item = (deliveryId: string): NewsMessageDto => ({
  deliveryId, createdAt: '2026-07-30T12:00:00Z', text: deliveryId,
})
const page = (ids: string[]): NewsPage => ({
  items: ids.map(item), hasMore: false, latestDeliveryId: ids[0], unreadCount: ids.length,
})
const event = (deliveryId: string, insertedCount = 1) => ({
  insertedCount, newestDeliveryId: deliveryId, unreadCount: insertedCount,
  initialSyncComplete: true,
})
async function flush(): Promise<void> {
  for (let index = 0; index < 6; index += 1) await Promise.resolve()
}

describe('news store review concurrency round 2', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.resetAllMocks()
    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status())
    vi.mocked(newsService.listNewsMessages).mockResolvedValue(page(['10']))
  })

  it('keeps the banner until showLatest catches metadata advanced during its request', async () => {
    const stale = deferred<NewsPage>()
    const successor = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(stale.promise).mockReturnValueOnce(successor.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = false
    store.pendingNewCount = 2
    store.messages = [item('8')]

    const showing = store.showLatest()
    await store.handleMessagesCommitted(event('12', 0))
    stale.resolve(page(['11']))
    await flush()

    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(2)
    expect(store.pendingNewCount).toBe(2)
    expect(store.isAtLatest).toBe(false)
    successor.resolve(page(['12', '11']))

    await expect(showing).resolves.toBe('12')
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['12', '11'])
    expect(store.pendingNewCount).toBe(0)
    expect(store.isAtLatest).toBe(true)
  })

  it('makes re-entry catch a newer status snapshot received during replacement', async () => {
    const stale = deferred<NewsPage>()
    const successor = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(stale.promise).mockReturnValueOnce(successor.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = false
    store.pendingNewCount = 2
    store.leavePage()

    const entering = store.enterPage()
    await flush()
    await store.handleStatusChanged(status('12'))
    stale.resolve(page(['11']))
    await flush()

    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(2)
    expect(store.pendingNewCount).toBe(2)
    successor.resolve(page(['12']))
    await entering
    expect(store.pendingNewCount).toBe(0)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['12'])
  })

  it('uses normal at-latest auto refresh for an event arriving after show lands', async () => {
    const automatic = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockResolvedValueOnce(page(['11'])).mockReturnValueOnce(automatic.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = false
    store.pendingNewCount = 1
    await expect(store.showLatest()).resolves.toBe('11')

    const committed = store.handleMessagesCommitted(event('12'))
    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(2)
    automatic.resolve(page(['12', '11']))
    await committed
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['12', '11'])
  })

  it('runs one queued successor after the first automatic attempt fails', async () => {
    const first = deferred<NewsPage>()
    const successor = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(first.promise).mockReturnValueOnce(successor.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = true
    store.messages = [item('10')]

    const firstEvent = store.handleMessagesCommitted(event('11')).catch((error: unknown) => error)
    const secondEvent = store.handleMessagesCommitted(event('12')).catch((error: unknown) => error)
    first.reject(new Error('first failed'))
    await flush()

    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(2)
    expect(store.refreshingLatest).toBe(true)
    successor.resolve(page(['12', '11']))

    expect(await firstEvent).toMatchObject({ message: 'first failed' })
    expect(await secondEvent).toMatchObject({ message: 'first failed' })
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['12', '11', '10'])
    expect(store.refreshingLatest).toBe(false)
  })

  it('settles one further successor when the prior successor fails during another event', async () => {
    const first = deferred<NewsPage>()
    const second = deferred<NewsPage>()
    const third = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages)
      .mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise).mockReturnValueOnce(third.promise)
    const store = useNewsStore()
    await store.initialize()
    store.isPageActive = true
    store.isAtLatest = true

    const results = [
      store.handleMessagesCommitted(event('11')).catch((error: unknown) => error),
      store.handleMessagesCommitted(event('12')).catch((error: unknown) => error),
    ]
    first.reject(new Error('first failed'))
    await flush()
    results.push(store.handleMessagesCommitted(event('13')).catch((error: unknown) => error))
    second.reject(new Error('second failed'))
    await flush()

    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(3)
    third.resolve(page(['13', '12', '11']))
    const errors = await Promise.all(results)
    expect(errors.map((error) => error.message)).toEqual(['first failed', 'first failed', 'first failed'])
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['13', '12', '11'])
    expect(store.refreshingLatest).toBe(false)
  })
})
