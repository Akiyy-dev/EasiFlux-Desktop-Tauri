import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import {
  commitLocalManifestImport,
  getPluginCatalog,
  prepareLocalManifestImport,
} from '../../src/services/pluginService'
import {
  controlPluginStrategy,
  getPluginStrategyAccess,
  listPluginStrategies,
  parsePluginStrategyAccess,
  pluginStrategyErrorMessage,
  reconcilePluginStrategy,
  startPluginStrategy,
  stopAllPluginStrategies,
} from '../../src/services/pluginStrategyService'
import type { PluginStrategyExecutionIntent } from '../../src/types/plugin'
import type { StrategyPolicy, StrategyRunView } from '../../src/types/pluginStrategy'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

const INVALID_RESPONSE = '插件策略服务返回的数据无效，请重试。'
const REQUEST_ID = '00000000-0000-4000-8000-000000000001'
const RUN_ID = '00000000-0000-4000-8000-000000000002'

function intent(): PluginStrategyExecutionIntent {
  return {
    actionId: 'sandbox.strategy',
    pluginId: 'com.example.strategy',
    pluginName: 'Threshold strategy',
    contributionId: 'strategy.threshold',
    title: 'Threshold once',
    runtime: 'wasm-v1',
    abi: 'strategy-json-v1',
    defaultInput: '{"threshold":"50000"}',
    requestedCapabilities: ['account.read', 'market.read', 'trade.place', 'strategy.run'],
    expectedCatalogGeneration: '8',
    expectedRevision: '13',
  }
}

function accessFixture() {
  return {
    schemaVersion: 1,
    pluginId: 'com.example.strategy', contributionId: 'strategy.threshold',
    catalogGeneration: '8', revision: '13',
    account: { accountId: 'paper-main', sessionEpoch: '21', environment: 'Testnet' },
    requestedCapabilities: ['account.read', 'market.read', 'trade.place', 'strategy.run'],
    authorizationToken: 'strategy-ticket-1', expiresAtMs: '1789920060123',
  }
}

function policy(): StrategyPolicy {
  return {
    intervalMs: 5000, maxOrderQty: '0.001', maxTotalQty: '0.01',
    maxActions: 20, maxRunSeconds: 3600, reduceOnly: false,
  }
}

function runFixture(overrides: Record<string, unknown> = {}): StrategyRunView & Record<string, unknown> {
  return {
    schemaVersion: 1, runId: RUN_ID, requestId: REQUEST_ID,
    pluginId: 'com.example.strategy', contributionId: 'strategy.threshold',
    account: { accountId: 'paper-main', sessionEpoch: '21', environment: 'Testnet' },
    symbol: 'BTCUSDT', status: 'running', reason: null, policy: policy(),
    capabilities: ['account.read', 'market.read', 'trade.place', 'strategy.run'],
    inputJson: '{"threshold":"50000"}', startedAtMs: '1789920000123',
    expiresAtMs: '1789923600123', sequence: '0', actionsSubmitted: 0,
    totalSubmittedQty: '0', lastMessage: '', lastReceipt: null,
    ...overrides,
  } as StrategyRunView & Record<string, unknown>
}

function catalogFixture(schemaVersion: 5 | 6, capabilities: string[], action = 'sandbox.strategy', abi = 'strategy-json-v1') {
  return {
    schemaVersion: 3, revision: '13', catalogGeneration: '8', availability: 'available',
    availabilityReasonCode: null,
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
    managedOwnership: {
      status: 'available', conflictingEntryCount: 0,
      rollbackPendingCount: 0, cleanupPendingCount: 0,
    },
    plugins: [{
      manifest: {
        schemaVersion, id: 'com.example.strategy', publisherId: 'com.example',
        publisher: 'Example', name: 'Strategy', description: 'Automatic strategy',
        version: '1.0.0', requestedCapabilities: capabilities,
        contributions: [{
          kind: 'command', contributionId: 'strategy.threshold', title: 'Threshold once',
          actionId: action,
          params: {
            runtime: 'wasm-v1', abi, moduleBase64: 'AGFzbQEAAAA=',
            defaultInput: '{"threshold":"50000"}',
          },
        }],
      },
      source: 'localDeclarative', management: 'external', canRemove: false,
      toggleBlockReasonCode: null, status: 'enabled', statusReasonCode: null,
      canToggle: true, grantedCapabilities: [],
    }],
  }
}

