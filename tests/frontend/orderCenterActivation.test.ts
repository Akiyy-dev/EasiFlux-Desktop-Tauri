/* eslint-disable vue/one-component-per-file -- Local component stubs verify mount preservation. */
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { defineComponent, h, nextTick, onMounted } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import PositionsTab from '../../src/components/trading/order-center/PositionsTab.vue'
import TradeFillsTab from '../../src/components/trading/order-center/TradeFillsTab.vue'
import ClosedPnlTab from '../../src/components/trading/order-center/ClosedPnlTab.vue'
import OrderCenter from '../../src/components/trading/OrderCenter.vue'
import { useConnectionStore } from '../../src/stores/connection'

vi.mock('../../src/composables/useTauriCommand', () => ({
  tauriInvoke: vi.fn(),
}))

vi.mock('../../src/services/dataSyncService', () => ({
  refreshSyncTask: vi.fn(),
}))

import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { refreshSyncTask } from '../../src/services/dataSyncService'

describe('order center activation', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(tauriInvoke).mockResolvedValue([])
    vi.mocked(refreshSyncTask).mockReset()
    vi.mocked(refreshSyncTask).mockResolvedValue()
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
