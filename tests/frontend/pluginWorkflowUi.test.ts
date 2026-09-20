import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import PluginCommands from '../../src/components/plugins/PluginCommands.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useConnectionStore } from '../../src/stores/connection'
import { usePluginStore } from '../../src/stores/plugin'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T) => void
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((next) => { resolve = next })
  return { promise, resolve }
}

function catalog(generation = '8') {
  return {
    schemaVersion: 3, revision: '13', catalogGeneration: generation,
    availability: 'available', availabilityReasonCode: null,
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
    managedOwnership: {
      status: 'available', conflictingEntryCount: 0,
      rollbackPendingCount: 0, cleanupPendingCount: 0,
    },
    plugins: [{
      manifest: {
        schemaVersion: 5,
        id: 'com.example.trader', publisherId: 'com.example', publisher: 'Example',
        name: 'Example trader', description: 'Account workflow', version: '1.0.0',
        requestedCapabilities: ['account.read', 'balances.read', 'market.read', 'trade.place'],
        contributions: [{
          kind: 'command', contributionId: 'trader.prepare', title: 'Prepare order',
          actionId: 'sandbox.accountWorkflow',
          params: {
            runtime: 'wasm-v1', abi: 'account-json-v1', moduleBase64: 'AGFzbQEAAAA=',
            defaultInput: '{"qty":"0.001"}',
          },
        }],
      },
      source: 'localDeclarative', management: 'external', canRemove: false,
      toggleBlockReasonCode: null, status: 'enabled', statusReasonCode: null,
      canToggle: true, grantedCapabilities: [],
    }],
  }
}

function access(grantedCapabilities: string[] = [], grantRevision = '0') {
  return {
    schemaVersion: 1, pluginId: 'com.example.trader', contributionId: 'trader.prepare',
    catalogGeneration: '8', revision: '13',
    account: { accountId: 'paper-main', sessionEpoch: '21', environment: 'Testnet' },
    requestedCapabilities: ['account.read', 'balances.read', 'market.read', 'trade.place'],
    grantedCapabilities, grantRevision,
  }
}

function workflowResult(requestId: string) {
  return {
    schemaVersion: 1, requestId,
    pluginId: 'com.example.trader', contributionId: 'trader.prepare',
    catalogGeneration: '8', revision: '13',
    account: { accountId: 'paper-main', sessionEpoch: '21', environment: 'Testnet' },
    grantRevision: '1',
    snapshot: {
      schemaVersion: 1,
      account: { accountId: 'paper-main', sessionEpoch: '21', environment: 'Testnet' },
      capturedAtMs: '1789920000123', symbol: 'BTCUSDT',
      grantedCapabilities: ['account.read', 'balances.read', 'market.read', 'trade.place'],
      balances: {
        items: [{ asset: 'USDT', available: '100.25', frozen: '0', total: '100.25' }],
        fetchedAtMs: '1789920000100', partial: true,
      },
      positions: null, orders: null,
      market: {
        ticker: {
          symbol: 'BTCUSDT', lastPrice: '50000', bidPrice: '49999',
          askPrice: '50001', markPrice: '50000.5',
        },
        fetchedAtMs: '1789920000110',
      },
    },
    output: {
      kind: 'placeOrder',
      order: {
        symbol: 'BTCUSDT', side: 'Buy', orderType: 'Limit', qty: '0.001',
        price: '50000', timeInForce: 'GTC', positionIdx: 1, reduceOnly: false,
      },
    },
    confirmation: {
      token: 'fixed-test-token', expiresAtMs: '1789920060123',
      submissionId: '00000000-0000-4000-8000-000000000001',
    },
  }
}

function displayResult(requestId: string) {
  return {
    ...workflowResult(requestId),
    snapshot: {
      ...workflowResult(requestId).snapshot,
      grantedCapabilities: ['account.read'],
      balances: null,
      market: null,
    },
    output: { kind: 'display', text: '<b>Default input completed</b>' },
    confirmation: null,
  }
}

async function mountWorkflow() {
  const pinia = createPinia()
  setActivePinia(pinia)
  vi.mocked(tauriInvoke).mockResolvedValueOnce(catalog())
  const store = usePluginStore()
  await store.load()
  const wrapper = mount(PluginCommands, {
    props: { plugin: store.catalog[0] },
    global: { plugins: [pinia] },
  })
  return { wrapper, store }
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.spyOn(globalThis.crypto, 'randomUUID').mockReturnValue(
    '00000000-0000-4000-8000-000000000099',
  )
})

