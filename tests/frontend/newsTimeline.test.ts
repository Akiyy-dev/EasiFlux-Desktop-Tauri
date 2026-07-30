import { flushPromises, mount } from '@vue/test-utils'
import { openUrl } from '@tauri-apps/plugin-opener'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import NewsTimeline from '../../src/components/news/NewsTimeline.vue'
import type { NewsMessageDto } from '../../src/types/news'

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }))

const firstLocalTime = new Date(2026, 6, 30, 9, 8, 7).toISOString()
const secondLocalTime = new Date(2026, 6, 29, 23, 59, 58).toISOString()

function message(deliveryId: string, text: string, createdAt = firstLocalTime): NewsMessageDto {
  return { deliveryId, createdAt, text }
}

function mountTimeline(overrides: Partial<{
  items: NewsMessageDto[]
  hasMore: boolean
  loadingMore: boolean
  loadMoreError: string | null
}> = {}) {
  return mount(NewsTimeline, {
    props: {
      items: [],
      hasMore: true,
      loadingMore: false,
      loadMoreError: null,
      ...overrides,
    },
  })
}

describe('NewsTimeline', () => {
  beforeEach(() => vi.mocked(openUrl).mockReset().mockResolvedValue(undefined))

  it('sorts delivery IDs above Number safe range newest-first using exact integer order', () => {
    const wrapper = mountTimeline({
      items: [
        message('9007199254740992', 'middle'),
        message('10', 'oldest'),
        message('9007199254740993', 'newest'),
      ],
    })

    expect(wrapper.findAll('.news-message-text').map((node) => node.text()))
      .toEqual(['newest', 'middle', 'oldest'])
  })

  function arrivingText(wrapper: ReturnType<typeof mountTimeline>): string[] {
    return wrapper.findAll('.news-entry')
      .filter((entry) => entry.get('.news-item').classes().includes('is-arriving'))
      .map((entry) => entry.get('.news-message-text').text())
  }

  it('does not animate the first mount or the first page loaded from an empty list', async () => {
    const initialItems = Array.from({ length: 50 }, (_, index) =>
      message(String(50 - index), `initial-${50 - index}`))
    const mountedWithHistory = mountTimeline({ items: initialItems })
    const mountedEmpty = mountTimeline()

    expect(arrivingText(mountedWithHistory)).toEqual([])
    await mountedEmpty.setProps({ items: initialItems })
    expect(arrivingText(mountedEmpty)).toEqual([])
  })

  it('marks only strictly newer prepended IDs as arriving', async () => {
    const wrapper = mountTimeline({ items: [message('3', 'three'), message('2', 'two')] })

    await wrapper.setProps({
      items: [message('4', 'four'), message('2', 'two'), message('5', 'five'), message('3', 'three')],
    })

    expect(arrivingText(wrapper)).toEqual(['five', 'four'])
  })

  it('does not animate an older append', async () => {
    const wrapper = mountTimeline({ items: [message('3', 'three'), message('2', 'two')] })

    await wrapper.setProps({
      items: [message('3', 'three'), message('2', 'two'), message('1', 'one')],
    })

    expect(arrivingText(wrapper)).toEqual([])
  })

  it('animates only the newer IDs when prepend and append arrive together', async () => {
    const wrapper = mountTimeline({ items: [message('3', 'three'), message('2', 'two')] })

    await wrapper.setProps({
      items: [message('5', 'five'), message('4', 'four'), message('3', 'three'), message('2', 'two'), message('1', 'one')],
    })

    expect(arrivingText(wrapper)).toEqual(['five', 'four'])
  })

  it('clears an arriving class when that row finishes its background animation', async () => {
    const wrapper = mountTimeline({ items: [message('2', 'two'), message('1', 'one')] })
    await wrapper.setProps({ items: [message('3', 'three'), message('2', 'two'), message('1', 'one')] })

    const arriving = wrapper.get('.news-item.is-arriving')
    await arriving.trigger('animationend')

    expect(arrivingText(wrapper)).toEqual([])
  })

  it('groups by local date and renders local time to the second', () => {
    const wrapper = mountTimeline({
      items: [message('2', 'morning'), message('1', 'previous day', secondLocalTime)],
    })

    expect(wrapper.findAll('.news-date-label').map((node) => node.text()))
      .toEqual(['2026年7月30日', '2026年7月29日'])
    expect(wrapper.findAll('.news-time').map((node) => node.text()))
      .toEqual(['09:08:07', '23:59:58'])
    expect(wrapper.findAll('time')).toHaveLength(4)
  })

  it('keeps blank messages visible with the exact placeholder', () => {
    const wrapper = mountTimeline({ items: [message('1', '  \n\t ')] })

    expect(wrapper.get('.news-message-text').text()).toBe('该消息暂无可展示的文本内容')
  })

  it('opens only segmented http links through the system opener and absorbs opener failure', async () => {
    vi.mocked(openUrl).mockRejectedValueOnce(new Error('opener unavailable'))
    const wrapper = mountTimeline({
      items: [message('1', '详情 https://example.com/a；javascript:alert(1)')],
    })

    const link = wrapper.get('a')
    expect(link.text()).toBe('https://example.com/a')
    expect(wrapper.findAll('a')).toHaveLength(1)
    await expect(link.trigger('click')).resolves.toBeUndefined()
    await flushPromises()
    expect(openUrl).toHaveBeenCalledWith('https://example.com/a')
    expect(wrapper.text()).toContain('javascript:alert(1)')
  })

  it('renders message text as text and omits source, IDs, and media metadata', () => {
    const item = {
      ...message('998877', '<img src=x onerror=alert(1)> visible text'),
      username: 'hidden-source',
      sourceChatId: 'hidden-chat',
      sourceMessageId: 'hidden-message',
      mediaType: 'hidden-media',
    }
    const wrapper = mountTimeline({ items: [item] })

    expect(wrapper.find('img').exists()).toBe(false)
    expect(wrapper.text()).toContain('<img src=x onerror=alert(1)> visible text')
    for (const hidden of ['998877', 'hidden-source', 'hidden-chat', 'hidden-message', 'hidden-media']) {
      expect(wrapper.text()).not.toContain(hidden)
    }
  })

  it('emits one manual request from the normal pagination action', async () => {
    const wrapper = mountTimeline({ items: [message('1', 'news')] })
    const button = wrapper.get('button')

    expect(button.text()).toBe('加载更早消息')
    await button.trigger('click')
    expect(wrapper.emitted('loadMore')).toHaveLength(1)
  })

  it('disables pagination with a stable loading label while a request is active', async () => {
    const wrapper = mountTimeline({ loadingMore: true })
    const button = wrapper.get('button')

    expect(button.text()).toBe('正在加载更早消息')
    expect(button.attributes('disabled')).toBeDefined()
    expect(button.attributes('aria-busy')).toBe('true')
    await button.trigger('click')
    expect(wrapper.emitted('loadMore')).toBeUndefined()
  })

  it('preserves a retry action while replacing raw pagination errors with safe copy', async () => {
    const wrapper = mountTimeline({ loadMoreError: 'token=raw-secret' })

    expect(wrapper.get('[role="alert"]').text()).toBe('加载更早消息失败，请重试')
    expect(wrapper.text()).not.toContain('raw-secret')
    await wrapper.get('button').trigger('click')
    expect(wrapper.emitted('loadMore')).toHaveLength(1)
  })

  it('renders an inert completion state when no older news remains', () => {
    const wrapper = mountTimeline({ hasMore: false })

    expect(wrapper.text()).toContain('已显示全部新闻')
    expect(wrapper.find('button').exists()).toBe(false)
  })
})
