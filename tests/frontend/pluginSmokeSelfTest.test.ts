import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'
import {
  confirmRemovalReload,
  isExpectedSmaOutput,
  runPluginSmokeSelfTest,
} from '../../src/plugin-smoke/selfTest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { usePluginStore } from '../../src/stores/plugin'
import PluginMarketplacePage from '../../src/components/plugins/PluginMarketplacePage.vue'

// Only the transport is replaced; driver and plugin store remain real.
vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn().mockResolvedValue(undefined) }))
let showModalDescriptor: PropertyDescriptor | undefined
let closeDescriptor: PropertyDescriptor | undefined
beforeEach(() => {
  showModalDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'showModal')
  closeDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'close')
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
    configurable: true,
    value(this: HTMLDialogElement) { this.setAttribute('open', '') },
  })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', {
    configurable: true,
    value(this: HTMLDialogElement) { this.removeAttribute('open') },
  })
})
afterEach(() => {
  vi.useRealTimers()
  vi.clearAllMocks()
  document.body.innerHTML = ''
  if (showModalDescriptor) Object.defineProperty(HTMLDialogElement.prototype, 'showModal', showModalDescriptor)
  else delete HTMLDialogElement.prototype.showModal
  if (closeDescriptor) Object.defineProperty(HTMLDialogElement.prototype, 'close', closeDescriptor)
  else delete HTMLDialogElement.prototype.close
})

const fixtureId = 'com.easiflux.examples.series-sma'
const accountFixtureId = 'com.easiflux.examples.account-workflow'
const fixtureManifest = {
  schemaVersion: 4,
  id: fixtureId,
  publisherId: 'com.easiflux.examples',
  publisher: 'EasiFlux example (unverified)',
  name: 'Series simple moving average',
  description: 'Computes the last-period average in a guest.',
  version: '1.0.0',
  contributions: [{
    kind: 'command',
    contributionId: 'series.sma',
    title: 'Compute simple moving average',
    actionId: 'sandbox.computeSeries',
    params: {
      runtime: 'wasm-v1', abi: 'series-f64-v1', moduleBase64: 'AGFzbQEAAAA=',
      parameter: { label: 'Period', default: 3, min: 1, max: 4096 },
    },
  }],
  requestedCapabilities: [],
}

const accountFixtureManifest = {
  schemaVersion: 5,
  id: accountFixtureId,
  publisherId: 'com.easiflux.examples',
  publisher: 'EasiFlux example (unverified)',
  name: 'Account workflow safety example',
  description: 'Synthetic account workflow fixture.',
  version: '1.0.0',
  contributions: [
    {
      kind: 'command', contributionId: 'account.available-balance',
      title: 'Show granted available balance', actionId: 'sandbox.accountWorkflow',
      params: { runtime: 'wasm-v1', abi: 'account-json-v1', moduleBase64: 'AGFzbQEAAAA=', defaultInput: '{}' },
    },
    {
      kind: 'command', contributionId: 'account.place-order',
      title: 'Prepare example limit order', actionId: 'sandbox.accountWorkflow',
      params: {
        runtime: 'wasm-v1', abi: 'account-json-v1', moduleBase64: 'AGFzbQEAAAA=',
        defaultInput: '{"kind":"placeOrder","order":{"symbol":"BTCUSDT","side":"Buy","orderType":"Limit","qty":"0.001","price":"50000","timeInForce":"GTC","positionIdx":1,"reduceOnly":false}}',
      },
    },
    {
      kind: 'command', contributionId: 'account.cancel-first-open-order',
      title: 'Prepare cancellation of first captured order', actionId: 'sandbox.accountWorkflow',
      params: { runtime: 'wasm-v1', abi: 'account-json-v1', moduleBase64: 'AGFzbQEAAAA=', defaultInput: '{}' },
    },
  ],
  requestedCapabilities: [
    'account.read', 'balances.read', 'orders.read', 'trade.place', 'trade.cancel',
  ],
}

function smokeItem(
  status: 'enabled' | 'disabled',
  manifest: typeof fixtureManifest | typeof accountFixtureManifest = fixtureManifest,
) {
  return {
    manifest,
    source: 'localDeclarative',
    management: 'managed',
    canRemove: status === 'disabled',
    toggleBlockReasonCode: null,
    status,
    statusReasonCode: null,
    canToggle: true,
    grantedCapabilities: [],
  }
}

