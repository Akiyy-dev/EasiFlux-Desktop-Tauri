import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import NewMessagesBanner from '../../src/components/news/NewMessagesBanner.vue'
import NewsStatusBar from '../../src/components/news/NewsStatusBar.vue'
import type { NewsStatusKind, NewsStatusSnapshot } from '../../src/types/news'

function status(kind: NewsStatusKind, overrides: Partial<NewsStatusSnapshot> = {}): NewsStatusSnapshot {
  return {
    kind,
    initialSyncComplete: kind !== 'initialSync',
    syncedCount: 0,
    unreadCount: 0,
    ...overrides,
  }
}

describe('NewsStatusBar', () => {
  it.each([
    ['deploymentMisconfigured', '新闻数据源未内置，请使用正确的安装包', null],
    ['notConfigured', '新闻服务未配置', '重新检查'],
    ['credentialStoreUnavailable', '凭据存储暂不可用', '重新检查'],
    ['initialSync', '正在同步历史新闻', null],
    ['live', '实时', null],
    ['retrying', '新闻服务暂时不可用，正在重试', null],
    ['credentialInvalid', '新闻服务凭据无效', '重新检查'],
    ['contractError', '新闻服务协议异常', '重试同步'],
    ['storageError', '新闻缓存暂不可用', '重试同步'],
    ['stopped', '新闻服务已停止', null],
  ] as const)('renders the Rust %s message and permitted recovery action', async (kind, copy, action) => {
    const wrapper = mount(NewsStatusBar, {
      props: { status: status(kind, { message: copy }), hasCachedMessages: false },
    })

    expect(wrapper.get('.news-status-label').text()).toBe(copy)
    const buttons = wrapper.findAll('button')
    expect(buttons.map((button) => button.text())).toEqual(action ? [action] : [])

    if (action) {
      await buttons[0].trigger('click')
      const event = action === '重新检查' ? 'recheckCredentials' : 'retrySync'
      expect(wrapper.emitted(event)).toHaveLength(1)
      const forbidden = event === 'recheckCredentials' ? 'retrySync' : 'recheckCredentials'
      expect(wrapper.emitted(forbidden)).toBeUndefined()
    }
  })

  it.each([
    ['deploymentMisconfigured', '新闻数据源未内置，请使用正确的安装包'],
    ['notConfigured', '新闻服务未配置'],
    ['credentialStoreUnavailable', '凭据存储暂不可用'],
    ['initialSync', '正在同步历史新闻'],
    ['live', '实时'],
    ['retrying', '新闻服务暂时不可用，正在重试'],
    ['credentialInvalid', '新闻服务凭据无效'],
    ['contractError', '新闻服务协议异常'],
    ['storageError', '新闻缓存暂不可用'],
    ['stopped', '新闻服务已停止'],
  ] as const)('uses the fixed %s fallback when the DTO message is absent', (kind, copy) => {
    const wrapper = mount(NewsStatusBar, {
      props: { status: status(kind), hasCachedMessages: false },
    })

    expect(wrapper.get('.news-status-label').text()).toBe(copy)
  })

  it('prefers the sanitized backend DTO message over the local fallback', () => {
    const wrapper = mount(NewsStatusBar, {
      props: {
        status: status('retrying', { message: '上游连接超时，正在重试' }),
        hasCachedMessages: false,
      },
    })

    expect(wrapper.get('.news-status-label').text()).toBe('上游连接超时，正在重试')
  })

  it('shows persisted initial-sync progress without exposing backend detail', () => {
    const wrapper = mount(NewsStatusBar, {
      props: {
        status: status('initialSync', { syncedCount: 27 }),
        hasCachedMessages: false,
      },
    })

    expect(wrapper.text()).toContain('已同步 27 条')
  })

  it('shows only a valid retry instant in local HH:mm:ss form', () => {
    const retryAt = new Date(2026, 6, 30, 9, 8, 7).toISOString()
    const valid = mount(NewsStatusBar, {
      props: { status: status('retrying', { retryAt }), hasCachedMessages: false },
    })
    const invalid = mount(NewsStatusBar, {
      props: { status: status('retrying', { retryAt: 'not-a-date' }), hasCachedMessages: false },
    })
    const absent = mount(NewsStatusBar, {
      props: { status: status('retrying'), hasCachedMessages: false },
    })

    expect(valid.text()).toContain('下次重试 09:08:07')
    expect(invalid.text()).not.toContain('下次重试')
    expect(invalid.text()).not.toContain('not-a-date')
    expect(absent.text()).not.toContain('下次重试')
  })

  it('explains that complete cached news remains readable during degradation', () => {
    const withCache = mount(NewsStatusBar, {
      props: { status: status('credentialInvalid'), hasCachedMessages: true },
    })
    const withoutCache = mount(NewsStatusBar, {
      props: { status: status('credentialInvalid'), hasCachedMessages: false },
    })

    expect(withCache.text()).toContain('已缓存新闻仍可阅读')
    expect(withoutCache.text()).not.toContain('已缓存新闻仍可阅读')
  })
})

describe('NewMessagesBanner', () => {
  it('announces the accumulated count and returns the user to latest news', async () => {
    const wrapper = mount(NewMessagesBanner, { props: { count: 12 } })

    expect(wrapper.get('[aria-live="polite"]').text()).toBe('有 12 条新消息')
    const button = wrapper.get('button')
    await button.trigger('click')
    expect(wrapper.emitted('showLatest')).toHaveLength(1)
  })

  it('does not render or announce an empty count', () => {
    const wrapper = mount(NewMessagesBanner, { props: { count: 0 } })

    expect(wrapper.html()).toBe('<!--v-if-->')
    expect(wrapper.find('[aria-live]').exists()).toBe(false)
  })
})
