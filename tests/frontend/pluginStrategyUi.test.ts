import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, ref } from 'vue'
import PluginStrategyDialog from '../../src/components/plugins/PluginStrategyDialog.vue'
import PluginStrategyMonitor from '../../src/components/plugins/PluginStrategyMonitor.vue'
import PluginMarketplacePage from '../../src/components/plugins/PluginMarketplacePage.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import type { PluginStrategyExecutionIntent } from '../../src/types/plugin'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

const REQUEST_ID = '00000000-0000-4000-8000-000000000001'
const RUN_ID = '00000000-0000-4000-8000-000000000002'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

function intent(): PluginStrategyExecutionIntent {
  return {
    actionId: 'sandbox.strategy', pluginId: 'com.example.strategy',
    pluginName: 'Threshold strategy', contributionId: 'strategy.threshold',
    title: 'Threshold once', runtime: 'wasm-v1', abi: 'strategy-json-v1',
    defaultInput: '{"threshold":"50000"}',
    requestedCapabilities: ['account.read', 'market.read', 'trade.place', 'strategy.run'],
    expectedCatalogGeneration: '8', expectedRevision: '13',
  }
}

function accessFixture() {
  return {
    schemaVersion: 1, pluginId: 'com.example.strategy', contributionId: 'strategy.threshold',
    catalogGeneration: '8', revision: '13',
    account: { accountId: 'paper-main', sessionEpoch: '21', environment: 'Testnet' },
    requestedCapabilities: ['account.read', 'market.read', 'trade.place', 'strategy.run'],
    authorizationToken: 'strategy-ticket-1', expiresAtMs: '1789920060123',
  }
}

function runFixture(status = 'running', overrides: Record<string, unknown> = {}) {
  return {
    schemaVersion: 1, runId: RUN_ID, requestId: REQUEST_ID,
    pluginId: 'com.example.strategy', contributionId: 'strategy.threshold',
    account: { accountId: 'paper-main', sessionEpoch: '21', environment: 'Testnet' },
    symbol: 'BTCUSDT', status, reason: null,
    policy: {
      intervalMs: 5000, maxOrderQty: '0.001', maxTotalQty: '0.01',
      maxActions: 20, maxRunSeconds: 3600, reduceOnly: false,
    },
    capabilities: ['account.read', 'market.read', 'trade.place', 'strategy.run'],
    inputJson: '{"threshold":"50000"}', startedAtMs: '1799996400000',
    expiresAtMs: '1800000000000', sequence: '4', actionsSubmitted: 1,
    totalSubmittedQty: '0.001', lastMessage: 'Order accepted, not necessarily filled',
    lastReceipt: {
      sequence: '4', kind: 'placeOrder', status: 'accepted',
      submissionId: 'submission-1', orderId: 'exchange-order-1', errorCode: null,
    },
    ...overrides,
  }
}

async function mountDialog() {
  vi.mocked(tauriInvoke).mockResolvedValueOnce(accessFixture())
  const wrapper = mount(PluginStrategyDialog, { props: { intent: intent() } })
  await flushPromises()
  return wrapper
}

