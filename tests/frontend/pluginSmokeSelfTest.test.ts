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

function smokeItem(status: 'enabled' | 'disabled') {
  return {
    manifest: fixtureManifest,
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
  status?: 'enabled' | 'disabled',
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
    plugins: status ? [smokeItem(status)] : [],
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

it('drives disabled-enable-compute-disable authority and a forbidden IPC through real UI', async () => {
  const pinia = createPinia()
  setActivePinia(pinia)
  let prepareCount = 0
  vi.mocked(tauriInvoke).mockImplementation(async (command, args) => {
    if (command === 'get_plugin_catalog') return smokeSnapshot('0', '0')
    if (command === 'prepare_local_manifest_import') {
      prepareCount++
      return {
        schemaVersion: 2, status: 'ready', token: String(prepareCount).repeat(32),
        expiresInSeconds: 300, catalogGeneration: '0', manifest: fixtureManifest,
        assessment: { kind: 'notInCatalog' },
      }
    }
    if (command === 'cancel_local_manifest_import') return { schemaVersion: 1, status: 'cancelled' }
    if (command === 'commit_local_manifest_import') {
      return { schemaVersion: 2, status: 'imported', pluginId: fixtureId, snapshot: smokeSnapshot('1', '1', 'disabled') }
    }
    if (command === 'set_plugin_enabled') {
      const enabled = (args as { enabled: boolean }).enabled
      return {
        schemaVersion: 3, revision: enabled ? '2' : '3', catalogGeneration: '1',
        plugin: smokeItem(enabled ? 'enabled' : 'disabled'),
      }
    }
    if (command === 'execute_plugin_compute') {
      const request = (args as { request: { requestId: string; expectedRevision: string } }).request
      if (request.expectedRevision !== '2') {
        throw { code: 'plugin_compute_disabled', message: 'details must not be rendered' }
      }
      return {
        schemaVersion: 1, requestId: request.requestId, pluginId: fixtureId,
        contributionId: 'series.sma', catalogGeneration: '1', revision: '2',
        value: 4, inputCount: 5, parameter: 3,
      }
    }
    if (command === 'list_account_profiles') throw new Error('not allowed')
    if (command === 'remove_managed_local_plugin') {
      return { schemaVersion: 1, status: 'removed', pluginId: fixtureId, snapshot: smokeSnapshot('4', '2') }
    }
    if (command === 'reload_plugin_catalog') return smokeSnapshot('4', '2')
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
    expect(commands).toContain('list_account_profiles')
    expect(tauriInvoke).toHaveBeenLastCalledWith('finish_plugin_smoke', {
      success: true,
      detail: expect.stringContaining('guest SMA 4'),
    })
  } finally {
    page.unmount()
  }
})

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
