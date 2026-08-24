/* eslint-disable vue/one-component-per-file -- Local component stubs verify mount preservation. */
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import type { MessageApi } from 'naive-ui'
import { defineComponent, h, nextTick, onMounted } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import PositionsTab from '../../src/components/trading/order-center/PositionsTab.vue'
import TradeFillsTab from '../../src/components/trading/order-center/TradeFillsTab.vue'
import ClosedPnlTab from '../../src/components/trading/order-center/ClosedPnlTab.vue'
import OrderCenter from '../../src/components/trading/OrderCenter.vue'
import OrderTable from '../../src/components/trading/OrderTable.vue'
import PositionTable from '../../src/components/trading/PositionTable.vue'
import { installMessageApi } from '../../src/services/errorService'
import { useConnectionStore } from '../../src/stores/connection'
import { useLogStore } from '../../src/stores/log'

vi.mock('../../src/composables/useTauriCommand', () => ({
  tauriInvoke: vi.fn(),
}))

vi.mock('../../src/services/dataSyncService', () => ({
  refreshSyncTask: vi.fn(),
}))

import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { refreshSyncTask } from '../../src/services/dataSyncService'

function notifiedSessionError(): Error {
  return Object.assign(new Error('private request failed'), {
    cause: {
      code: 'AUTH_SESSION_EXPIRED',
      message: '账户会话已失效',
      notificationId: 'notification-session-query',
    },
  })
}