function smokeSnapshot(
  revision: string,
  catalogGeneration: string,
  plugins: ReturnType<typeof smokeItem>[] = [],
) {
  return {
    schemaVersion: 3,
    revision,
    catalogGeneration,
    availability: 'available',
    availabilityReasonCode: null,
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
    managedOwnership: {
      status: 'available', conflictingEntryCount: 0,
      rollbackPendingCount: 0, cleanupPendingCount: 0,
    },
    plugins,
  }
}

it('accepts only the exact rendered SMA scalar and provenance', () => {
  const provenance = `来源：Series SMA（${fixtureId}） / series.sma；输入 5 个；Period 3。`
  expect(isExpectedSmaOutput('结果：4', provenance)).toBe(true)
  expect(isExpectedSmaOutput('结果：40', provenance)).toBe(false)
  expect(isExpectedSmaOutput('结果：4.5', provenance)).toBe(false)
  expect(isExpectedSmaOutput('结果：4', provenance.replace('输入 5 个', '输入 4 个'))).toBe(false)
  expect(isExpectedSmaOutput('结果：4', provenance.replace('Period 3', 'Period 2'))).toBe(false)
})

it('drives v4 and v5 authority, confirmations, revocation and forbidden IPC through real UI', async () => {
  const pinia = createPinia()
  setActivePinia(pinia)
  let prepareCount = 0
  let revision = 0
  let generation = 0
  let installed: ReturnType<typeof smokeItem>[] = []
  let grantedCapabilities: string[] = []
  let grantRevision = 0
  const preparedActions = new Map<string, 'placeOrder' | 'cancelOrder'>()
  const snapshot = () => smokeSnapshot(String(revision), String(generation), installed)
  vi.mocked(tauriInvoke).mockImplementation(async (command, args) => {
    if (command === 'get_plugin_catalog') return snapshot()
    if (command === 'prepare_local_manifest_import') {
      prepareCount++
      const manifest = prepareCount <= 2 ? fixtureManifest : accountFixtureManifest
      return {
        schemaVersion: 2, status: 'ready', token: String(prepareCount).repeat(32),
        expiresInSeconds: 300, catalogGeneration: String(generation), manifest,
        assessment: { kind: 'notInCatalog' },
      }
    }
    if (command === 'cancel_local_manifest_import') return { schemaVersion: 1, status: 'cancelled' }
    if (command === 'commit_local_manifest_import') {
      const manifest = prepareCount <= 2 ? fixtureManifest : accountFixtureManifest
      revision++
      generation++
      installed = [...installed, smokeItem('disabled', manifest)]
      return { schemaVersion: 2, status: 'imported', pluginId: manifest.id, snapshot: snapshot() }
    }
    if (command === 'set_plugin_enabled') {
      const request = args as { id: string; enabled: boolean }
      revision++
      installed = installed.map(item => item.manifest.id === request.id
        ? smokeItem(request.enabled ? 'enabled' : 'disabled', item.manifest)
        : item)
      const plugin = installed.find(item => item.manifest.id === request.id)
      if (!plugin) throw new Error('fixture missing')
      return {
        schemaVersion: 3, revision: String(revision), catalogGeneration: String(generation),
        plugin,
      }
    }
    if (command === 'execute_plugin_compute') {
      const request = (args as { request: { requestId: string; expectedRevision: string } }).request
      if (!installed.some(item => item.manifest.id === fixtureId && item.status === 'enabled')) {
        throw { code: 'plugin_compute_disabled', message: 'details must not be rendered' }
      }
      return {
        schemaVersion: 1, requestId: request.requestId, pluginId: fixtureId,
        contributionId: 'series.sma', catalogGeneration: String(generation),
        revision: request.expectedRevision,
        value: 4, inputCount: 5, parameter: 3,
      }
    }
    if (command === 'get_plugin_workflow_access') {
      const request = (args as { request: {
        pluginId: string; contributionId: string; expectedCatalogGeneration: string;
        expectedRevision: string;
      } }).request
      return {
        schemaVersion: 1, pluginId: request.pluginId, contributionId: request.contributionId,
        catalogGeneration: request.expectedCatalogGeneration,
        revision: request.expectedRevision,
        account: {
          accountId: 'plugin-smoke-account', sessionEpoch: '1',
          environment: 'Injected synthetic smoke host',
        },
        requestedCapabilities: accountFixtureManifest.requestedCapabilities,
        grantedCapabilities, grantRevision: String(grantRevision),
      }
    }
    if (command === 'set_plugin_workflow_grants') {
      const request = (args as { request: {
        pluginId: string; contributionId: string; expectedCatalogGeneration: string;
        expectedRevision: string; capabilities: string[];
      } }).request
      grantedCapabilities = [...request.capabilities]
      grantRevision++
      return {
        schemaVersion: 1, pluginId: request.pluginId, contributionId: request.contributionId,
        catalogGeneration: request.expectedCatalogGeneration, revision: request.expectedRevision,
        account: {
          accountId: 'plugin-smoke-account', sessionEpoch: '1',
          environment: 'Injected synthetic smoke host',
        },
        requestedCapabilities: accountFixtureManifest.requestedCapabilities,
        grantedCapabilities, grantRevision: String(grantRevision),
      }
    }
    if (command === 'run_plugin_workflow') {
      const request = (args as { request: {
        pluginId: string; contributionId: string; expectedCatalogGeneration: string;
        expectedRevision: string; requestId: string; symbol: string; inputJson: string;
      } }).request
      const place = request.contributionId === 'account.place-order'
      const output = place ? JSON.parse(request.inputJson) : {
        kind: 'cancelOrder', order: { symbol: 'BTCUSDT', orderId: 'plugin-smoke-open-order' },
      }
      const token = place
        ? '00000000-0000-4000-8000-000000000101'
        : '00000000-0000-4000-8000-000000000102'
      preparedActions.set(token, place ? 'placeOrder' : 'cancelOrder')
      return {
        schemaVersion: 1, requestId: request.requestId,
        pluginId: request.pluginId, contributionId: request.contributionId,
        catalogGeneration: request.expectedCatalogGeneration, revision: request.expectedRevision,
        account: {
          accountId: 'plugin-smoke-account', sessionEpoch: '1',
          environment: 'Injected synthetic smoke host',
        },
        grantRevision: String(grantRevision),
        snapshot: {
          schemaVersion: 1,
          account: {
            accountId: 'plugin-smoke-account', sessionEpoch: '1',
            environment: 'Injected synthetic smoke host',
          },
          capturedAtMs: '1789948800000', symbol: request.symbol,
          grantedCapabilities,
          balances: {
            items: [{ asset: 'USDT', available: '12.5', frozen: '0', total: '12.5' }],
            fetchedAtMs: '1789948799900', partial: true,
          },
          positions: null,
          orders: {
            items: [{
              orderId: 'plugin-smoke-open-order', symbol: 'BTCUSDT', side: 'Buy',
              orderType: 'Limit', price: '50000', qty: '0.001', status: 'New',
              orderLinkId: 'plugin-smoke-existing-link', filledQty: '0', avgPrice: '0',
            }],
            fetchedAtMs: '1789948799950', partial: true,
          },
          market: null,
        },
        output,
        confirmation: {
          token, expiresAtMs: '1789948860000',
          submissionId: place ? '00000000-0000-4000-8000-000000000201' : null,
        },
      }
    }
    if (command === 'confirm_plugin_workflow') {
      const token = (args as { token: string }).token
      const action = preparedActions.get(token)
      if (!action) throw new Error('unknown confirmation token')
      preparedActions.delete(token)
      return {
        schemaVersion: 1, token,
        account: {
          accountId: 'plugin-smoke-account', sessionEpoch: '1',
          environment: 'Injected synthetic smoke host',
        },
        action, status: 'accepted',
        submissionId: action === 'placeOrder'
          ? '00000000-0000-4000-8000-000000000201' : null,
        order: {
          orderId: action === 'placeOrder' ? 'plugin-smoke-placed-1' : 'plugin-smoke-open-order',
          symbol: 'BTCUSDT', side: 'Buy', orderType: 'Limit',
          price: action === 'placeOrder' ? '49000' : '50000',
          qty: action === 'placeOrder' ? '0.002' : '0.001',
          status: action === 'placeOrder' ? 'New' : 'Unknown',
          orderLinkId: action === 'placeOrder'
            ? '00000000-0000-4000-8000-000000000201' : 'plugin-smoke-existing-link',
          filledQty: '0', avgPrice: '0',
        },
        errorCode: null,
      }
    }
    if (command === 'cancel_plugin_compute') {
      return { schemaVersion: 1, requestId: 'unused', cancelled: false }
    }
    if (command === 'list_account_profiles') throw new Error('not allowed')
    if (command === 'remove_managed_local_plugin') {
      const pluginId = (args as { id: string }).id
      revision++
      generation++
      installed = installed.filter(item => item.manifest.id !== pluginId)
      return { schemaVersion: 1, status: 'removed', pluginId, snapshot: snapshot() }
    }
    if (command === 'reload_plugin_catalog') return snapshot()
    if (command === 'finish_plugin_smoke') return undefined
    throw new Error(`unexpected command: ${command}`)
  })

  const store = usePluginStore()
  await store.load()
  const page = mount(PluginMarketplacePage, {
    props: { section: 'manage' },
    global: { plugins: [pinia] },
    attachTo: document.body,
  })
  try {
    await runPluginSmokeSelfTest()
    await flushPromises()
    const commands = vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)
    expect(commands.filter(command => command === 'execute_plugin_compute')).toHaveLength(3)
    expect(commands.filter(command => command === 'run_plugin_workflow')).toHaveLength(2)
    expect(commands.filter(command => command === 'confirm_plugin_workflow')).toHaveLength(2)
    expect(commands.filter(command => command === 'set_plugin_workflow_grants')).toHaveLength(2)
    expect(commands).toContain('list_account_profiles')
    expect(tauriInvoke).toHaveBeenLastCalledWith('finish_plugin_smoke', {
      success: true,
      detail: expect.stringContaining('v4 SMA preserved'),
    })
  } finally {
    page.unmount()
  }
}, 30_000)

