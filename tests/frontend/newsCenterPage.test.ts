import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import NewsCenterPage from '../../src/components/news/NewsCenterPage.vue'
import type { NewsMessageDto, NewsPage, NewsStatusKind, NewsStatusSnapshot } from '../../src/types/news'
import { useNewsStore } from '../../src/stores/news'
import * as newsService from '../../src/services/newsService'

vi.mock('../../src/services/newsService', () => ({
  getNewsStatus: vi.fn(),
  listNewsMessages: vi.fn(),
  markNewsSeen: vi.fn(),
  recheckNewsCredentials: vi.fn(),
  retryNewsSync: vi.fn(),
}))

const createdAt = new Date(2026, 6, 30, 9, 8, 7).toISOString()

function message(deliveryId: string): NewsMessageDto {
  return { deliveryId, createdAt, text: `news-${deliveryId}` }
}

function status(kind: NewsStatusKind = 'live', overrides: Partial<NewsStatusSnapshot> = {}): NewsStatusSnapshot {
  return {
    kind,
    initialSyncComplete: kind !== 'initialSync',
    syncedCount: 0,
    latestDeliveryId: '10',
    unreadCount: 1,
    ...overrides,
  }
}

function page(ids: string[], overrides: Partial<NewsPage> = {}): NewsPage {
  return {
    items: ids.map(message),
    hasMore: true,
    latestDeliveryId: ids[0],
    unreadCount: 1,
    ...overrides,
  }
}

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

async function settle(): Promise<void> {
  await flushPromises()
  await nextTick()
  await flushPromises()
}

function mountPage(active = true) {
  const pinia = createPinia()
  setActivePinia(pinia)
  const store = useNewsStore()
  const wrapper = mount(NewsCenterPage, { props: { active }, global: { plugins: [pinia] } })
  return { wrapper, store }
}

function installScroller(element: HTMLElement) {
  const scrollTo = vi.fn((options: ScrollToOptions | number, y?: number) => {
    element.scrollTop = typeof options === 'number' ? (y ?? 0) : (options.top ?? element.scrollTop)
  })
  Object.defineProperty(element, 'scrollTo', { configurable: true, value: scrollTo })
  return scrollTo
}

