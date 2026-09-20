import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { getPluginCatalog } from '../../src/services/pluginService'
import {
  confirmPluginWorkflow,
  getPluginWorkflowAccess,
  parseWorkflowAccess,
  runPluginWorkflow,
  setPluginWorkflowGrants,
} from '../../src/services/pluginWorkflowService'
import type { PluginWorkflowExecutionIntent } from '../../src/types/plugin'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

const INVALID_RESPONSE_ERROR = '插件工作流服务返回的数据无效，请重试。'

function intent(): PluginWorkflowExecutionIntent {
  return {
    actionId: 'sandbox.accountWorkflow',
    pluginId: 'com.example.trader',
    pluginName: 'Example trader',
    contributionId: 'trader.prepare',
    title: 'Prepare order',
    runtime: 'wasm-v1',
    abi: 'account-json-v1',
    defaultInput: '{"qty":"0.001"}',
    requestedCapabilities: ['account.read', 'balances.read', 'market.read', 'trade.place'],
    expectedCatalogGeneration: '8',
    expectedRevision: '13',
  }
}

function accessFixture() {
  return {
    schemaVersion: 1,
    pluginId: 'com.example.trader',
    contributionId: 'trader.prepare',
    catalogGeneration: '8',
    revision: '13',
    account: { accountId: 'paper-main', sessionEpoch: '21', environment: 'Testnet' },
    requestedCapabilities: ['account.read', 'balances.read', 'market.read', 'trade.place'],
    grantedCapabilities: [] as string[],
    grantRevision: '0',
  }
}

function resultFixture() {
  return {
    schemaVersion: 1,
    requestId: 'workflow-fixed-request',
    pluginId: 'com.example.trader',
    contributionId: 'trader.prepare',
    catalogGeneration: '8',
    revision: '13',
    account: { accountId: 'paper-main', sessionEpoch: '21', environment: 'Testnet' },
    grantRevision: '1',
    snapshot: {
      schemaVersion: 1,
      account: { accountId: 'paper-main', sessionEpoch: '21', environment: 'Testnet' },
      capturedAtMs: '1789920000123',
      symbol: 'BTCUSDT',
      grantedCapabilities: ['account.read', 'balances.read', 'market.read', 'trade.place'],
      balances: {
        items: [{ asset: 'USDT', available: '100.25', frozen: '0', total: '100.25' }],
        fetchedAtMs: '1789920000100', partial: true,
      },
      positions: null,
      orders: null,
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
      token: 'fixed-test-token',
      expiresAtMs: '1789920060123',
      submissionId: '00000000-0000-4000-8000-000000000001',
    },
  }
}

function catalogFixture(schemaVersion: 4 | 5 = 5) {
  const workflow = {
    kind: 'command', contributionId: 'trader.prepare', title: 'Prepare order',
    actionId: 'sandbox.accountWorkflow',
    params: {
      runtime: 'wasm-v1', abi: 'account-json-v1', moduleBase64: 'AGFzbQEAAAA=',
      defaultInput: '{"qty":"0.001"}',
    },
  }
  return {
    schemaVersion: 3,
    revision: '13', catalogGeneration: '8', availability: 'available',
    availabilityReasonCode: null,
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
    managedOwnership: {
      status: 'available', conflictingEntryCount: 0,
      rollbackPendingCount: 0, cleanupPendingCount: 0,
    },
    plugins: [{
      manifest: {
        schemaVersion,
        id: 'com.example.trader', publisherId: 'com.example', publisher: 'Example',
        name: 'Example trader', description: 'Account workflow', version: '1.0.0',
        requestedCapabilities: schemaVersion === 5
          ? ['account.read', 'balances.read', 'market.read', 'trade.place']
          : [],
        contributions: [workflow],
      },
      source: 'localDeclarative', management: 'external', canRemove: false,
      toggleBlockReasonCode: null, status: 'enabled', statusReasonCode: null,
      canToggle: true, grantedCapabilities: [],
    }],
  }
}