it('reports initial catalog timeout once without dispatching a lifecycle mutation', async () => {
  vi.useFakeTimers()
  setActivePinia(createPinia())
  document.body.innerHTML = '<button data-testid="plugin-import-button">Import</button>'
  let mutations = 0
  document.querySelector('button')!.addEventListener('click', () => { mutations += 1 })
  const result = runPluginSmokeSelfTest()
  await vi.advanceTimersByTimeAsync(15_100)
  await result
  expect(mutations).toBe(0)
  expect(tauriInvoke).toHaveBeenCalledTimes(1)
  expect(tauriInvoke).toHaveBeenCalledWith('finish_plugin_smoke', {
    success: false, detail: 'Timed out: initial catalog readiness',
  })
  await vi.advanceTimersByTimeAsync(90_000)
  expect(tauriInvoke).toHaveBeenCalledTimes(1)
})

it('confirms a real Marketplace reload even when the unchanged catalog retains its generation', async () => {
  vi.useFakeTimers()
  const pinia = createPinia()
  setActivePinia(pinia)
  vi.mocked(tauriInvoke).mockResolvedValue({
    schemaVersion: 3, revision: '4', catalogGeneration: '8',
    availability: 'available', availabilityReasonCode: null,
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
    managedOwnership: { status: 'available', conflictingEntryCount: 0, rollbackPendingCount: 0, cleanupPendingCount: 0 },
    plugins: [],
  })
  const store = usePluginStore()
  await store.load()
  const page = mount(PluginMarketplacePage, { props: { section: 'manage' }, global: { plugins: [pinia] }, attachTo: document.body })
  try {
    const result = confirmRemovalReload().then(() => null, error => error)
    expect(store.reloadStatus).toBe('loading')
    await vi.advanceTimersByTimeAsync(15_100)
    expect(await result).toBeNull()
    expect(store.reloadStatus).toBe('ready')
    expect(store.catalogGeneration).toBe('8')
    expect(tauriInvoke).toHaveBeenCalledWith('reload_plugin_catalog')
  } finally { page.unmount() }
})
