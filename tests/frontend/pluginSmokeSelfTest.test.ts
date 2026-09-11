import { afterEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { mount } from '@vue/test-utils'
import { confirmRemovalReload, runPluginSmokeSelfTest } from '../../src/plugin-smoke/selfTest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { usePluginStore } from '../../src/stores/plugin'
import PluginMarketplacePage from '../../src/components/plugins/PluginMarketplacePage.vue'

// Only the transport is replaced; driver and plugin store remain real.
vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn().mockResolvedValue(undefined) }))
afterEach(() => { vi.useRealTimers(); vi.clearAllMocks(); document.body.innerHTML = '' })

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