describe('plugin workflow catalog parsing', () => {
  it('preserves v5 requested capabilities and deep clones workflow params and arrays', async () => {
    const wire = catalogFixture()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(wire)

    const parsed = await getPluginCatalog()
    const manifest = parsed.plugins[0].manifest
    expect(manifest.schemaVersion).toBe(5)
    if (manifest.schemaVersion !== 5) throw new Error('expected v5')
    expect(manifest.requestedCapabilities).toEqual([
      'account.read', 'balances.read', 'market.read', 'trade.place',
    ])
    expect(manifest.contributions[0]).toMatchObject({
      actionId: 'sandbox.accountWorkflow',
      params: { defaultInput: '{"qty":"0.001"}' },
    })

    const wireManifest = wire.plugins[0].manifest
    wireManifest.requestedCapabilities[0] = 'trade.cancel'
    ;(wireManifest.contributions[0].params as { defaultInput: string }).defaultInput = '{}'
    expect(manifest.requestedCapabilities[0]).toBe('account.read')
    expect(manifest.contributions[0].params.defaultInput).toBe('{"qty":"0.001"}')
  })

  it('rejects the account workflow action in manifest v4', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(catalogFixture(4))
    await expect(getPluginCatalog()).rejects.toThrow('插件服务返回的数据无效')
  })
})