describe('strategy launch dialog', () => {
  beforeEach(() => {
    vi.resetAllMocks()
    vi.stubGlobal('crypto', { randomUUID: () => REQUEST_ID })
  })

  it('does not treat enabling or opening a strategy as permission to trade', async () => {
    const wrapper = await mountDialog()
    expect(wrapper.findAll('input[type=checkbox]').every(
      (box) => !(box.element as HTMLInputElement).checked,
    )).toBe(true)
    expect(wrapper.get('button[data-start-strategy]').attributes('disabled')).toBeDefined()
    expect(vi.mocked(tauriInvoke).mock.calls.filter(([command]) => command === 'start_plugin_strategy'))
      .toHaveLength(0)
  })

  it('starts exactly once after selected authority, bounded policy and automatic-trading consent', async () => {
    const wrapper = await mountDialog()
    for (const box of wrapper.findAll('[data-testid="strategy-capability"]')) {
      await box.setValue(true)
    }
    await wrapper.get('[data-testid="strategy-acknowledgement"]').setValue(true)
    vi.mocked(tauriInvoke).mockResolvedValueOnce(runFixture())

    await wrapper.get('[data-start-strategy]').trigger('click')
    await wrapper.get('[data-start-strategy]').trigger('click')
    await flushPromises()

    const starts = vi.mocked(tauriInvoke).mock.calls.filter(([command]) => command === 'start_plugin_strategy')
    expect(starts).toHaveLength(1)
    expect(starts[0][1]).toMatchObject({
      request: {
        authorizationToken: 'strategy-ticket-1', requestId: REQUEST_ID,
        acknowledgeAutomaticTrading: true,
        capabilities: ['account.read', 'market.read', 'trade.place', 'strategy.run'],
        policy: {
          intervalMs: 5000, maxOrderQty: '0.001', maxTotalQty: '0.01',
          maxActions: 20, maxRunSeconds: 3600, reduceOnly: false,
        },
      },
    })
    expect(vi.mocked(tauriInvoke).mock.calls.some(([command]) => command.includes('confirm'))).toBe(false)
    expect(wrapper.get('[data-testid="strategy-started"]').text()).toContain('running')
  })

  it('refreshes authoritative runs after an uncertain start without resubmitting', async () => {
    const wrapper = await mountDialog()
    for (const box of wrapper.findAll('[data-testid="strategy-capability"]')) await box.setValue(true)
    await wrapper.get('[data-testid="strategy-acknowledgement"]').setValue(true)
    vi.mocked(tauriInvoke)
      .mockRejectedValueOnce(new Error('transport lost'))
      .mockResolvedValueOnce({ schemaVersion: 1, runs: [runFixture()] })

    await wrapper.get('[data-start-strategy]').trigger('click')
    await flushPromises()

    expect(vi.mocked(tauriInvoke).mock.calls.filter(([command]) => command === 'start_plugin_strategy'))
      .toHaveLength(1)
    expect(vi.mocked(tauriInvoke).mock.calls.filter(([command]) => command === 'list_plugin_strategies'))
      .toHaveLength(1)
    expect(wrapper.get('[data-testid="strategy-start-uncertain"]').text())
      .toContain('不会自动重试')
  })
})