describe('v6 strategy manifests', () => {
  beforeEach(() => vi.resetAllMocks())

  it('preserves the full v6 requested capability list and strategy ABI', async () => {
    const wire = catalogFixture(6, ['account.read', 'market.read', 'trade.place', 'strategy.run'])
    vi.mocked(tauriInvoke).mockResolvedValueOnce(wire)

    const parsed = await getPluginCatalog()
    const manifest = parsed.plugins[0].manifest
    expect(manifest.schemaVersion).toBe(6)
    if (manifest.schemaVersion !== 6) throw new Error('expected v6')
    expect(manifest.requestedCapabilities).toEqual([
      'account.read', 'market.read', 'trade.place', 'strategy.run',
    ])
    expect(manifest.contributions[0]).toMatchObject({
      actionId: 'sandbox.strategy', params: { abi: 'strategy-json-v1' },
    })
  })

  it('accepts the checked-in threshold strategy through the strict catalog parser', async () => {
    const manifest = JSON.parse(readFileSync(resolve(
      process.cwd(), 'examples/plugins/threshold-strategy/manifest.json',
    ), 'utf8')) as Record<string, unknown>
    const wire = catalogFixture(6, [])
    wire.plugins[0].manifest = manifest as typeof wire.plugins[0]['manifest']
    vi.mocked(tauriInvoke).mockResolvedValueOnce(wire)

    const parsed = await getPluginCatalog()
    expect(parsed.plugins[0].manifest).toMatchObject({
      schemaVersion: 6,
      id: 'com.easiflux.examples.threshold-strategy',
      requestedCapabilities: [
        'account.read', 'orders.read', 'market.read', 'trade.place', 'trade.cancel', 'strategy.run',
      ],
      contributions: [{
        actionId: 'sandbox.strategy',
        params: { runtime: 'wasm-v1', abi: 'strategy-json-v1' },
      }],
    })
  })

  it('binds import commit to the complete previewed v6 capability list', async () => {
    const manifest = JSON.parse(readFileSync(resolve(
      process.cwd(), 'examples/plugins/threshold-strategy/manifest.json',
    ), 'utf8')) as Record<string, unknown>
    vi.mocked(tauriInvoke).mockResolvedValueOnce({
      schemaVersion: 2, status: 'ready', token: 'a'.repeat(32), expiresInSeconds: 300,
      catalogGeneration: '8', manifest, assessment: { kind: 'notInCatalog' },
    })
    const preview = await prepareLocalManifestImport()
    if (preview.status !== 'ready') throw new Error('expected ready preview')

    const imported = (candidateManifest: Record<string, unknown>) => {
      const snapshot = catalogFixture(6, [])
      snapshot.plugins[0] = {
        ...snapshot.plugins[0],
        manifest: candidateManifest as typeof snapshot.plugins[0]['manifest'],
        management: 'managed', canRemove: true, status: 'disabled',
      }
      return {
        schemaVersion: 2, status: 'imported',
        pluginId: 'com.easiflux.examples.threshold-strategy', snapshot,
      }
    }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(imported(manifest))
    await expect(commitLocalManifestImport(preview)).resolves.toMatchObject({ status: 'imported' })

    vi.mocked(tauriInvoke).mockResolvedValueOnce(imported({
      ...manifest,
      requestedCapabilities: [
        'account.read', 'market.read', 'trade.place', 'trade.cancel', 'strategy.run',
      ],
    }))
    await expect(commitLocalManifestImport(preview)).rejects.toThrow('插件服务返回的数据无效')
  })

  it.each([
    ['copied v5 manifest with strategy.run', catalogFixture(5, ['account.read', 'strategy.run'], 'sandbox.accountWorkflow', 'account-json-v1')],
    ['strategy action on v5', catalogFixture(5, ['account.read'], 'sandbox.strategy')],
    ['workflow action on v6', catalogFixture(6, ['account.read', 'strategy.run'], 'sandbox.accountWorkflow', 'account-json-v1')],
    ['wrong strategy ABI', catalogFixture(6, ['account.read', 'strategy.run'], 'sandbox.strategy', 'account-json-v1')],
    ['missing strategy.run', catalogFixture(6, ['account.read'], 'sandbox.strategy')],
  ])('rejects %s', async (_name, wire) => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(wire)
    await expect(getPluginCatalog()).rejects.toThrow('插件服务返回的数据无效')
  })
})

