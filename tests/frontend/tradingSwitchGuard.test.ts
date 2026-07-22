import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { nextTick } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import OpenOrdersTab from '../../src/components/trading/order-center/OpenOrdersTab.vue'
import OrderPanel from '../../src/components/trading/OrderPanel.vue'
import OrderTable from '../../src/components/trading/OrderTable.vue'
import { useOrderPanel } from '../../src/composables/useOrderPanel'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { refreshSyncTask } from '../../src/services/dataSyncService'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useConnectionStore } from '../../src/stores/connection'
import { useOrderStore } from '../../src/stores/order'
import type { AccountSwitchResult, AppConfig, Order } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))
vi.mock('../../src/services/dataSyncService', () => ({ refreshSyncTask: vi.fn() }))

const order: Order = {
  orderId: 'order-1', symbol: 'BTCUSDT', side: 'Buy', orderType: 'Limit',
  price: '60000', qty: '0.1', status: 'New', filledQty: '0', avgPrice: '0',
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

const mutationCommands = new Set(['place_order', 'cancel_order', 'cancel_all_orders'])

function privateMutationCalls(): string[] {
  return vi.mocked(tauriInvoke).mock.calls
    .map(([command]) => command)
    .filter((command) => mutationCommands.has(command))
}

describe('account-switch trading mutation guard', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(refreshSyncTask).mockReset()
    vi.mocked(refreshSyncTask).mockResolvedValue()
    useConnectionStore().setStatus('connected')
  })

  function beginSwitch() {
    const pending = deferred<AccountSwitchResult>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') return pending.promise
      if (command === 'list_account_profiles') return Promise.resolve([])
      if (command === 'get_connection_status') return Promise.resolve('disconnected')
      if (command === 'place_order' || command === 'cancel_order') return Promise.resolve(order)
      return Promise.resolve(undefined)
    })
    const switching = useAccountProfilesStore().switchAccount('backup')
    return { pending, switching }
  }

  async function finishSwitch(
    pending: ReturnType<typeof deferred<AccountSwitchResult>>,
    switching: Promise<AccountSwitchResult>,
  ): Promise<void> {
    pending.resolve({ activeAccountId: 'backup', connected: false, sessionEpoch: 1 })
    await switching
  }

  async function blockTradingWithBootstrapFailure() {
    let bootstrapCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 1 })
      }
      if (command === 'get_config') {
        return Promise.resolve({ activeAccountId: 'backup' } as AppConfig)
      }
      if (command === 'list_account_profiles') return Promise.resolve([])
      if (command === 'get_connection_status') return Promise.resolve('connected')
      if (command === 'scheduler_run_task') {
        bootstrapCalls += 1
        return bootstrapCalls === 1
          ? Promise.reject(new Error('bootstrap raw-secret'))
          : Promise.resolve(undefined)
      }
      if (command === 'place_order' || command === 'cancel_order') return Promise.resolve(order)
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    await store.switchAccount('backup')
    return store
  }

  it('rejects direct order-store writes without invoking Tauri while switching', async () => {
    const { pending, switching } = beginSwitch()
    const store = useOrderStore()
    await flushPromises()

    await expect(store.placeOrder({
      symbol: 'BTCUSDT', side: 'Buy', orderType: 'Market', qty: '0.1',
    })).rejects.toThrow('Account switch in progress')
    await expect(store.cancelOrder({ symbol: 'BTCUSDT', orderId: 'order-1' }))
      .rejects.toThrow('Account switch in progress')
    await expect(store.cancelAllOrders()).rejects.toThrow('Account switch in progress')
    expect(privateMutationCalls()).toEqual([])

    await finishSwitch(pending, switching)
  })

  it('keeps the normal store mutation path and skips an uncoordinated cancel-all refresh', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'place_order' || command === 'cancel_order') return Promise.resolve(order)
      if (command === 'refresh_orders') return Promise.resolve([])
      return Promise.resolve(undefined)
    })
    const store = useOrderStore()

    await store.placeOrder({ symbol: 'BTCUSDT', side: 'Buy', orderType: 'Market', qty: '0.1' })
    await store.cancelOrder({ symbol: 'BTCUSDT', orderId: 'order-1' })
    await store.cancelAllOrders()

    expect(privateMutationCalls()).toEqual(['place_order', 'cancel_order', 'cancel_all_orders'])
    expect(tauriInvoke).not.toHaveBeenCalledWith('refresh_orders', expect.anything())
  })

  it('disables and guards every current-orders cancellation path while switching', async () => {
    useOrderStore().setOpenOrders([order])
    const { pending, switching } = beginSwitch()
    const wrapper = mount(OpenOrdersTab, { props: { active: false }, global: { plugins: [pinia] } })
    await flushPromises()

    for (const label of ['撤单', '批量撤单', '一键全部撤单']) {
      const button = wrapper.findAll('button').find((item) => item.text() === label)
      expect(button?.attributes('disabled')).toBeDefined()
    }
    const vm = wrapper.vm as unknown as {
      cancelOne: (row: Order) => Promise<void>
      batchCancel: (rows: Order[]) => Promise<void>
      cancelAll: () => Promise<void>
    }
    await vm.cancelOne(order)
    await vm.batchCancel([order])
    await vm.cancelAll()
    expect(privateMutationCalls()).toEqual([])

    await finishSwitch(pending, switching)
  })

  it('disables and guards the legacy order-table cancel path while switching', async () => {
    useOrderStore().setOpenOrders([order])
    const { pending, switching } = beginSwitch()
    const wrapper = mount(OrderTable, { props: { active: false }, global: { plugins: [pinia] } })
    await flushPromises()

    const button = wrapper.findAll('button').find((item) => item.text() === '撤单')
    expect(button?.attributes('disabled')).toBeDefined()
    await (wrapper.vm as unknown as { cancel: (row: Order) => Promise<void> }).cancel(order)
    expect(privateMutationCalls()).toEqual([])

    await finishSwitch(pending, switching)
  })

  it('keeps programmatic order writes and the order panel blocked after bootstrap failure', async () => {
    const profilesStore = await blockTradingWithBootstrapFailure()
    const orderStore = useOrderStore()
    const panel = useOrderPanel()
    panel.qty.value = '0.1'
    panel.price.value = '60000'
    await nextTick()

    expect(profilesStore.switching).toBe(false)
    expect(profilesStore.tradingBlocked).toBe(true)
    expect(profilesStore.tradingBlockedMessage).toContain('synchronization failed')
    expect(panel.canSubmit.value).toBe(false)
    const orderPanel = mount(OrderPanel, { global: { plugins: [pinia] } })
    const blockedMessage = orderPanel.get('[data-testid="trading-blocked-message"]')
    expect(blockedMessage.attributes('role')).toBe('alert')
    expect(blockedMessage.text()).toContain('synchronization failed')
    await expect(orderStore.placeOrder({
      symbol: 'BTCUSDT', side: 'Buy', orderType: 'Limit', qty: '0.1', price: '60000',
    })).rejects.toThrow('synchronization failed')
    await expect(orderStore.cancelOrder({ symbol: 'BTCUSDT', orderId: 'order-1' }))
      .rejects.toThrow('synchronization failed')
    await expect(orderStore.cancelAllOrders()).rejects.toThrow('synchronization failed')
    await panel.submit()
    expect(privateMutationCalls()).toEqual([])

    await profilesStore.retryReconciliation()
    expect(profilesStore.tradingBlocked).toBe(false)
    expect(profilesStore.tradingBlockedMessage).toBeNull()
    expect(panel.canSubmit.value).toBe(true)

    await orderStore.placeOrder({
      symbol: 'BTCUSDT', side: 'Buy', orderType: 'Limit', qty: '0.1', price: '60000',
    })
    expect(privateMutationCalls()).toEqual(['place_order'])
  })

  it('keeps every cancellation UI disabled and guarded until reconciliation retry succeeds', async () => {
    const profilesStore = await blockTradingWithBootstrapFailure()
    useOrderStore().setOpenOrders([order])
    const current = mount(OpenOrdersTab, {
      props: { active: false }, global: { plugins: [pinia] },
    })
    const legacy = mount(OrderTable, {
      props: { active: false }, global: { plugins: [pinia] },
    })
    await flushPromises()

    for (const label of ['撤单', '批量撤单', '一键全部撤单']) {
      const button = current.findAll('button').find((item) => item.text() === label)
      expect(button?.attributes('disabled')).toBeDefined()
    }
    const legacyButton = legacy.findAll('button').find((item) => item.text() === '撤单')
    expect(legacyButton?.attributes('disabled')).toBeDefined()

    const currentVm = current.vm as unknown as {
      cancelOne: (row: Order) => Promise<void>
      batchCancel: (rows: Order[]) => Promise<void>
      cancelAll: () => Promise<void>
    }
    await currentVm.cancelOne(order)
    await currentVm.batchCancel([order])
    await currentVm.cancelAll()
    await (legacy.vm as unknown as { cancel: (row: Order) => Promise<void> }).cancel(order)
    expect(privateMutationCalls()).toEqual([])

    await profilesStore.retryReconciliation()
    await nextTick()
    expect(current.findAll('button').find((item) => item.text() === '撤单')
      ?.attributes('disabled')).toBeUndefined()
    expect(legacy.findAll('button').find((item) => item.text() === '撤单')
      ?.attributes('disabled')).toBeUndefined()
  })
})
