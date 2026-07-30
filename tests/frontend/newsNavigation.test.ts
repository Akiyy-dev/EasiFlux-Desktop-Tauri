import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AppShell from '../../src/components/layout/AppShell.vue'
import NavigationRail from '../../src/components/layout/NavigationRail.vue'
import NewsCenterPage from '../../src/components/news/NewsCenterPage.vue'
import Sidebar from '../../src/components/layout/Sidebar.vue'
import TopBar from '../../src/components/layout/TopBar.vue'
import { useNewsStore } from '../../src/stores/news'
import * as newsService from '../../src/services/newsService'
import { useNewsRuntimeHost } from '../../src/composables/useNewsRuntimeHost'

vi.mock('../../src/composables/useNewsRuntimeHost', () => ({ useNewsRuntimeHost: vi.fn() }))
vi.mock('../../src/composables/useChartWorkspaceAutosaveHost', () => ({
  useChartWorkspaceAutosaveHost: vi.fn(),
}))
vi.mock('../../src/services/newsService', () => ({
  getNewsStatus: vi.fn(), listNewsMessages: vi.fn(), markNewsSeen: vi.fn(),
  recheckNewsCredentials: vi.fn(), retryNewsSync: vi.fn(),
}))
vi.mock('../../src/components/layout/TradingLayout.vue', () => ({
  default: { props: { active: Boolean }, template: '<div data-testid="trading-layout" />' },
}))
vi.mock('../../src/components/market/KlineChart.vue', () => ({
  default: { props: { active: Boolean, mode: String }, template: '<div data-testid="kline-chart" />' },
}))

async function select(wrapper: ReturnType<typeof mount>, label: string): Promise<void> {
  await wrapper.getComponent(NavigationRail).get(`button[aria-label^="${label}"]`).trigger('click')
  await flushPromises()
}

