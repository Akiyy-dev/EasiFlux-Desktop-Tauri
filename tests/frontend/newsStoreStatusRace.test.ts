import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import * as newsService from '../../src/services/newsService'
import { useNewsStore } from '../../src/stores/news'
import type { NewsStatusKind, NewsStatusSnapshot } from '../../src/types/news'

vi.mock('../../src/services/newsService')

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  const promise = new Promise<T>((ok) => { resolve = ok })
  return { promise, resolve }
}

function status(kind: NewsStatusKind, latestDeliveryId: string, message: string): NewsStatusSnapshot {
  return {
    kind, initialSyncComplete: true, syncedCount: 10,
    latestDeliveryId, unreadCount: 3, message,
  }
}

function expectPresentation(
  store: ReturnType<typeof useNewsStore>,
  kind: NewsStatusKind,
  latestDeliveryId: string,
  message: string,
): void {
  expect(store.status).toMatchObject({ kind, latestDeliveryId, message })
}

describe('news store status presentation ordering', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.resetAllMocks()
    vi.mocked(newsService.listNewsMessages).mockResolvedValue({
      items: [], hasMore: false, latestDeliveryId: '10', unreadCount: 0,
    })
  })

  it('keeps a newer-ID credential event presentation after an older initialize response', async () => {
    const pending = deferred<NewsStatusSnapshot>()
    vi.mocked(newsService.getNewsStatus).mockReturnValueOnce(pending.promise)
    const store = useNewsStore()
    const initializing = store.initialize()

    await store.handleStatusChanged(status('credentialInvalid', '12', '凭据无效'))
    pending.resolve({
      ...status('live', '10', '过期实时状态'), retryAt: '2026-07-30T01:00:00Z',
    })
    await initializing

    expectPresentation(store, 'credentialInvalid', '12', '凭据无效')
    expect(store.status?.retryAt).toBeUndefined()
  })

  it('keeps an equal-ID credential event presentation after an older initialize response', async () => {
    const pending = deferred<NewsStatusSnapshot>()
    vi.mocked(newsService.getNewsStatus).mockReturnValueOnce(pending.promise)
    const store = useNewsStore()
    const initializing = store.initialize()

    await store.handleStatusChanged(status('credentialInvalid', '10', '同一消息水位下凭据无效'))
    pending.resolve(status('live', '10', '过期实时状态'))
    await initializing

    expectPresentation(store, 'credentialInvalid', '10', '同一消息水位下凭据无效')
  })

  it('keeps a status event presentation received during enterPage', async () => {
    const pending = deferred<NewsStatusSnapshot>()
    vi.mocked(newsService.getNewsStatus).mockReturnValueOnce(pending.promise)
    const store = useNewsStore()
    store.messages = [{ deliveryId: '10', createdAt: '2026-07-30T12:00:00Z', text: '10' }]
    store.isAtLatest = false
    const entering = store.enterPage()

    await store.handleStatusChanged(status('credentialInvalid', '10', '进入页面期间凭据失效'))
    pending.resolve(status('live', '10', '过期实时状态'))
    await entering

    expectPresentation(store, 'credentialInvalid', '10', '进入页面期间凭据失效')
  })

  it('keeps a status event presentation received during credential recheck', async () => {
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status('live', '10', '实时'))
    const pending = deferred<NewsStatusSnapshot>()
    vi.mocked(newsService.recheckNewsCredentials).mockReturnValueOnce(pending.promise)
    const store = useNewsStore()
    await store.initialize()
    const rechecking = store.recheckCredentials()

    await store.handleStatusChanged(status('contractError', '10', '协议已变化'))
    pending.resolve(status('live', '10', '过期重检结果'))
    await rechecking

    expectPresentation(store, 'contractError', '10', '协议已变化')
  })

  it('keeps a status event presentation received during retrySync', async () => {
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status('live', '10', '实时'))
    const pending = deferred<NewsStatusSnapshot>()
    vi.mocked(newsService.retryNewsSync).mockReturnValueOnce(pending.promise)
    const store = useNewsStore()
    await store.initialize()
    const retrying = store.retrySync()

    await store.handleStatusChanged(status('storageError', '10', '缓存不可用'))
    pending.resolve(status('live', '10', '过期重试结果'))
    await retrying

    expectPresentation(store, 'storageError', '10', '缓存不可用')
  })
})