describe('NewsCenterPage', () => {
  beforeEach(() => {
    vi.mocked(newsService.getNewsStatus).mockReset().mockResolvedValue(status())
    vi.mocked(newsService.listNewsMessages).mockReset().mockResolvedValue(page(['10', '9']))
    vi.mocked(newsService.markNewsSeen).mockReset().mockResolvedValue({ latestDeliveryId: '10', unreadCount: 0 })
    vi.mocked(newsService.recheckNewsCredentials).mockReset().mockResolvedValue(status())
    vi.mocked(newsService.retryNewsSync).mockReset().mockResolvedValue(status())
  })

  it('treats 24px as latest and avoids duplicate latest-state updates', async () => {
    const { wrapper, store } = mountPage()
    await settle()
    const setAtLatest = vi.spyOn(store, 'setAtLatest')
    const scroll = wrapper.get<HTMLElement>('[data-testid="news-scroll-container"]')

    scroll.element.scrollTop = 25
    await scroll.trigger('scroll')
    scroll.element.scrollTop = 26
    await scroll.trigger('scroll')
    scroll.element.scrollTop = 24
    await scroll.trigger('scroll')

    expect(setAtLatest.mock.calls).toEqual([[false], [true]])
  })

  it('hides the timeline during initial sync and shows cumulative progress', async () => {
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status('initialSync', {
      initialSyncComplete: false, latestDeliveryId: undefined, unreadCount: 0, syncedCount: 27,
    }))
    const { wrapper } = mountPage()
    await settle()

    expect(wrapper.text()).toContain('已同步 27 条')
    expect(wrapper.find('.news-timeline').exists()).toBe(false)
    expect(wrapper.get('.news-initial-state').text()).toContain('正在准备最新新闻')
  })

  it('keeps a complete cached timeline visible below a degraded status', async () => {
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status('credentialInvalid'))
    const { wrapper } = mountPage()
    await settle()

    expect(wrapper.get('.news-status-bar').text()).toContain('已缓存新闻仍可阅读')
    expect(wrapper.get('.news-timeline').text()).toContain('news-10')
  })

  it('shows loading after status arrives while the first complete page is still pending', async () => {
    const pending = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages).mockReturnValueOnce(pending.promise)
    const { wrapper } = mountPage()
    await flushPromises()
    await nextTick()

    expect(wrapper.get('.news-status-bar').text()).toContain('实时')
    expect(wrapper.get('.news-loading-state').text()).toContain('正在加载新闻')
    expect(wrapper.find('.news-timeline').exists()).toBe(false)

    pending.resolve(page(['10', '9']))
    await settle()
    expect(wrapper.get('.news-timeline').text()).toContain('news-10')
  })

  it('marks the first rendered top after initial sync completes while the page stays latest', async () => {
    vi.mocked(newsService.getNewsStatus).mockResolvedValueOnce(status('initialSync', {
      initialSyncComplete: false, latestDeliveryId: undefined, unreadCount: 0,
    }))
    const { wrapper, store } = mountPage()
    await settle()
    vi.mocked(newsService.markNewsSeen).mockClear()
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce(page(['10', '9']))

    await store.handleStatusChanged(status('live'))
    await settle()

    expect(wrapper.get('.news-entry:first-child .news-message-text').text()).toBe('news-10')
    expect(newsService.markNewsSeen).toHaveBeenCalledWith('10')
  })

  it('keeps the top after a prepended refresh and marks only the rendered top seen', async () => {
    const { wrapper, store } = mountPage()
    await settle()
    vi.mocked(newsService.markNewsSeen).mockClear()
    vi.mocked(newsService.markNewsSeen).mockResolvedValue({ latestDeliveryId: '11', unreadCount: 0 })
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce(page(['11', '10', '9'], {
      latestDeliveryId: '11', unreadCount: 1,
    }))
    const scroll = wrapper.get<HTMLElement>('[data-testid="news-scroll-container"]')
    const scrollTo = installScroller(scroll.element)
    scroll.element.scrollTop = 0

    await store.handleMessagesCommitted({
      insertedCount: 1, newestDeliveryId: '11', unreadCount: 1, initialSyncComplete: true,
    })
    await settle()

    expect(wrapper.get('.news-entry:first-child .news-message-text').text()).toBe('news-11')
    expect(scrollTo).toHaveBeenCalledWith({ top: 0 })
    expect(newsService.markNewsSeen).toHaveBeenCalledWith('11')
  })

  it('preserves the away DOM and viewport while accumulating the banner', async () => {
    const { wrapper, store } = mountPage()
    await settle()
    const scroll = wrapper.get<HTMLElement>('[data-testid="news-scroll-container"]')
    scroll.element.scrollTop = 160
    await scroll.trigger('scroll')
    const entries = wrapper.findAll('.news-message-text').map((node) => node.text())

    await store.handleMessagesCommitted({
      insertedCount: 2, newestDeliveryId: '12', unreadCount: 3, initialSyncComplete: true,
    })
    await settle()

    expect(wrapper.findAll('.news-message-text').map((node) => node.text())).toEqual(entries)
    expect(scroll.element.scrollTop).toBe(160)
    expect(wrapper.get('.new-messages-banner').text()).toBe('有 2 条新消息')
  })

  it('reveals and marks the new rendered top only after a successful banner request', async () => {
    const { wrapper, store } = mountPage()
    await settle()
    const scroll = wrapper.get<HTMLElement>('[data-testid="news-scroll-container"]')
    const scrollTo = installScroller(scroll.element)
    scroll.element.scrollTop = 160
    await scroll.trigger('scroll')
    await store.handleMessagesCommitted({
      insertedCount: 1, newestDeliveryId: '11', unreadCount: 2, initialSyncComplete: true,
    })
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce(page(['11', '10'], {
      latestDeliveryId: '11', unreadCount: 2,
    }))
    vi.mocked(newsService.markNewsSeen).mockClear()
    vi.mocked(newsService.markNewsSeen).mockResolvedValue({ latestDeliveryId: '11', unreadCount: 0 })

    await wrapper.get('.new-messages-banner').trigger('click')
    await settle()

    expect(scrollTo).toHaveBeenCalledWith({ top: 0 })
    expect(wrapper.get('.news-entry:first-child .news-message-text').text()).toBe('news-11')
    expect(newsService.markNewsSeen).toHaveBeenCalledWith('11')
  })

  it('does not scroll or mark when a banner request fails or becomes stale', async () => {
    const failed = mountPage()
    await settle()
    let scroll = failed.wrapper.get<HTMLElement>('[data-testid="news-scroll-container"]')
    let scrollTo = installScroller(scroll.element)
    scroll.element.scrollTop = 160
    await scroll.trigger('scroll')
    await failed.store.handleMessagesCommitted({
      insertedCount: 1, newestDeliveryId: '11', unreadCount: 2, initialSyncComplete: true,
    })
    vi.mocked(newsService.listNewsMessages).mockRejectedValueOnce(new Error('offline'))
    vi.mocked(newsService.markNewsSeen).mockClear()
    await failed.wrapper.get('.new-messages-banner').trigger('click')
    await settle()
    expect(scrollTo).not.toHaveBeenCalled()
    expect(newsService.markNewsSeen).not.toHaveBeenCalled()

    vi.mocked(newsService.getNewsStatus).mockResolvedValue(status())
    vi.mocked(newsService.listNewsMessages).mockResolvedValue(page(['10', '9']))
    const stale = mountPage()
    await settle()
    expect(newsService.markNewsSeen).toHaveBeenCalledTimes(1)
    expect(newsService.markNewsSeen).toHaveBeenLastCalledWith('10')
    const marksBeforeStaleBanner = vi.mocked(newsService.markNewsSeen).mock.calls.length
    scroll = stale.wrapper.get<HTMLElement>('[data-testid="news-scroll-container"]')
    scrollTo = installScroller(scroll.element)
    scroll.element.scrollTop = 160
    await scroll.trigger('scroll')
    await stale.store.handleMessagesCommitted({
      insertedCount: 1, newestDeliveryId: '11', unreadCount: 2, initialSyncComplete: true,
    })
    const pending = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages).mockReturnValueOnce(pending.promise)
    await stale.wrapper.get('.new-messages-banner').trigger('click')
    await stale.wrapper.setProps({ active: false })
    pending.resolve(page(['11', '10'], { latestDeliveryId: '11' }))
    await settle()
    expect(scrollTo).not.toHaveBeenCalled()
    expect(newsService.markNewsSeen).toHaveBeenCalledTimes(marksBeforeStaleBanner)
  })

  it('fetches pending news before marking when the user manually returns to top', async () => {
    const { wrapper, store } = mountPage()
    await settle()
    const scroll = wrapper.get<HTMLElement>('[data-testid="news-scroll-container"]')
    installScroller(scroll.element)
    scroll.element.scrollTop = 160
    await scroll.trigger('scroll')
    await store.handleMessagesCommitted({
      insertedCount: 1, newestDeliveryId: '11', unreadCount: 2, initialSyncComplete: true,
    })
    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce(page(['11', '10'], {
      latestDeliveryId: '11', unreadCount: 2,
    }))
    vi.mocked(newsService.markNewsSeen).mockClear()
    vi.mocked(newsService.markNewsSeen).mockResolvedValue({ latestDeliveryId: '11', unreadCount: 0 })

    scroll.element.scrollTop = 0
    await scroll.trigger('scroll')
    await settle()

    expect(wrapper.get('.news-entry:first-child .news-message-text').text()).toBe('news-11')
    expect(newsService.markNewsSeen).toHaveBeenCalledWith('11')
    expect(newsService.markNewsSeen).not.toHaveBeenCalledWith('10')
  })

  it('preserves an away viewport with the measured scroll-height delta for an in-place prepend', async () => {
    const { wrapper, store } = mountPage()
    await settle()
    const scroll = wrapper.get<HTMLElement>('[data-testid="news-scroll-container"]')
    scroll.element.scrollTop = 160
    await scroll.trigger('scroll')
    let reads = 0
    Object.defineProperty(scroll.element, 'scrollHeight', {
      configurable: true,
      get: () => {
        reads += 1
        if (reads === 1) return 1000
        scroll.element.scrollTop = 220
        return 1160
      },
    })

    store.messages = [message('11'), ...store.messages]
    await settle()

    expect(scroll.element.scrollTop).toBe(320)
  })

  it('returns to top after a prepend even when native anchoring moves scrollTop after render', async () => {
    const { wrapper, store } = mountPage()
    await settle()
    vi.mocked(newsService.markNewsSeen).mockClear()
    vi.mocked(newsService.markNewsSeen).mockResolvedValue({ latestDeliveryId: '11', unreadCount: 0 })
    const scroll = wrapper.get<HTMLElement>('[data-testid="news-scroll-container"]')
    const scrollTo = installScroller(scroll.element)
    scroll.element.scrollTop = 0
    let reads = 0
    Object.defineProperty(scroll.element, 'scrollHeight', {
      configurable: true,
      get: () => {
        reads += 1
        if (reads === 1) return 1000
        scroll.element.scrollTop = 160
        store.isAtLatest = false
        return 1160
      },
    })

    store.messages = [message('11'), ...store.messages]
    await settle()

    expect(scrollTo).toHaveBeenCalledWith({ top: 0 })
    expect(scroll.element.scrollTop).toBe(0)
    expect(store.isAtLatest).toBe(true)
    expect(newsService.markNewsSeen).toHaveBeenCalledWith('11')
  })

  it('dispatches only one load-more action while the first request is busy', async () => {
    const { wrapper } = mountPage()
    await settle()
    const pending = deferred<NewsPage>()
    vi.mocked(newsService.listNewsMessages).mockReturnValueOnce(pending.promise)
    const button = wrapper.get('.news-pagination button')

    await button.trigger('click')
    await button.trigger('click')

    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(2)
    expect(button.attributes('disabled')).toBeDefined()
    pending.resolve(page(['8'], { latestDeliveryId: '10', unreadCount: 0, hasMore: false }))
    await settle()
    expect(wrapper.text()).toContain('news-8')
  })

  it('renders explicit loading, empty, and safe error states without hiding cache', async () => {
    const loadingStatus = deferred<NewsStatusSnapshot>()
    vi.mocked(newsService.getNewsStatus).mockReturnValueOnce(loadingStatus.promise)
    const loading = mountPage()
    expect(loading.wrapper.get('.news-loading-state').text()).toContain('正在加载新闻')
    loadingStatus.resolve(status())
    await settle()

    vi.mocked(newsService.listNewsMessages).mockResolvedValueOnce(page([], {
      hasMore: false, latestDeliveryId: '10', unreadCount: 0,
    }))
    const empty = mountPage()
    await settle()
    expect(empty.wrapper.get('.news-empty-state').text()).toContain('暂无新闻')

    vi.mocked(newsService.getNewsStatus).mockRejectedValueOnce(new Error('token=secret'))
    const errored = mountPage()
    await settle()
    expect(errored.wrapper.get('.news-error-state').text()).toContain('新闻加载失败')
    expect(errored.wrapper.text()).not.toContain('token=secret')
  })

  it('uses a dedicated scroll surface and an 880px reading column', () => {
    const { wrapper } = mountPage(false)
    const scroll = wrapper.get<HTMLElement>('[data-testid="news-scroll-container"]')
    const column = wrapper.get<HTMLElement>('.news-reading-column')

    expect(scroll.classes()).toContain('news-scroll-container')
    expect(column.element.style.maxWidth).toBe('880px')
  })
})
