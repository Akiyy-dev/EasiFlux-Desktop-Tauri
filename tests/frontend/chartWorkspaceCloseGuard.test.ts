import { flushPromises, mount } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useChartWorkspaceCloseGuard } from '../../src/composables/useChartWorkspaceCloseGuard'
import { flushAllChartWorkspaces } from '../../src/services/chartWorkspaceFlushRegistry'
import { reportError } from '../../src/services/errorService'

const mocks = vi.hoisted(() => ({
  onCloseRequested: vi.fn(),
  destroy: vi.fn(),
  unlisten: vi.fn(),
  listener: null as null | ((event: { preventDefault: () => void }) => Promise<void> | void),
}))

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    onCloseRequested: mocks.onCloseRequested,
    destroy: mocks.destroy,
  }),
}))

vi.mock('../../src/services/chartWorkspaceFlushRegistry', () => ({
  flushAllChartWorkspaces: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('../../src/services/errorService', () => ({ reportError: vi.fn() }))

const Harness = defineComponent({
  setup() {
    useChartWorkspaceCloseGuard()
    return () => h('div')
  },
})

async function closeGuardFixture() {
  const wrapper = mount(Harness)
  await flushPromises()
  const events: Array<{ preventDefault: ReturnType<typeof vi.fn> }> = []
  const fireClose = async () => {
    const event = { preventDefault: vi.fn() }
    events.push(event)
    await mocks.listener?.(event)
    await flushPromises()
    return event
  }
  return { wrapper, events, fireClose }
}

describe('chart workspace close guard', () => {
  beforeEach(() => {
    vi.useRealTimers()
    mocks.listener = null
    mocks.unlisten.mockReset()
    mocks.onCloseRequested.mockReset().mockImplementation(async (listener) => {
      mocks.listener = listener
      return mocks.unlisten
    })
    mocks.destroy.mockReset().mockResolvedValue(undefined)
    vi.mocked(flushAllChartWorkspaces).mockReset().mockResolvedValue(undefined)
    vi.mocked(reportError).mockReset()
  })

  it('prevents close, awaits one flush, then destroys the window', async () => {
    const fixture = await closeGuardFixture()

    const event = await fixture.fireClose()

    expect(event.preventDefault).toHaveBeenCalledOnce()
    expect(flushAllChartWorkspaces).toHaveBeenCalledExactlyOnceWith('close')
    expect(mocks.destroy).toHaveBeenCalledOnce()
    fixture.wrapper.unmount()
    expect(mocks.unlisten).toHaveBeenCalledOnce()
  })

  it('still closes after a reported flush failure and does not recurse', async () => {
    vi.mocked(flushAllChartWorkspaces).mockRejectedValueOnce(new Error('disk full'))
    const fixture = await closeGuardFixture()

    const first = await fixture.fireClose()
    const second = await fixture.fireClose()

    expect(first.preventDefault).toHaveBeenCalledOnce()
    expect(second.preventDefault).toHaveBeenCalledOnce()
    expect(reportError).toHaveBeenCalledOnce()
    expect(mocks.destroy).toHaveBeenCalledOnce()
  })

  it('times out a stuck flush and still destroys the window', async () => {
    vi.useFakeTimers()
    vi.mocked(flushAllChartWorkspaces).mockReturnValueOnce(new Promise(() => undefined))
    const fixture = await closeGuardFixture()
    const event = { preventDefault: vi.fn() }

    const closing = mocks.listener?.(event)
    expect(event.preventDefault).toHaveBeenCalledOnce()
    expect(mocks.destroy).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(5_000)
    await closing

    expect(reportError).toHaveBeenCalledOnce()
    expect(mocks.destroy).toHaveBeenCalledOnce()
  })

  it('reports a destroy failure and allows a later close retry', async () => {
    mocks.destroy.mockRejectedValueOnce(new Error('window busy'))
    const fixture = await closeGuardFixture()

    await fixture.fireClose()
    expect(reportError).toHaveBeenCalledWith(
      expect.any(Error),
      '关闭图表窗口失败',
    )
    mocks.destroy.mockResolvedValueOnce(undefined)
    await fixture.fireClose()

    expect(mocks.destroy).toHaveBeenCalledTimes(2)
    expect(flushAllChartWorkspaces).toHaveBeenCalledTimes(2)
  })

  it('coalesces close requests while flushing', async () => {
    const pending = deferred<void>()
    vi.mocked(flushAllChartWorkspaces).mockReturnValueOnce(pending.promise)
    const fixture = await closeGuardFixture()
    const first = { preventDefault: vi.fn() }
    const second = { preventDefault: vi.fn() }

    const closing = mocks.listener?.(first)
    await mocks.listener?.(second)
    expect(first.preventDefault).toHaveBeenCalledOnce()
    expect(second.preventDefault).toHaveBeenCalledOnce()
    expect(flushAllChartWorkspaces).toHaveBeenCalledOnce()
    pending.resolve()
    await closing
    expect(mocks.destroy).toHaveBeenCalledOnce()
  })
})

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}