describe('persistent strategy monitor', () => {
  beforeEach(() => {
    vi.resetAllMocks()
    vi.stubGlobal('crypto', { randomUUID: () => REQUEST_ID })
  })

  it('keeps the native run visible without a dialog and explains budgets and accepted versus filled', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce({ schemaVersion: 1, runs: [runFixture()] })
    const wrapper = mount(PluginStrategyMonitor, { props: { strategyIntents: [] } })
    await flushPromises()

    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('0.001 / 0.01')
    expect(wrapper.text()).toContain('数量单位')
    expect(wrapper.text()).toContain('accepted 不代表成交')
    expect(wrapper.text()).toContain('停止不会自动撤销交易所订单')
    expect(wrapper.find('[data-testid="strategy-dialog"]').exists()).toBe(false)
  })

  it('continues monitoring the native run after the launch dialog closes', async () => {
    vi.mocked(tauriInvoke).mockImplementation(async (command) => {
      if (command === 'get_plugin_strategy_access') return accessFixture()
      if (command === 'list_plugin_strategies') return { schemaVersion: 1, runs: [runFixture()] }
      throw new Error(`unexpected command: ${command}`)
    })
    const Host = defineComponent({
      components: { PluginStrategyDialog, PluginStrategyMonitor },
      setup: () => ({ open: ref(true), intent: intent() }),
      template: `
        <PluginStrategyDialog v-if="open" :intent="intent" @close="open = false" />
        <PluginStrategyMonitor :strategy-intents="[]" />
      `,
    })
    const wrapper = mount(Host)
    await flushPromises()
    expect(wrapper.find('[data-testid="strategy-dialog"]').exists()).toBe(true)
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).exists()).toBe(true)

    await wrapper.getComponent(PluginStrategyDialog).get('footer button').trigger('click')
    expect(wrapper.find('[data-testid="strategy-dialog"]').exists()).toBe(false)
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('running')
    expect(vi.mocked(tauriInvoke).mock.calls.some(([command]) => command === 'control_plugin_strategy'))
      .toBe(false)
  })

  it('does not claim pause or stop succeeded after IPC failure and refreshes authoritative state', async () => {
    vi.mocked(tauriInvoke)
      .mockResolvedValueOnce({ schemaVersion: 1, runs: [runFixture()] })
      .mockRejectedValueOnce(new Error('transport lost'))
      .mockResolvedValueOnce({ schemaVersion: 1, runs: [runFixture()] })
    const wrapper = mount(PluginStrategyMonitor, { props: { strategyIntents: [] } })
    await flushPromises()

    await wrapper.get('[data-testid="strategy-pause"]').trigger('click')
    await flushPromises()
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('running')
    expect(wrapper.get('[data-testid="strategy-monitor-error"]').text()).toContain('重新读取')
    expect(vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)).toEqual([
      'list_plugin_strategies', 'control_plugin_strategy', 'list_plugin_strategies',
    ])
  })

  it('offers reconcile only for recoveryRequired and never resumes automatically', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce({
      schemaVersion: 1, runs: [runFixture('recoveryRequired')],
    })
    const wrapper = mount(PluginStrategyMonitor, { props: { strategyIntents: [] } })
    await flushPromises()

    expect(wrapper.get('[data-testid="strategy-reconcile"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="strategy-resume"]').exists()).toBe(false)
    expect(vi.mocked(tauriInvoke).mock.calls.some(([command]) => command === 'start_plugin_strategy'))
      .toBe(false)
  })

  it('resumes only after fresh access and explicit consent with unchanged policy and counters', async () => {
    vi.mocked(tauriInvoke)
      .mockResolvedValueOnce({ schemaVersion: 1, runs: [runFixture('paused')] })
      .mockResolvedValueOnce(accessFixture())
      .mockResolvedValueOnce(runFixture('running'))
    const wrapper = mount(PluginStrategyMonitor, { props: { strategyIntents: [intent()] } })
    await flushPromises()

    const resume = wrapper.get<HTMLButtonElement>('[data-testid="strategy-resume"]')
    expect(resume.attributes('disabled')).toBeDefined()
    expect(vi.mocked(tauriInvoke).mock.calls.some(([command]) => command === 'get_plugin_strategy_access'))
      .toBe(false)
    await wrapper.get('[data-testid="strategy-resume-ack"]').setValue(true)
    await resume.trigger('click')
    await flushPromises()

    expect(vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)).toEqual([
      'list_plugin_strategies', 'get_plugin_strategy_access', 'start_plugin_strategy',
    ])
    expect(vi.mocked(tauriInvoke).mock.calls[2][1]).toMatchObject({
      request: {
        resumeRunId: RUN_ID,
        policy: {
          intervalMs: 5000, maxOrderQty: '0.001', maxTotalQty: '0.01',
          maxActions: 20, maxRunSeconds: 3600, reduceOnly: false,
        },
      },
    })
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('1 / 20')
  })

  it('lets emergency stop preempt a resume waiting for fresh access', async () => {
    const access = deferred<ReturnType<typeof accessFixture>>()
    vi.mocked(tauriInvoke).mockImplementation(async (command) => {
      if (command === 'list_plugin_strategies') {
        return { schemaVersion: 1, runs: [runFixture('paused')] }
      }
      if (command === 'get_plugin_strategy_access') return access.promise
      if (command === 'stop_all_plugin_strategies') {
        return { schemaVersion: 1, runs: [runFixture('stopping')] }
      }
      if (command === 'start_plugin_strategy') return runFixture('running')
      throw new Error(`unexpected command: ${command}`)
    })
    const wrapper = mount(PluginStrategyMonitor, { props: { strategyIntents: [intent()] } })
    await flushPromises()

    await wrapper.get('[data-testid="strategy-resume-ack"]').setValue(true)
    await wrapper.get('[data-testid="strategy-resume"]').trigger('click')
    await flushPromises()
    const stopAll = wrapper.get('[data-testid="strategy-stop-all"]')
    expect(stopAll.attributes('disabled')).toBeUndefined()
    await stopAll.trigger('click')
    await flushPromises()
    expect(vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)).toContain('stop_all_plugin_strategies')

    access.resolve(accessFixture())
    await flushPromises()
    expect(vi.mocked(tauriInvoke).mock.calls.some(([command]) => command === 'start_plugin_strategy'))
      .toBe(false)
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('stopping')
  })

  it('ignores an already-issued resume completion after emergency stop', async () => {
    const start = deferred<ReturnType<typeof runFixture>>()
    vi.mocked(tauriInvoke).mockImplementation(async (command) => {
      if (command === 'list_plugin_strategies') {
        return { schemaVersion: 1, runs: [runFixture('paused')] }
      }
      if (command === 'get_plugin_strategy_access') return accessFixture()
      if (command === 'start_plugin_strategy') return start.promise
      if (command === 'stop_all_plugin_strategies') {
        return { schemaVersion: 1, runs: [runFixture('stopping')] }
      }
      throw new Error(`unexpected command: ${command}`)
    })
    const wrapper = mount(PluginStrategyMonitor, { props: { strategyIntents: [intent()] } })
    await flushPromises()

    await wrapper.get('[data-testid="strategy-resume-ack"]').setValue(true)
    await wrapper.get('[data-testid="strategy-resume"]').trigger('click')
    await flushPromises()
    expect(vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)).toContain('start_plugin_strategy')

    await wrapper.get('[data-testid="strategy-stop-all"]').trigger('click')
    await flushPromises()
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('stopping')

    start.resolve(runFixture('running', { sequence: '5' }))
    await flushPromises()
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('stopping')
  })

  it('lets emergency stop preempt reconciliation and ignores its stale completion', async () => {
    const reconciliation = deferred<ReturnType<typeof runFixture>>()
    vi.mocked(tauriInvoke).mockImplementation(async (command) => {
      if (command === 'list_plugin_strategies') {
        return { schemaVersion: 1, runs: [runFixture('recoveryRequired')] }
      }
      if (command === 'reconcile_plugin_strategy') return reconciliation.promise
      if (command === 'stop_all_plugin_strategies') {
        return { schemaVersion: 1, runs: [runFixture('stopping')] }
      }
      throw new Error(`unexpected command: ${command}`)
    })
    const wrapper = mount(PluginStrategyMonitor, { props: { strategyIntents: [] } })
    await flushPromises()

    await wrapper.get('[data-testid="strategy-reconcile"]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-testid="strategy-stop-all"]').trigger('click')
    await flushPromises()
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('stopping')

    reconciliation.resolve(runFixture('paused'))
    await flushPromises()
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('stopping')
  })

  it('releases a stale list flight after an intent change and permits the next refresh', async () => {
    const firstList = deferred<{ schemaVersion: number; runs: ReturnType<typeof runFixture>[] }>()
    vi.mocked(tauriInvoke)
      .mockImplementationOnce(() => firstList.promise)
      .mockResolvedValueOnce({ schemaVersion: 1, runs: [runFixture()] })
    const wrapper = mount(PluginStrategyMonitor, { props: { strategyIntents: [] } })
    await flushPromises()

    await wrapper.setProps({ strategyIntents: [intent()] })
    firstList.resolve({ schemaVersion: 1, runs: [] })
    await flushPromises()

    const refresh = wrapper.get('button.ef-btn-secondary')
    expect(refresh.attributes('disabled')).toBeUndefined()
    await refresh.trigger('click')
    await flushPromises()
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('running')
  })

  it('rejects a reset resume response and refreshes the authoritative paused run', async () => {
    const paused = runFixture('paused', {
      sequence: '4', actionsSubmitted: 2, totalSubmittedQty: '0.002',
    })
    vi.mocked(tauriInvoke)
      .mockResolvedValueOnce({ schemaVersion: 1, runs: [paused] })
      .mockResolvedValueOnce(accessFixture())
      .mockResolvedValueOnce(runFixture('running', {
        sequence: '0', actionsSubmitted: 0, totalSubmittedQty: '0',
      }))
      .mockResolvedValueOnce({ schemaVersion: 1, runs: [paused] })
    const wrapper = mount(PluginStrategyMonitor, { props: { strategyIntents: [intent()] } })
    await flushPromises()

    await wrapper.get('[data-testid="strategy-resume-ack"]').setValue(true)
    await wrapper.get('[data-testid="strategy-resume"]').trigger('click')
    await flushPromises()

    expect(vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)).toEqual([
      'list_plugin_strategies', 'get_plugin_strategy_access',
      'start_plugin_strategy', 'list_plugin_strategies',
    ])
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('paused')
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('2 / 20')
  })

  it('keeps emergency stop available with a disconnected account and removed plugin', async () => {
    vi.mocked(tauriInvoke)
      .mockResolvedValueOnce({ schemaVersion: 1, runs: [runFixture()] })
      .mockResolvedValueOnce({ schemaVersion: 1, runs: [runFixture('stopping')] })
    const wrapper = mount(PluginStrategyMonitor, { props: { strategyIntents: [] } })
    await flushPromises()

    const stopAll = wrapper.get('[data-testid="strategy-stop-all"]')
    expect(stopAll.attributes('disabled')).toBeUndefined()
    await stopAll.trigger('click')
    await flushPromises()
    expect(tauriInvoke).toHaveBeenLastCalledWith('stop_all_plugin_strategies')
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('stopping')
  })

  it('stays mounted and loads runs when the plugin catalog itself fails', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(tauriInvoke).mockImplementation(async (command) => {
      if (command === 'get_plugin_catalog') throw new Error('catalog unavailable')
      if (command === 'list_plugin_strategies') return { schemaVersion: 1, runs: [runFixture()] }
      throw new Error(`unexpected command: ${command}`)
    })
    const wrapper = mount(PluginMarketplacePage, {
      props: { section: 'installed' },
      global: { plugins: [pinia] },
    })
    await flushPromises()

    expect(wrapper.get('[data-testid="plugin-strategy-monitor"]').exists()).toBe(true)
    expect(wrapper.get(`[data-run-id="${RUN_ID}"]`).text()).toContain('running')
    expect(wrapper.get('[data-testid="plugin-load-error"]').exists()).toBe(true)
  })
})
