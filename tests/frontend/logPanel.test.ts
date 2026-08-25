import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import LogPanel from '../../src/components/common/LogPanel.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useConnectionStore } from '../../src/stores/connection'
import { useLogStore } from '../../src/stores/log'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

function endpoint(overrides: Record<string, unknown> = {}) {
  return {
    endpoint: 'activity-orders',
    success: false,
    code: undefined,
    dataType: '',
    dataKeys: [],
    envelopeHint: '',
    rawCount: 0,
    parsedCount: 0,
    firstItemKeys: [],
    error: '账户会话已失效',
    ...overrides,
  }
}

describe('LogPanel private probe ownership', () => {
  let pinia: ReturnType<typeof createPinia>

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(tauriInvoke).mockReset()
    useConnectionStore().setStatus('connected')
  })

  it('keeps a typed probe notification marker out of repeated log delivery', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce({
      balancesOk: false,
      balanceCount: 0,
      endpoints: [endpoint({ notificationId: 'notification-probe-session' })],
    })
    const wrapper = mount(LogPanel, {
      global: { plugins: [pinia], stubs: { NButton: true } },
    })

    await (wrapper.vm as unknown as { runProbe: () => Promise<void> }).runProbe()

    expect(useLogStore().entries).toHaveLength(0)
  })

  it('keeps ordinary probe failures visible in the log panel', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce({
      balancesOk: false,
      balanceCount: 0,
      endpoints: [endpoint({ error: '连接失败，请检查网络后重试' })],
    })
    const wrapper = mount(LogPanel, {
      global: { plugins: [pinia], stubs: { NButton: true } },
    })

    await (wrapper.vm as unknown as { runProbe: () => Promise<void> }).runProbe()

    expect(useLogStore().entries).toHaveLength(2)
    expect(useLogStore().entries[0].level).toBe('error')
  })
})