describe('news navigation integration', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(useNewsRuntimeHost).mockReset()
    vi.mocked(newsService.getNewsStatus).mockReset().mockResolvedValue({
      kind: 'live', initialSyncComplete: true, syncedCount: 2,
      latestDeliveryId: '10', unreadCount: 0,
    })
    vi.mocked(newsService.listNewsMessages).mockReset().mockResolvedValue({
      items: [], hasMore: false, latestDeliveryId: '10', unreadCount: 0,
    })
    vi.mocked(newsService.markNewsSeen).mockReset().mockResolvedValue({
      latestDeliveryId: '10', unreadCount: 0,
    })
  })

  function mountShell() {
    return mount(AppShell, {
      global: {
        plugins: [pinia],
        stubs: {
          DashboardPage: { template: '<div data-testid="dashboard-page" />' },
          AccountCenterPage: { template: '<div data-testid="account-page" />' },
        },
      },
    })
  }

  it('starts the app-wide runtime once before news is ever visited', () => {
    const wrapper = mountShell()

    expect(useNewsRuntimeHost).toHaveBeenCalledOnce()
    expect(wrapper.findComponent(NewsCenterPage).exists()).toBe(false)
  })

  it('retains one visited NewsCenterPage instance and drives active enter and leave', async () => {
    const store = useNewsStore()
    const enterPage = vi.spyOn(store, 'enterPage')
    const leavePage = vi.spyOn(store, 'leavePage')
    const wrapper = mountShell()

    await select(wrapper, '新闻')
    const first = wrapper.getComponent(NewsCenterPage).element
    expect(enterPage).toHaveBeenCalledTimes(1)

    await select(wrapper, '账户')
    expect(wrapper.getComponent(NewsCenterPage).isVisible()).toBe(false)
    expect(leavePage).toHaveBeenCalledTimes(1)

    await select(wrapper, '新闻')
    expect(wrapper.getComponent(NewsCenterPage).element).toBe(first)
    expect(enterPage).toHaveBeenCalledTimes(2)
  })

  it('retains loaded history and an away viewport while restoring a valid page session', async () => {
    let latestRequests = 0
    vi.mocked(newsService.listNewsMessages).mockImplementation(async (beforeDeliveryId) => {
      if (beforeDeliveryId === undefined) {
        latestRequests += 1
        if (latestRequests > 1) return {
          items: [
            { deliveryId: '11', createdAt: '2026-07-30T01:01:00Z', text: 'news-11' },
            { deliveryId: '10', createdAt: '2026-07-30T01:00:00Z', text: 'news-10' },
          ],
          hasMore: true, latestDeliveryId: '11', unreadCount: 1,
        }
        return {
        items: [
          { deliveryId: '10', createdAt: '2026-07-30T01:00:00Z', text: 'news-10' },
          { deliveryId: '9', createdAt: '2026-07-30T00:59:00Z', text: 'news-9' },
        ],
        hasMore: true, latestDeliveryId: '10', unreadCount: 0,
        }
      }
      if (beforeDeliveryId === '9') return {
        items: [{ deliveryId: '8', createdAt: '2026-07-30T00:58:00Z', text: 'news-8' }],
        hasMore: true, latestDeliveryId: '10', unreadCount: 0,
      }
      return {
        items: [{ deliveryId: '7', createdAt: '2026-07-30T00:57:00Z', text: 'news-7' }],
        hasMore: false, latestDeliveryId: '11', unreadCount: 1,
      }
    })
    const store = useNewsStore()
    const wrapper = mountShell()
    await select(wrapper, '新闻')
    await store.loadMore()
    await flushPromises()
    const scroll = wrapper.get<HTMLElement>('[data-testid="news-scroll-container"]')
    scroll.element.scrollTop = 160
    await scroll.trigger('scroll')
    const preservedText = wrapper.findAll('.news-message-text').map((node) => node.text())

    await select(wrapper, '账户')
    await store.handleMessagesCommitted({
      insertedCount: 1, newestDeliveryId: '11', unreadCount: 1, initialSyncComplete: true,
    })
    const callsBeforeReturn = vi.mocked(newsService.listNewsMessages).mock.calls.length
    await select(wrapper, '新闻')

    expect(newsService.listNewsMessages).toHaveBeenCalledTimes(callsBeforeReturn)
    expect(wrapper.findAll('.news-message-text').map((node) => node.text())).toEqual(preservedText)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10', '9', '8'])
    expect(store.hasMore).toBe(true)
    expect(store.isAtLatest).toBe(false)
    expect(store.pendingNewCount).toBe(1)
    expect(scroll.element.scrollTop).toBe(160)

    await store.loadMore()
    await store.handleMessagesCommitted({
      insertedCount: 2, newestDeliveryId: '12', unreadCount: 3, initialSyncComplete: true,
    })
    await flushPromises()

    expect(newsService.listNewsMessages).toHaveBeenLastCalledWith('8', 50)
    expect(store.messages.map(({ deliveryId }) => deliveryId)).toEqual(['10', '9', '8', '7'])
    expect(store.hasMore).toBe(false)
    expect(store.pendingNewCount).toBe(3)
    expect(scroll.element.scrollTop).toBe(160)
  })

  it('keeps TopBar and the rail mounted and hides Sidebar only for charts and news', async () => {
    const wrapper = mountShell()
    const top = wrapper.getComponent(TopBar).element
    const rail = wrapper.getComponent(NavigationRail).element

    await select(wrapper, '新闻')
    expect(wrapper.findComponent(Sidebar).exists()).toBe(false)
    expect(wrapper.getComponent(TopBar).element).toBe(top)
    expect(wrapper.getComponent(NavigationRail).element).toBe(rail)

    await select(wrapper, '图表')
    expect(wrapper.findComponent(Sidebar).exists()).toBe(false)

    await select(wrapper, '账户')
    expect(wrapper.findComponent(Sidebar).exists()).toBe(true)
  })

  it('passes app-wide unread state to the rail without clearing it on an empty visit', async () => {
    const store = useNewsStore()
    store.unreadCount = 100
    const wrapper = mountShell()

    expect(wrapper.getComponent(NavigationRail).get('.news-unread-badge').text()).toBe('99+')
    await select(wrapper, '新闻')

    expect(newsService.markNewsSeen).not.toHaveBeenCalled()
  })
})

describe('NavigationRail news badge', () => {
  it.each([
    [0, undefined, '新闻'],
    [1, '1', '新闻，1 条未读'],
    [99, '99', '新闻，99 条未读'],
    [100, '99+', '新闻，100 条未读'],
  ] as const)('renders unread %i without duplicate announcements', (count, badge, label) => {
    const wrapper = mount(NavigationRail, { props: { active: 'home', newsUnreadCount: count } })
    const button = wrapper.get(`button[aria-label="${label}"]`)

    if (badge === undefined) expect(button.find('.news-unread-badge').exists()).toBe(false)
    else {
      const node = button.get('.news-unread-badge')
      expect(node.text()).toBe(badge)
      expect(node.attributes('aria-hidden')).toBe('true')
    }
  })
})