describe('plugin workflow strict wire parsing', () => {
  beforeEach(() => vi.resetAllMocks())

  it('accepts ungranted correlated access and rejects stale or corrupt authority', () => {
    expect(parseWorkflowAccess(accessFixture(), intent()).grantedCapabilities).toEqual([])
    expect(() => parseWorkflowAccess({ ...accessFixture(), pluginId: 'com.example.wrong' }, intent()))
      .toThrow(INVALID_RESPONSE_ERROR)
    expect(() => parseWorkflowAccess({ ...accessFixture(), catalogGeneration: '08' }, intent()))
      .toThrow(INVALID_RESPONSE_ERROR)
    expect(() => parseWorkflowAccess({ ...accessFixture(), extra: true }, intent()))
      .toThrow(INVALID_RESPONSE_ERROR)
    expect(() => parseWorkflowAccess({
      ...accessFixture(), grantedCapabilities: ['trade.place'],
    }, intent())).toThrow(INVALID_RESPONSE_ERROR)
  })

  it('builds allowlisted authority and grant requests without spreading caller data', async () => {
    const current = accessFixture()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(current)
    await getPluginWorkflowAccess(intent())
    expect(tauriInvoke).toHaveBeenLastCalledWith('get_plugin_workflow_access', {
      request: {
        pluginId: 'com.example.trader', contributionId: 'trader.prepare',
        expectedCatalogGeneration: '8', expectedRevision: '13',
      },
    })

    const granted = {
      ...current,
      grantedCapabilities: ['account.read', 'balances.read', 'market.read', 'trade.place'],
      grantRevision: '1',
    }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(granted)
    await setPluginWorkflowGrants(intent(), parseWorkflowAccess(current, intent()), [
      'account.read', 'balances.read', 'market.read', 'trade.place',
    ])
    expect(tauriInvoke).toHaveBeenLastCalledWith('set_plugin_workflow_grants', {
      request: {
        pluginId: 'com.example.trader', contributionId: 'trader.prepare',
        expectedCatalogGeneration: '8', expectedRevision: '13',
        expectedAccountId: 'paper-main', expectedSessionEpoch: '21',
        expectedGrantRevision: '0',
        capabilities: ['account.read', 'balances.read', 'market.read', 'trade.place'],
      },
    })
  })

  it('accepts a requested subset with account authority and rejects a subset missing it', async () => {
    const current = parseWorkflowAccess(accessFixture(), intent())
    vi.mocked(tauriInvoke).mockResolvedValueOnce({
      ...accessFixture(), grantedCapabilities: ['account.read'], grantRevision: '1',
    })
    const updated = await setPluginWorkflowGrants(intent(), current, ['account.read'])
    expect(updated.grantedCapabilities).toEqual(['account.read'])
    expect(tauriInvoke).toHaveBeenCalledTimes(1)

    await expect(setPluginWorkflowGrants(intent(), current, ['balances.read']))
      .rejects.toThrow(INVALID_RESPONSE_ERROR)
    expect(tauriInvoke).toHaveBeenCalledTimes(1)
  })

  it('correlates result identity, account/session/grant revision, symbols and section authority', async () => {
    const access = parseWorkflowAccess({
      ...accessFixture(),
      grantedCapabilities: ['account.read', 'balances.read', 'market.read', 'trade.place'],
      grantRevision: '1',
    }, intent())
    vi.mocked(tauriInvoke).mockResolvedValueOnce(resultFixture())
    const result = await runPluginWorkflow(intent(), access, {
      requestId: 'workflow-fixed-request', symbol: 'BTCUSDT', inputJson: '{"qty":"0.001"}',
    })
    expect(result.snapshot.balances?.items[0].available).toBe('100.25')
    expect(result.output.kind).toBe('placeOrder')
    expect(tauriInvoke).toHaveBeenLastCalledWith('run_plugin_workflow', {
      request: {
        pluginId: 'com.example.trader', contributionId: 'trader.prepare',
        expectedCatalogGeneration: '8', expectedRevision: '13',
        requestId: 'workflow-fixed-request', expectedAccountId: 'paper-main',
        expectedSessionEpoch: '21', expectedGrantRevision: '1',
        symbol: 'BTCUSDT', inputJson: '{"qty":"0.001"}',
      },
    })

    for (const corrupt of [
      { ...resultFixture(), requestId: 'workflow-other-request' },
      { ...resultFixture(), grantRevision: '2' },
      {
        ...resultFixture(),
        account: { accountId: 'paper-other', sessionEpoch: '21', environment: 'Testnet' },
      },
      {
        ...resultFixture(),
        snapshot: { ...resultFixture().snapshot, positions: { items: [], fetchedAtMs: '1', partial: true } },
      },
      {
        ...resultFixture(),
        snapshot: {
          ...resultFixture().snapshot,
          market: {
            ticker: {
              ...resultFixture().snapshot.market!.ticker,
              symbol: 'ETHUSDT',
            },
            fetchedAtMs: '1789920000110',
          },
        },
      },
      {
        ...resultFixture(),
        output: {
          kind: 'placeOrder',
          order: { ...resultFixture().output.order, positionIdx: 0 },
        },
      },
      {
        ...resultFixture(),
        output: {
          kind: 'placeOrder',
          order: { ...resultFixture().output.order, positionIdx: 2 },
        },
      },
      {
        ...resultFixture(),
        snapshot: {
          ...resultFixture().snapshot,
          balances: {
            ...resultFixture().snapshot.balances,
            items: [{ asset: 'usdt', available: '100.25', frozen: '0', total: '100.25' }],
          },
        },
      },
    ]) {
      vi.mocked(tauriInvoke).mockResolvedValueOnce(corrupt)
      await expect(runPluginWorkflow(intent(), access, {
        requestId: 'workflow-fixed-request', symbol: 'BTCUSDT', inputJson: '{"qty":"0.001"}',
      })).rejects.toThrow(INVALID_RESPONSE_ERROR)
    }
  })

  it('preserves valid plain-text output including whitespace and line breaks', async () => {
    const access = parseWorkflowAccess({
      ...accessFixture(), grantedCapabilities: ['account.read'], grantRevision: '1',
    }, intent())
    const display = {
      ...resultFixture(),
      snapshot: {
        ...resultFixture().snapshot,
        grantedCapabilities: ['account.read'], balances: null, market: null,
      },
      output: { kind: 'display', text: '  first line\nsecond line  ' },
      confirmation: null,
    }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(display)
    const parsed = await runPluginWorkflow(intent(), access, {
      requestId: 'workflow-fixed-request', symbol: 'BTCUSDT', inputJson: '{}',
    })
    expect(parsed.output).toEqual({ kind: 'display', text: '  first line\nsecond line  ' })
  })

  it('confirms with the token only and rejects a mismatched receipt', async () => {
    const result = resultFixture()
    const accepted = {
      schemaVersion: 1,
      token: 'fixed-test-token',
      account: { accountId: 'paper-main', sessionEpoch: '21', environment: 'Testnet' },
      action: 'placeOrder', status: 'accepted',
      submissionId: '00000000-0000-4000-8000-000000000001',
      order: {
        orderId: 'exchange-order-1', symbol: 'BTCUSDT', side: 'Buy', orderType: 'Limit',
        price: '50000', qty: '0.001', status: 'New', orderLinkId: null,
        filledQty: '0', avgPrice: '0',
      },
      errorCode: null,
    }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(accepted)
    await confirmPluginWorkflow(result.confirmation!, result.output, result.account)
    expect(tauriInvoke).toHaveBeenLastCalledWith('confirm_plugin_workflow', {
      token: 'fixed-test-token',
    })

    vi.mocked(tauriInvoke).mockResolvedValueOnce({ ...accepted, token: 'other-token' })
    await expect(confirmPluginWorkflow(result.confirmation!, result.output, result.account))
      .rejects.toThrow(INVALID_RESPONSE_ERROR)

    for (const order of [
      { ...accepted.order, status: 'Created' },
      { ...accepted.order, side: 'Hold' },
      { ...accepted.order, orderType: 'Trigger' },
    ]) {
      vi.mocked(tauriInvoke).mockResolvedValueOnce({ ...accepted, order })
      await expect(confirmPluginWorkflow(result.confirmation!, result.output, result.account))
        .rejects.toThrow(INVALID_RESPONSE_ERROR)
    }
  })

  it('accepts a captured open-order cancellation and treats accepted as non-terminal', async () => {
    const cancelIntent: PluginWorkflowExecutionIntent = {
      ...intent(),
      requestedCapabilities: ['account.read', 'orders.read', 'trade.cancel'],
    }
    const cancelAccess = parseWorkflowAccess({
      ...accessFixture(),
      requestedCapabilities: ['account.read', 'orders.read', 'trade.cancel'],
      grantedCapabilities: ['account.read', 'orders.read', 'trade.cancel'],
      grantRevision: '4',
    }, cancelIntent)
    const cancelResult = {
      ...resultFixture(),
      grantRevision: '4',
      snapshot: {
        ...resultFixture().snapshot,
        grantedCapabilities: ['account.read', 'orders.read', 'trade.cancel'],
        balances: null,
        market: null,
        orders: {
          items: [{
            orderId: 'exchange-order-1', symbol: 'BTCUSDT', side: 'Buy',
            orderType: 'Limit', price: '50000', qty: '0.001', status: 'New',
            orderLinkId: null, filledQty: '0', avgPrice: '0',
          }],
          fetchedAtMs: '1789920000100', partial: true,
        },
      },
      output: {
        kind: 'cancelOrder',
        order: { symbol: 'BTCUSDT', orderId: 'exchange-order-1' },
      },
      confirmation: {
        token: 'fixed-cancel-token', expiresAtMs: '1789920060123', submissionId: null,
      },
    }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(cancelResult)
    const prepared = await runPluginWorkflow(cancelIntent, cancelAccess, {
      requestId: 'workflow-fixed-request', symbol: 'BTCUSDT', inputJson: '{}',
    })
    expect(prepared.output).toEqual({
      kind: 'cancelOrder', order: { symbol: 'BTCUSDT', orderId: 'exchange-order-1' },
    })

    vi.mocked(tauriInvoke).mockResolvedValueOnce({
      schemaVersion: 1, token: 'fixed-cancel-token',
      account: { accountId: 'paper-main', sessionEpoch: '21', environment: 'Testnet' },
      action: 'cancelOrder', status: 'accepted', submissionId: null,
      order: {
        orderId: 'exchange-order-1', symbol: 'BTCUSDT', side: 'Buy',
        orderType: 'Limit', price: '50000', qty: '0.001', status: 'Unknown',
        orderLinkId: null, filledQty: '0', avgPrice: '0',
      },
      errorCode: null,
    })
    const receipt = await confirmPluginWorkflow(
      prepared.confirmation!, prepared.output, prepared.account,
    )
    expect(receipt.status).toBe('accepted')
    expect(receipt.order?.status).toBe('Unknown')
    expect(tauriInvoke).toHaveBeenLastCalledWith('confirm_plugin_workflow', {
      token: 'fixed-cancel-token',
    })
  })
})