describe('order center activation', () => {
  let pinia: Pinia
  let toastError: ReturnType<typeof vi.fn>

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(tauriInvoke).mockResolvedValue([])
    vi.mocked(refreshSyncTask).mockReset()
    vi.mocked(refreshSyncTask).mockResolvedValue()
    toastError = vi.fn()
    installMessageApi({ error: toastError } as unknown as MessageApi)
    useLogStore().clear()
    useLogStore().clearError()
  })

  it('defers shared private-panel refresh until activation', async () => {
    useConnectionStore().setStatus('connected')
    const wrapper = mount(PositionsTab, {
      props: { active: false },
      global: { plugins: [pinia], stubs: { TanstackDataTable: true } },
    })

    await flushPromises()
    expect(refreshSyncTask).not.toHaveBeenCalled()

    await wrapper.setProps({ active: true })
    await flushPromises()
    expect(refreshSyncTask).toHaveBeenCalledTimes(1)
  })

  it('does not refresh an inactive shared tab when connection becomes ready', async () => {
    const connection = useConnectionStore()
    mount(PositionsTab, {
      props: { active: false },
      global: { plugins: [pinia], stubs: { TanstackDataTable: true } },
    })

    connection.setStatus('connected')
    await nextTick()
    await flushPromises()

    expect(refreshSyncTask).not.toHaveBeenCalled()
  })

  it('defers trade-fill requests until activation', async () => {
    useConnectionStore().setStatus('connected')
    const wrapper = mount(TradeFillsTab, {
      props: { active: false },
      global: { plugins: [pinia], stubs: { TanstackDataTable: true } },
    })

    await flushPromises()
    expect(tauriInvoke).not.toHaveBeenCalled()

    await wrapper.setProps({ active: true })
    await flushPromises()
    expect(tauriInvoke).toHaveBeenCalledTimes(1)
    expect(tauriInvoke).toHaveBeenCalledWith('fetch_trade_fills', expect.any(Object))
  })

  it('defers closed-PnL requests until activation', async () => {
    useConnectionStore().setStatus('connected')
    const wrapper = mount(ClosedPnlTab, {
      props: { active: false },
      global: { plugins: [pinia], stubs: { TanstackDataTable: true } },
    })

    await flushPromises()
    expect(tauriInvoke).not.toHaveBeenCalled()

    await wrapper.setProps({ active: true })
    await flushPromises()
    expect(tauriInvoke).toHaveBeenCalledTimes(1)
    expect(tauriInvoke).toHaveBeenCalledWith('fetch_closed_pnl', expect.any(Object))
  })

  it('keeps every current query path as one-log one-Toast ordinary-error owner', async () => {
    useConnectionStore().setStatus('connected')

    const positions = mount(PositionsTab, {
      props: { active: false },
      global: { plugins: [pinia], stubs: { TanstackDataTable: true } },
    })
    const fills = mount(TradeFillsTab, {
      props: { active: false },
      global: { plugins: [pinia], stubs: { TanstackDataTable: true } },
    })
    const pnl = mount(ClosedPnlTab, {
      props: { active: false },
      global: { plugins: [pinia], stubs: { TanstackDataTable: true } },
    })
    const legacy = mount(PositionTable, {
      props: { active: false },
      global: { plugins: [pinia], stubs: { NButton: true, NDataTable: true } },
    })
    const legacyOrders = mount(OrderTable, {
      props: { active: false },
      global: { plugins: [pinia], stubs: { NButton: true, NDataTable: true } },
    })
    await flushPromises()

    const cases: Array<{
      invoke: () => Promise<void>
      reject: () => void
    }> = [
      {
        invoke: () => (positions.vm as unknown as { refresh: () => Promise<void> }).refresh(),
        reject: () => vi.mocked(refreshSyncTask).mockRejectedValueOnce(new Error('positions query failed')),
      },
      {
        invoke: () => (fills.vm as unknown as { refresh: () => Promise<void> }).refresh(),
        reject: () => vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('fills query failed')),
      },
      {
        invoke: () => (pnl.vm as unknown as { refresh: () => Promise<void> }).refresh(),
        reject: () => vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('pnl query failed')),
      },
      {
        invoke: () => (legacy.vm as unknown as {
          refreshPanels: () => Promise<void>
        }).refreshPanels(),
        reject: () => vi.mocked(refreshSyncTask).mockRejectedValueOnce(new Error('legacy query failed')),
      },
      {
        invoke: () => (legacyOrders.vm as unknown as {
          refreshPanels: () => Promise<void>
        }).refreshPanels(),
        reject: () => vi.mocked(refreshSyncTask).mockRejectedValueOnce(new Error('legacy orders failed')),
      },
    ]

    for (const testCase of cases) {
      useLogStore().clear()
      useLogStore().clearError()
      toastError.mockClear()
      testCase.reject()
      await testCase.invoke()
      expect(useLogStore().entries).toHaveLength(1)
      expect(toastError).toHaveBeenCalledTimes(1)
    }
  })

  it.each([
    'trade fills query',
    'closed PnL query',
  ])('keeps notified session expiry out of generic delivery for %s', async (owner) => {
    useConnectionStore().setStatus('connected')
    let invoke: () => Promise<void>

    if (owner === 'trade fills query') {
      const wrapper = mount(TradeFillsTab, {
        props: { active: false },
        global: { plugins: [pinia], stubs: { TanstackDataTable: true } },
      })
      invoke = () => (wrapper.vm as unknown as { refresh: () => Promise<void> }).refresh()
      vi.mocked(tauriInvoke).mockRejectedValueOnce(notifiedSessionError())
    } else {
      const wrapper = mount(ClosedPnlTab, {
        props: { active: false },
        global: { plugins: [pinia], stubs: { TanstackDataTable: true } },
      })
      invoke = () => (wrapper.vm as unknown as { refresh: () => Promise<void> }).refresh()
      vi.mocked(tauriInvoke).mockRejectedValueOnce(notifiedSessionError())
    }

    useLogStore().clear()
    useLogStore().clearError()
    toastError.mockClear()
    await invoke()

    expect(useLogStore().entries).toHaveLength(0)
    expect(toastError).toHaveBeenCalledTimes(0)
  })

  it('keeps tab component instances mounted while switching tabs', async () => {
    const positionsMounted = vi.fn()
    const positionsStub = defineComponent({
      props: { active: Boolean },
      setup() {
        onMounted(positionsMounted)
        return () => h('div')
      },
    })
    const passiveStub = defineComponent({
      props: { active: Boolean },
      setup: () => () => h('div'),
    })
    const wrapper = mount(OrderCenter, {
      global: {
        plugins: [pinia],
        stubs: {
          PositionsTab: positionsStub,
          OpenOrdersTab: passiveStub,
          OrderHistoryTab: passiveStub,
          TradeFillsTab: passiveStub,
          ClosedPnlTab: passiveStub,
        },
      },
    })

    expect(positionsMounted).toHaveBeenCalledTimes(1)
    await wrapper.findAll('[role="tab"]')[1].trigger('click')
    await nextTick()

    expect(positionsMounted).toHaveBeenCalledTimes(1)
  })
})
