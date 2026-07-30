import { createPinia, setActivePinia } from 'pinia'
import { mount } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useNewsRuntimeHost } from '../../src/composables/useNewsRuntimeHost'

const mocks = vi.hoisted(() => ({
  order: [] as string[],
  handlers: new Map<string, (payload: unknown) => void>(),
  ready: vi.fn(),
  initialize: vi.fn(),
  committed: vi.fn(),
  status: vi.fn(),
  report: vi.fn(),
}))

vi.mock('../../src/composables/useTauriEvent', () => ({
  useTauriEvent: (event: string, handler: (payload: unknown) => void) => {
    mocks.order.push(`listen:${event}`)
    mocks.handlers.set(event, handler)
  },
  whenTauriListenersReady: () => { mocks.order.push('ready'); return mocks.ready() },
}))
vi.mock('../../src/stores/news', () => ({
  useNewsStore: () => ({
    initialize: mocks.initialize,
    handleMessagesCommitted: mocks.committed,
    handleStatusChanged: mocks.status,
  }),
}))
vi.mock('../../src/services/errorService', () => ({ reportError: mocks.report }))

const Harness = defineComponent({ setup() { useNewsRuntimeHost(); return () => h('div') } })

async function flush(): Promise<void> {
  await Promise.resolve()
  await Promise.resolve()
}

describe('news runtime host', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    mocks.order.length = 0
    mocks.handlers.clear()
    for (const mock of [mocks.ready, mocks.initialize, mocks.committed, mocks.status, mocks.report]) mock.mockReset()
    mocks.ready.mockImplementation(async () => { mocks.order.push('ready:done') })
    mocks.initialize.mockImplementation(async () => { mocks.order.push('initialize') })
    mocks.committed.mockResolvedValue(undefined)
    mocks.status.mockResolvedValue(undefined)
  })

  it('registers both listeners synchronously before readiness and initialization', async () => {
    mount(Harness)
    await flush()

    expect(mocks.order).toEqual([
      'listen:news://messages-committed',
      'listen:news://status-changed',
      'ready', 'ready:done', 'initialize',
    ])
  })

  it('still initializes from the SQLite snapshot when listener readiness rejects', async () => {
    mocks.ready.mockRejectedValueOnce(new Error('listen failed'))
    mount(Harness)
    await flush()

    expect(mocks.report).toHaveBeenCalledWith(expect.any(Error), '初始化新闻监听失败')
    expect(mocks.initialize).toHaveBeenCalledOnce()
  })

  it('contains and reports event handler, readiness, and initialization rejections', async () => {
    mocks.committed.mockRejectedValueOnce(new Error('event failed'))
    mocks.status.mockRejectedValueOnce(new Error('status failed'))
    mocks.ready.mockRejectedValueOnce(new Error('listen failed'))
    mount(Harness)
    mocks.handlers.get('news://messages-committed')?.({})
    mocks.handlers.get('news://status-changed')?.({})
    await flush()

    expect(mocks.report).toHaveBeenCalledWith(expect.any(Error), '处理新闻提交事件失败')
    expect(mocks.report).toHaveBeenCalledWith(expect.any(Error), '处理新闻状态事件失败')
    expect(mocks.report).toHaveBeenCalledWith(expect.any(Error), '初始化新闻监听失败')

    mocks.report.mockClear()
    mocks.ready.mockResolvedValueOnce(undefined)
    mocks.initialize.mockRejectedValueOnce(new Error('snapshot failed'))
    mount(Harness)
    await flush()
    expect(mocks.report).toHaveBeenCalledWith(expect.any(Error), '初始化新闻中心失败')
  })
})