describe('plugin strategy strict service boundary', () => {
  beforeEach(() => vi.resetAllMocks())

  it('parses an exact correlated access ticket and rejects drift or unknown fields', () => {
    expect(parsePluginStrategyAccess(accessFixture(), intent()).authorizationToken)
      .toBe('strategy-ticket-1')
    for (const invalid of [
      { ...accessFixture(), pluginId: 'com.example.other' },
      { ...accessFixture(), catalogGeneration: '08' },
      { ...accessFixture(), requestedCapabilities: ['account.read', 'strategy.run'] },
      { ...accessFixture(), extra: true },
    ]) {
      expect(() => parsePluginStrategyAccess(invalid, intent())).toThrow(INVALID_RESPONSE)
    }
  })

  it('uses the exact authority request and starts once with explicit authority and consent', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(accessFixture())
    const access = await getPluginStrategyAccess(intent())
    expect(tauriInvoke).toHaveBeenLastCalledWith('get_plugin_strategy_access', {
      request: {
        pluginId: 'com.example.strategy', contributionId: 'strategy.threshold',
        expectedCatalogGeneration: '8', expectedRevision: '13',
      },
    })

    vi.mocked(tauriInvoke).mockResolvedValueOnce(runFixture())
    const run = await startPluginStrategy(intent(), access, {
      requestId: REQUEST_ID, resumeRunId: null, symbol: 'BTCUSDT',
      inputJson: '{"threshold":"50000"}',
      capabilities: ['account.read', 'market.read', 'trade.place', 'strategy.run'],
      policy: policy(), acknowledgeAutomaticTrading: true,
    })
    expect(run.runId).toBe(RUN_ID)
    expect(tauriInvoke).toHaveBeenLastCalledWith('start_plugin_strategy', {
      request: {
        authorizationToken: 'strategy-ticket-1', requestId: REQUEST_ID,
        resumeRunId: null, symbol: 'BTCUSDT', inputJson: '{"threshold":"50000"}',
        capabilities: ['account.read', 'market.read', 'trade.place', 'strategy.run'],
        policy: policy(), acknowledgeAutomaticTrading: true,
      },
    })
  })

  it('rejects invalid policy without numeric conversion or an incomplete authority selection', async () => {
    const access = parsePluginStrategyAccess(accessFixture(), intent())
    const calls = vi.mocked(tauriInvoke).mock.calls.length
    for (const invalidPolicy of [
      { ...policy(), maxOrderQty: '0' },
      { ...policy(), maxOrderQty: '0.02' },
      { ...policy(), maxTotalQty: '1e999999999999999999999999999999999999999999999999999999999999' },
      { ...policy(), intervalMs: 4999 },
      { ...policy(), maxActions: 1001 },
      { ...policy(), maxRunSeconds: 59 },
    ]) {
      await expect(startPluginStrategy(intent(), access, {
        requestId: REQUEST_ID, resumeRunId: null, symbol: 'BTCUSDT', inputJson: '{}',
        capabilities: ['account.read', 'market.read', 'trade.place', 'strategy.run'],
        policy: invalidPolicy, acknowledgeAutomaticTrading: true,
      })).rejects.toThrow(INVALID_RESPONSE)
    }
    await expect(startPluginStrategy(intent(), access, {
      requestId: REQUEST_ID, resumeRunId: null, symbol: 'BTCUSDT', inputJson: '{}',
      capabilities: ['account.read', 'trade.place'], policy: policy(),
      acknowledgeAutomaticTrading: true,
    })).rejects.toThrow(INVALID_RESPONSE)
    expect(tauriInvoke).toHaveBeenCalledTimes(calls)
  })

  it('rejects unrecognized run fields, statuses, counters, receipts and correlation', async () => {
    const access = parsePluginStrategyAccess(accessFixture(), intent())
    const corrupt = [
      runFixture({ status: 'starting' }),
      runFixture({ sequence: '01' }),
      runFixture({ actionsSubmitted: 21 }),
      runFixture({ totalSubmittedQty: '0.011' }),
      runFixture({ pluginId: 'com.example.other' }),
      runFixture({ requestId: '00000000-0000-4000-8000-000000000099' }),
      runFixture({ reason: 'raw host error' }),
      runFixture({ extra: true }),
      runFixture({ lastReceipt: {
        sequence: '1', kind: 'placeOrder', status: 'accepted', submissionId: null,
        orderId: null, errorCode: 'plugin_strategy_rejected',
      } }),
      runFixture({ lastReceipt: {
        sequence: '1', kind: 'placeOrder', status: 'filled', submissionId: null,
        orderId: null, errorCode: null,
      } }),
    ]
    for (const value of corrupt) {
      vi.mocked(tauriInvoke).mockResolvedValueOnce(value)
      await expect(startPluginStrategy(intent(), access, {
        requestId: REQUEST_ID, resumeRunId: null, symbol: 'BTCUSDT', inputJson: '{"threshold":"50000"}',
        capabilities: ['account.read', 'market.read', 'trade.place', 'strategy.run'],
        policy: policy(), acknowledgeAutomaticTrading: true,
      })).rejects.toThrow(INVALID_RESPONSE)
    }
  })

  it('strictly parses list/control/stop-all/reconcile and correlates direct responses', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce({ schemaVersion: 1, runs: [runFixture()] })
    expect((await listPluginStrategies()).runs).toHaveLength(1)

    vi.mocked(tauriInvoke).mockResolvedValueOnce(runFixture({ status: 'paused' }))
    expect((await controlPluginStrategy(RUN_ID, 'pause')).status).toBe('paused')
    expect(tauriInvoke).toHaveBeenLastCalledWith('control_plugin_strategy', {
      request: { runId: RUN_ID, action: 'pause' },
    })

    vi.mocked(tauriInvoke).mockResolvedValueOnce({ schemaVersion: 1, runs: [] })
    expect((await stopAllPluginStrategies()).runs).toEqual([])

    vi.mocked(tauriInvoke).mockResolvedValueOnce(runFixture({ status: 'paused' }))
    expect((await reconcilePluginStrategy(RUN_ID)).status).toBe('paused')
    expect(tauriInvoke).toHaveBeenLastCalledWith('reconcile_plugin_strategy', { runId: RUN_ID })

    vi.mocked(tauriInvoke).mockResolvedValueOnce(runFixture({ runId: '00000000-0000-4000-8000-000000000099' }))
    await expect(controlPluginStrategy(RUN_ID, 'stop')).rejects.toThrow(INVALID_RESPONSE)
    vi.mocked(tauriInvoke).mockResolvedValueOnce({ schemaVersion: 1, runs: [], extra: true })
    await expect(listPluginStrategies()).rejects.toThrow(INVALID_RESPONSE)
  })

  it('maps only exact native strategy errors and never exposes raw host text', () => {
    expect(pluginStrategyErrorMessage({
      code: 'plugin_strategy_token_invalid', message: 'D:\\private\\secret',
    })).toContain('凭据已过期')
    for (const error of [
      { code: 'plugin_strategy_future_code', message: 'raw host detail' },
      { code: 'plugin_strategy_denied', message: 'raw host detail', extra: true },
      new Error('raw host detail'),
    ]) {
      expect(pluginStrategyErrorMessage(error)).toBe('插件策略操作失败，请重试。')
      expect(pluginStrategyErrorMessage(error)).not.toContain('raw host detail')
    }
  })
})