describe('plugin account workflow dialog', () => {
  it('loads access without credentials, keeps grants separate, and never trades during run', async () => {
    const { wrapper, store } = await mountWorkflow()
    expect(store.runCommand('com.example.trader', 'trader.prepare')?.actionId)
      .toBe('sandbox.accountWorkflow')
    vi.mocked(tauriInvoke).mockResolvedValueOnce(access())
    await wrapper.get('[data-testid="plugin-command-button"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="plugin-workflow-dialog"]').text()).toContain('paper-main')
    expect(wrapper.get('[data-testid="plugin-workflow-dialog"]').text()).toContain('Testnet')
    expect(wrapper.findAll<HTMLInputElement>('[data-testid="workflow-capability"]')
      .every((checkbox) => !checkbox.element.checked)).toBe(true)
    expect(vi.mocked(tauriInvoke).mock.calls.map(([name]) => name)).toEqual([
      'get_plugin_catalog', 'get_plugin_workflow_access',
    ])

    const capabilities = wrapper.findAll<HTMLInputElement>('[data-testid="workflow-capability"]')
    await capabilities[0].setValue(true)
    await capabilities[1].setValue(true)
    await capabilities[2].setValue(true)
    await capabilities[3].setValue(true)
    vi.mocked(tauriInvoke).mockResolvedValueOnce(access(
      ['account.read', 'balances.read', 'market.read', 'trade.place'], '1',
    ))
    await wrapper.get('[data-testid="workflow-save-grants"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="workflow-snapshot"]').exists()).toBe(false)

    vi.mocked(tauriInvoke).mockImplementationOnce((_name, args) => {
      const requestId = (args as { request: { requestId: string } }).request.requestId
      return Promise.resolve(workflowResult(requestId))
    })
    await wrapper.get('[data-testid="workflow-symbol"]').setValue('BTCUSDT')
    await wrapper.get('[data-testid="workflow-run"]').trigger('click')
    await flushPromises()

    const invokeCalls = vi.mocked(tauriInvoke).mock.calls
    expect(invokeCalls.filter(([name]) => name === 'confirm_plugin_workflow')).toHaveLength(0)
    expect(wrapper.get('[data-testid="workflow-snapshot"]').text()).toContain('100.25')
    expect(wrapper.get('[data-testid="workflow-snapshot"]').text()).toContain('快照不会自动刷新')
    expect(wrapper.get('[data-testid="workflow-snapshot"]').text()).not.toContain('30 秒内获取')
    expect(wrapper.get('[data-testid="workflow-partial-warning"]').text()).toContain('可能不完整')
    expect(wrapper.get('[data-testid="workflow-confirmation"]').text()).toContain('真实订单')
    expect(wrapper.get('[data-testid="workflow-confirmation"]').text()).toContain('0.001')
  })

  it('runs the visible default JSON with only the requested account authority', async () => {
    const { wrapper } = await mountWorkflow()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(access(['account.read'], '1'))
    await wrapper.get('[data-testid="plugin-command-button"]').trigger('click')
    await flushPromises()

    expect(wrapper.get<HTMLTextAreaElement>('[data-testid="workflow-input"]').element.value)
      .toBe('{"qty":"0.001"}')
    vi.mocked(tauriInvoke).mockImplementationOnce((_name, args) => Promise.resolve(
      displayResult((args as { request: { requestId: string } }).request.requestId),
    ))
    await wrapper.get('[data-testid="workflow-run"]').trigger('click')
    await flushPromises()

    const snapshot = wrapper.get('[data-testid="workflow-snapshot"]')
    expect(snapshot.text()).toContain('<b>Default input completed</b>')
    expect(snapshot.find('b').exists()).toBe(false)
    expect(snapshot.text()).toContain('未授权，未读取余额')
    expect(wrapper.find('[data-testid="workflow-confirmation"]').exists()).toBe(false)
  })

  it('sends only the immutable token on explicit confirm and blocks unknown-outcome replay', async () => {
    const { wrapper } = await mountWorkflow()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(access(
      ['account.read', 'balances.read', 'market.read', 'trade.place'], '1',
    ))
    await wrapper.get('[data-testid="plugin-command-button"]').trigger('click')
    await flushPromises()
    vi.mocked(tauriInvoke).mockImplementationOnce((_name, args) => Promise.resolve(
      workflowResult((args as { request: { requestId: string } }).request.requestId),
    ))
    await wrapper.get('[data-testid="workflow-symbol"]').setValue('BTCUSDT')
    await wrapper.get('[data-testid="workflow-run"]').trigger('click')
    await flushPromises()

    const unknown = {
      schemaVersion: 1, token: 'fixed-test-token',
      account: { accountId: 'paper-main', sessionEpoch: '21', environment: 'Testnet' },
      action: 'placeOrder', status: 'unknown',
      submissionId: '00000000-0000-4000-8000-000000000001',
      order: null, errorCode: 'plugin_workflow_unknown',
    }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(unknown)
    await wrapper.get('[data-testid="workflow-confirm"]').trigger('click')
    await flushPromises()

    const confirmCalls = vi.mocked(tauriInvoke).mock.calls
      .filter(([name]) => name === 'confirm_plugin_workflow')
    expect(confirmCalls).toHaveLength(1)
    expect(confirmCalls[0]?.[1]).toEqual({ token: 'fixed-test-token' })
    expect(wrapper.get('[data-testid="workflow-receipt-unknown"]').text()).toContain('请前往交易页')
    expect(wrapper.find('[data-testid="workflow-confirm"]').exists()).toBe(false)
  })

  it('treats a dropped confirmation response as unknown and never offers token replay', async () => {
    const { wrapper } = await mountWorkflow()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(access(
      ['account.read', 'balances.read', 'market.read', 'trade.place'], '1',
    ))
    await wrapper.get('[data-testid="plugin-command-button"]').trigger('click')
    await flushPromises()
    vi.mocked(tauriInvoke).mockImplementationOnce((_name, args) => Promise.resolve(
      workflowResult((args as { request: { requestId: string } }).request.requestId),
    ))
    await wrapper.get('[data-testid="workflow-symbol"]').setValue('BTCUSDT')
    await wrapper.get('[data-testid="workflow-run"]').trigger('click')
    await flushPromises()

    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('transport dropped after dispatch'))
    const detachedConfirmButton = wrapper.get('[data-testid="workflow-confirm"]')
    await detachedConfirmButton.trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="workflow-receipt-unknown"]').text())
      .toContain('不要重新提交')
    expect(wrapper.find('[data-testid="workflow-confirm"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="workflow-error"]').exists()).toBe(false)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([name]) => name === 'confirm_plugin_workflow')).toHaveLength(1)

    await detachedConfirmButton.trigger('click')
    await flushPromises()
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([name]) => name === 'confirm_plugin_workflow')).toHaveLength(1)
  })

  it('clears prepared confirmation on input change and suppresses late runs after account change', async () => {
    const { wrapper } = await mountWorkflow()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(access(
      ['account.read', 'balances.read', 'market.read', 'trade.place'], '1',
    ))
    await wrapper.get('[data-testid="plugin-command-button"]').trigger('click')
    await flushPromises()
    vi.mocked(tauriInvoke).mockImplementationOnce((_name, args) => Promise.resolve(
      workflowResult((args as { request: { requestId: string } }).request.requestId),
    ))
    await wrapper.get('[data-testid="workflow-symbol"]').setValue('BTCUSDT')
    await wrapper.get('[data-testid="workflow-run"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="workflow-confirmation"]').exists()).toBe(true)
    await wrapper.get('[data-testid="workflow-input"]').setValue('{"qty":"0.002"}')
    expect(wrapper.find('[data-testid="workflow-confirmation"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="workflow-snapshot"]').exists()).toBe(false)

    const late = deferred<ReturnType<typeof workflowResult>>()
    vi.mocked(tauriInvoke).mockReturnValueOnce(late.promise)
    await wrapper.get('[data-testid="workflow-run"]').trigger('click')
    useAccountProfilesStore().adoptSessionEpoch(22)
    useConnectionStore().setStatus('disconnected')
    await nextTick()
    late.resolve(workflowResult('workflow-00000000-0000-4000-8000-000000000099'))
    await flushPromises()
    expect(wrapper.find('[data-testid="workflow-snapshot"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="workflow-confirmation"]').exists()).toBe(false)
  })
})
