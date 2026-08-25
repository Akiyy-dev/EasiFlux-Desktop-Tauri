import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { describe, expect, it, vi } from 'vitest'
import NotificationList from '../../src/components/notifications/NotificationList.vue'
import NotificationPopover from '../../src/components/notifications/NotificationPopover.vue'
import TopBar from '../../src/components/layout/TopBar.vue'
import type { NotificationRecord } from '../../src/types/notification'
import { useNotificationStore } from '../../src/stores/notification'

vi.mock('naive-ui', () => ({
  NPopover: {
    name: 'NPopover',
    props: ['internalOnAfterLeave'],
    template: '<div><slot name="trigger" /><slot /></div>',
  },
}))

const now = 1_700_000_000_000
const today: NotificationRecord = {
  id: 'today', scope: { type: 'global' }, category: 'trading', kind: 'orderFilled', severity: 'success',
  content: { messageKey: 'order.filled', params: {}, fallbackTitle: '订单成交', fallbackBody: 'BTC 已成交' },
  dedupeKey: 'today', occurrenceCount: 1, createdAtMs: now - 1_000, updatedAtMs: now - 1_000,
}
const earlier = { ...today, id: 'earlier', createdAtMs: now - 86_400_000, updatedAtMs: now - 86_400_000 }

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

function mountList(overrides: Record<string, unknown> = {}) {
  return mount(NotificationList, {
    props: {
      items: [today, earlier], filter: 'all', unreadCount: 2, initialLoading: false, loadingMore: false,
      error: null, pageError: null, nextCursor: 'next', now, markReadPendingIds: new Set<string>(),
      removePendingIds: new Set<string>(), markAllReadPending: false, ...overrides,
    },
  })
}

describe('NotificationList', () => {
  it('groups backend order into local today and earlier without sorting', () => {
    const wrapper = mountList()
    expect(wrapper.text()).toContain('今天')
    expect(wrapper.text()).toContain('更早')
    expect(wrapper.findAll('article').map((item) => item.attributes('data-notification-id'))).toEqual(['today', 'earlier'])
  })

  it('renders skeleton only for an initial empty load and distinct empty copy per filter', () => {
    expect(mountList({ items: [], initialLoading: true }).findAll('[data-testid="notification-skeleton"]')).not.toHaveLength(0)
    expect(mountList({ items: [], filter: 'all' }).text()).toContain('暂无通知')
    expect(mountList({ items: [], filter: 'unread' }).text()).toContain('未读通知为空')
    expect(mountList({ initialLoading: true }).findAll('article')).toHaveLength(2)
  })

  it('emits semantic retry, load-more, mark-all, filter, activation, and deletion events', async () => {
    const wrapper = mountList()
    await wrapper.get('[data-testid="notification-filter-unread"]').trigger('click')
    await wrapper.get('[data-testid="notification-mark-all"]').trigger('click')
    await wrapper.get('[data-testid="notification-load-more"]').trigger('click')
    await wrapper.getComponent({ name: 'NotificationItem' }).vm.$emit('activate', today)
    await wrapper.getComponent({ name: 'NotificationItem' }).vm.$emit('delete', today.id)

    expect(wrapper.emitted('filter')).toEqual([['unread']])
    expect(wrapper.emitted('markAll')).toHaveLength(1)
    expect(wrapper.emitted('loadMore')).toHaveLength(1)
    expect(wrapper.emitted('activate')).toEqual([[today]])
    expect(wrapper.emitted('delete')).toEqual([[today.id]])
  })

  it('preserves cached items for page errors and offers the corresponding retry actions', async () => {
    const first = mountList({ items: [], error: '首次加载失败' })
    await first.get('[data-testid="notification-retry-first"]').trigger('click')
    expect(first.emitted('retryFirst')).toHaveLength(1)

    const page = mountList({ pageError: '下一页失败' })
    await page.get('[data-testid="notification-retry-page"]').trigger('click')
    expect(page.findAll('article')).toHaveLength(2)
    expect(page.emitted('retryPage')).toHaveLength(1)
  })

  it('keeps cached items visible with a polite first-page error and retry action', async () => {
    const wrapper = mountList({ error: '刷新通知失败' })

    expect(wrapper.findAll('article')).toHaveLength(2)
    expect(wrapper.get('[role="status"]').text()).toContain('刷新通知失败')
    await wrapper.get('[data-testid="notification-retry-first"]').trigger('click')
    expect(wrapper.emitted('retryFirst')).toHaveLength(1)
  })
})

