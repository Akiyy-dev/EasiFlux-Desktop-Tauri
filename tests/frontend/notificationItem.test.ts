import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import NotificationItem from '../../src/components/notifications/NotificationItem.vue'
import type { NotificationRecord } from '../../src/types/notification'

const record: NotificationRecord = {
  id: 'notice-1',
  scope: { type: 'account', accountId: 'primary' },
  category: 'riskAccount',
  kind: 'riskOrderBlocked',
  severity: 'warning',
  content: {
    messageKey: 'risk.orderBlocked',
    params: {},
    fallbackTitle: '风险订单已拦截',
    fallbackBody: '保证金不足，订单未提交。',
  },
  action: { type: 'openAccountSettings', accountSection: 'risk' },
  dedupeKey: 'risk:1',
  occurrenceCount: 2,
  createdAtMs: 1_700_000_000_000,
  updatedAtMs: 1_700_000_000_000,
}

function mountItem(overrides: Record<string, unknown> = {}) {
  return mount(NotificationItem, {
    props: {
      record,
      markReadPending: false,
      deletePending: false,
      now: record.createdAtMs + 30_000,
      ...overrides,
    },
  })
}

describe('NotificationItem', () => {
  it('renders one article with sibling primary and delete controls for an unread record', () => {
    const wrapper = mountItem()

    const article = wrapper.get('article')
    const buttons = article.findAll('button')
    expect(buttons).toHaveLength(2)
    expect(buttons[0].find('button').exists()).toBe(false)
    expect(article.attributes('data-unread')).toBe('true')
    expect(article.text()).toContain('未读')
    expect(article.text()).toContain('风险订单已拦截')
    expect(article.text()).toContain('保证金不足，订单未提交。')
    expect(article.text()).toContain('风险与账户')
    expect(article.text()).toContain('警告')
    expect(article.text()).toContain('刚刚')
    expect(article.text()).toContain('2 次')
    expect(buttons[1].attributes('aria-label')).toBe('删除通知')
    expect(article.get('time').attributes('datetime')).toBe(new Date(record.createdAtMs).toISOString())
  })

  it.each(['Enter', ' '])('does not duplicate %s activation when the native button click follows keydown', async (key) => {
    const wrapper = mountItem()
    const primary = wrapper.get('button[type="button"]')

    await primary.trigger('keydown', { key })
    await primary.trigger('click')

    expect(wrapper.emitted('activate')).toEqual([[record]])
  })

  it('emits delete only when Delete is pressed on the primary or delete button is clicked', async () => {
    const wrapper = mountItem()
    const [primary, deleteButton] = wrapper.findAll('button')

    await deleteButton.trigger('keydown', { key: 'Delete' })
    await primary.trigger('keydown', { key: 'Delete' })
    await deleteButton.trigger('click')

    expect(wrapper.emitted('delete')).toEqual([[record.id], [record.id]])
  })

  it('marks an in-flight item busy and disables both controls', () => {
    const wrapper = mountItem({ markReadPending: true })

    expect(wrapper.get('article').attributes('aria-busy')).toBe('true')
    expect(wrapper.findAll('button').every((button) => button.attributes('disabled') !== undefined)).toBe(true)
  })

  it('renders unread titles with stronger typography than read titles', () => {
    const unreadTitle = mountItem().get('.notification-item__title').element
    const readTitle = mountItem({ record: { ...record, readAtMs: record.createdAtMs + 1 } })
      .get('.notification-item__title').element

    expect(getComputedStyle(unreadTitle).fontWeight).not.toBe(getComputedStyle(readTitle).fontWeight)
  })
})