describe('NotificationPopover', () => {
  it('focuses the rendered panel without waiting for a deferred store open', async () => {
    setActivePinia(createPinia())
    const pendingOpen = deferred<void>()
    vi.spyOn(useNotificationStore(), 'open').mockReturnValue(pendingOpen.promise)
    const before = document.createElement('button')
    document.body.append(before)
    before.focus()
    const wrapper = mount(NotificationPopover, {
      props: { show: false },
      attachTo: document.body,
    })

    await wrapper.setProps({ show: true })
    await wrapper.vm.$nextTick()

    expect(document.activeElement).toBe(wrapper.get('#notification-popover-dialog').element)
    pendingOpen.resolve()
    await flushPromises()
    wrapper.unmount()
    before.remove()
  })

  it('does not focus a closed panel when a deferred store open resolves', async () => {
    setActivePinia(createPinia())
    const pendingOpen = deferred<void>()
    vi.spyOn(useNotificationStore(), 'open').mockReturnValue(pendingOpen.promise)
    const outside = document.createElement('button')
    document.body.append(outside)
    const wrapper = mount(NotificationPopover, {
      props: { show: false },
      attachTo: document.body,
    })

    await wrapper.setProps({ show: true })
    await wrapper.setProps({ show: false })
    outside.focus()
    pendingOpen.resolve()
    await flushPromises()
    await wrapper.vm.$nextTick()

    expect(document.activeElement).toBe(outside)
    wrapper.unmount()
    outside.remove()
  })

  it('opens by loading the store only and forwards activation after a false mark-read result', async () => {
    setActivePinia(createPinia())
    const store = useNotificationStore()
    const open = vi.spyOn(store, 'open').mockResolvedValue(undefined)
    const markRead = vi.spyOn(store, 'markRead').mockResolvedValue(false)
    store.items = [{ ...today, action: { type: 'openTrading' } }]
    const wrapper = mount(NotificationPopover, { props: { show: false } })

    await wrapper.setProps({ show: true })
    await flushPromises()
    expect(open).toHaveBeenCalledTimes(1)
    await wrapper.getComponent({ name: 'NotificationList' }).vm.$emit('activate', store.items[0])
    await flushPromises()
    expect(markRead).toHaveBeenCalledWith('today')
    expect(wrapper.emitted('action')).toEqual([[{ type: 'openTrading' }]])
    expect(wrapper.emitted('update:show')).toBeUndefined()
  })

  it('waits for the popover to leave before opening notification settings', async () => {
    setActivePinia(createPinia())
    const wrapper = mount(NotificationPopover, { props: { show: true } })
    await wrapper.get('[data-testid="notification-settings"]').trigger('click')

    expect(wrapper.emitted('update:show')).toEqual([[false]])
    expect(wrapper.emitted('action')).toBeUndefined()

    const afterLeave = wrapper.getComponent({ name: 'NPopover' }).props('internalOnAfterLeave')
    expect(afterLeave).toBeTypeOf('function')
    ;(afterLeave as () => void)()
    await wrapper.vm.$nextTick()

    expect(wrapper.emitted('action')).toEqual([[{ type: 'openNotificationSettings' }]])
  })

  it('drops a canceled settings intent before a later ordinary leave', async () => {
    setActivePinia(createPinia())
    const wrapper = mount(NotificationPopover, { props: { show: true } })

    await wrapper.get('[data-testid="notification-settings"]').trigger('click')
    await wrapper.setProps({ show: false })
    await wrapper.setProps({ show: true })
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await flushPromises()
    await wrapper.setProps({ show: false })

    const afterLeave = wrapper.getComponent({ name: 'NPopover' }).props('internalOnAfterLeave')
    expect(afterLeave).toBeTypeOf('function')
    ;(afterLeave as () => void)()
    await wrapper.vm.$nextTick()

    expect(wrapper.emitted('action')).toBeUndefined()
  })

  it('requests close on Escape and removes its document listener when closed', async () => {
    setActivePinia(createPinia())
    const wrapper = mount(NotificationPopover, { props: { show: true } })
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await flushPromises()
    expect(wrapper.emitted('update:show')).toEqual([[false]])

    await wrapper.setProps({ show: false })
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    expect(wrapper.emitted('update:show')).toEqual([[false]])
  })

  it('requests close when a document click lands outside the controlled popover panel', async () => {
    setActivePinia(createPinia())
    const wrapper = mount(NotificationPopover, { props: { show: true }, attachTo: document.body })
    const outside = document.createElement('button')
    document.body.append(outside)

    outside.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await flushPromises()

    expect(wrapper.emitted('update:show')).toEqual([[false]])
    wrapper.unmount()
    outside.remove()
  })

  it.each([
    [null, undefined, '通知，未读数量未知'],
    [0, undefined, '通知，无未读'],
    [1, '1', '通知，1 条未读'],
    [99, '99', '通知，99 条未读'],
    [100, '99+', '通知，100 条未读'],
  ])('renders the %s unread bell state accessibly', async (unreadCount, badge, label) => {
    setActivePinia(createPinia())
    useNotificationStore().unreadCount = unreadCount as number | null
    const wrapper = mount(TopBar, { props: { title: '首页' } })
    await flushPromises()
    const bell = wrapper.get('[data-testid="notification-bell"]')
    expect(bell.attributes('aria-label')).toBe(label)
    expect(bell.attributes('aria-controls')).toBe('notification-popover-dialog')
    const badgeNode = bell.find('.top-bar__notification-badge')
    expect(badgeNode.exists() ? badgeNode.text() : undefined).toBe(badge)
    expect(bell.get('[aria-live="polite"]').text()).toContain(unreadCount === null ? '未知' : String(unreadCount))
  })
})
