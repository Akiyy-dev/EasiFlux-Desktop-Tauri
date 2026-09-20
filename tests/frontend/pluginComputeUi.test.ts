import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import PluginCommands from '../../src/components/plugins/PluginCommands.vue'
import PluginComputeDialog from '../../src/components/plugins/PluginComputeDialog.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { usePluginStore } from '../../src/stores/plugin'
import type {
  PluginCatalogSnapshot,
  PluginComputeExecutionIntent,
  PluginComputeResult,
} from '../../src/types/plugin'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T) => void
  reject: (reason: unknown) => void
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

function snapshot(
  revision = '13',
  catalogGeneration = '8',
  includeSecond = false,
): PluginCatalogSnapshot {
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
    plugins: [{
      manifest: {
        schemaVersion: 4,
        id: 'com.example.analytics',
        publisherId: 'com.example',
        publisher: 'Example',
        name: '<b>Analytics</b>',
        description: 'Local compute',
        version: '1.0.0',
        requestedCapabilities: [],
        contributions: [{
          kind: 'command',
          contributionId: 'analytics.average',
          title: '<img src=x> Average',
          actionId: 'sandbox.computeSeries',
          params: {
            runtime: 'wasm-v1',
            abi: 'series-f64-v1',
            moduleBase64: 'AGFzbQEAAAA=',
            parameter: { label: 'Window', default: 3, min: 1, max: 10 },
          },
        }, ...(includeSecond ? [{
          kind: 'command' as const,
          contributionId: 'analytics.sum',
          title: 'Sum',
          actionId: 'sandbox.computeSeries' as const,
          params: {
            runtime: 'wasm-v1' as const,
            abi: 'series-f64-v1' as const,
            moduleBase64: 'AGFzbQEAAAA=',
            parameter: { label: 'Scale', default: 5, min: 1, max: 20 },
          },
        }] : [])],
      },
      source: 'localDeclarative',
      management: 'external',
      canRemove: false,
      toggleBlockReasonCode: null,
      status: 'enabled',
      statusReasonCode: null,
      canToggle: true,
      grantedCapabilities: [],
    }],
  }
}

async function mountLoaded(catalog = snapshot()) {
  const pinia = createPinia()
  setActivePinia(pinia)
  vi.mocked(tauriInvoke).mockResolvedValueOnce(catalog)
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
})

describe('plugin compute command UI', () => {
  it('selects compute explicitly and waits for Run before invoking the backend', async () => {
    const { wrapper } = await mountLoaded()
    const command = wrapper.get('[data-testid="plugin-command-button"]')

    expect(command.text()).toContain('运行计算')
    expect(command.attributes('disabled')).toBeUndefined()
    await command.trigger('click')

    const dialog = wrapper.get('[data-testid="plugin-compute-dialog"]')
    expect(dialog.text()).toContain('<img src=x> Average')
    expect(dialog.find('img').exists()).toBe(false)
    expect(wrapper.get<HTMLInputElement>('[data-testid="plugin-compute-parameter"]').element.value)
      .toBe('3')
    expect(tauriInvoke).toHaveBeenCalledTimes(1)
  })

  it('runs once, disables duplicate Run, and renders scalar provenance as plain text', async () => {
    const { wrapper } = await mountLoaded()
    await wrapper.get('[data-testid="plugin-command-button"]').trigger('click')
    const execute = deferred<PluginComputeResult>()
    vi.mocked(tauriInvoke).mockReturnValueOnce(execute.promise)
    await wrapper.get('[data-testid="plugin-compute-input"]').setValue('1, 2 3e0')
    await wrapper.get('[data-testid="plugin-compute-parameter"]').setValue('2.5')
    const run = wrapper.get<HTMLButtonElement>('[data-testid="plugin-compute-run"]')

    await run.trigger('click')
    expect(run.element.disabled).toBe(true)
    expect(wrapper.get('[data-testid="plugin-compute-status"]').text()).toContain('正在运行')
    expect(wrapper.get('[data-testid="plugin-compute-dialog"]').attributes('aria-busy')).toBe('true')
    const executeCall = vi.mocked(tauriInvoke).mock.calls[1]
    expect(executeCall?.[0]).toBe('execute_plugin_compute')
    const sent = (executeCall?.[1] as { request: { requestId: string } }).request
    execute.resolve({
      schemaVersion: 1,
      requestId: sent.requestId,
      pluginId: 'com.example.analytics',
      contributionId: 'analytics.average',
      catalogGeneration: '8',
      revision: '13',
      value: 2,
      inputCount: 3,
      parameter: 2.5,
    })
    await flushPromises()

    expect(wrapper.get('[data-testid="plugin-compute-status"]').text()).toContain('已完成')
    expect(wrapper.get('[data-testid="plugin-compute-result"]').text()).toContain('2')
    const provenance = wrapper.get('[data-testid="plugin-compute-provenance"]').text()
    expect(provenance).toContain('<b>Analytics</b>')
    expect(provenance).toContain('analytics.average')
    expect(provenance).toContain('3')
    expect(provenance).toContain('2.5')
    expect(wrapper.find('b').exists()).toBe(false)
  })

  it('shows cancelling and holds local busy until the actual execution settles', async () => {
    const { wrapper } = await mountLoaded()
    await wrapper.get('[data-testid="plugin-command-button"]').trigger('click')
    const execute = deferred<PluginComputeResult>()
    vi.mocked(tauriInvoke).mockReturnValueOnce(execute.promise)
    await wrapper.get('[data-testid="plugin-compute-input"]').setValue('1 2 3')
    await wrapper.get('[data-testid="plugin-compute-run"]').trigger('click')
    const executeCall = vi.mocked(tauriInvoke).mock.calls[1]
    const requestId = (executeCall?.[1] as { request: { requestId: string } }).request.requestId
    vi.mocked(tauriInvoke).mockResolvedValueOnce({ schemaVersion: 1, requestId, cancelled: true })

    await wrapper.get('[data-testid="plugin-compute-cancel"]').trigger('click')
    await flushPromises()
    expect(wrapper.get('[data-testid="plugin-compute-status"]').text()).toContain('正在取消')
    expect(wrapper.get('[data-testid="plugin-compute-dialog"]').attributes('aria-busy')).toBe('true')

    execute.reject({ code: 'plugin_compute_cancelled', message: 'guest details' })
    await flushPromises()
    expect(wrapper.get('[data-testid="plugin-compute-status"]').text()).toContain('错误')
    expect(wrapper.get('[data-testid="plugin-compute-error"]').text()).toContain('已取消')
    expect(wrapper.get('[data-testid="plugin-compute-dialog"]').attributes('aria-busy'))
      .toBeUndefined()
  })

  it('discards a success that races with a requested cancellation', async () => {
    const { wrapper } = await mountLoaded()
    await wrapper.get('[data-testid="plugin-command-button"]').trigger('click')
    const execute = deferred<PluginComputeResult>()
    vi.mocked(tauriInvoke).mockReturnValueOnce(execute.promise)
    await wrapper.get('[data-testid="plugin-compute-input"]').setValue('1 2 3')
    await wrapper.get('[data-testid="plugin-compute-run"]').trigger('click')
    const executeCall = vi.mocked(tauriInvoke).mock.calls[1]
    const requestId = (executeCall?.[1] as { request: { requestId: string } }).request.requestId
    vi.mocked(tauriInvoke).mockResolvedValueOnce({ schemaVersion: 1, requestId, cancelled: false })
    await wrapper.get('[data-testid="plugin-compute-cancel"]').trigger('click')
    execute.resolve({
      schemaVersion: 1, requestId,
      pluginId: 'com.example.analytics', contributionId: 'analytics.average',
      catalogGeneration: '8', revision: '13', value: 2, inputCount: 3, parameter: 3,
    })
    await flushPromises()

    expect(wrapper.find('[data-testid="plugin-compute-result"]').exists()).toBe(false)
    expect(wrapper.get('[data-testid="plugin-compute-error"]').text()).toContain('已取消')
  })

  it('keys the form by command identity, resets fields, and cancels A when selecting B', async () => {
    const { wrapper } = await mountLoaded(snapshot('13', '8', true))
    const buttons = wrapper.findAll('[data-testid="plugin-command-button"]')
    await buttons[0].trigger('click')
    const execute = deferred<PluginComputeResult>()
    vi.mocked(tauriInvoke).mockReturnValueOnce(execute.promise)
    await wrapper.get('[data-testid="plugin-compute-input"]').setValue('1 2 3')
    await wrapper.get('[data-testid="plugin-compute-run"]').trigger('click')
    const executeCall = vi.mocked(tauriInvoke).mock.calls[1]
    const requestId = (executeCall?.[1] as { request: { requestId: string } }).request.requestId
    vi.mocked(tauriInvoke).mockResolvedValueOnce({ schemaVersion: 1, requestId, cancelled: true })

    await buttons[1].trigger('click')
    await flushPromises()

    expect(tauriInvoke).toHaveBeenCalledWith('cancel_plugin_compute', { requestId })
    expect(wrapper.get('[data-testid="plugin-compute-dialog"]').text()).toContain('Sum')
    expect(wrapper.get<HTMLTextAreaElement>('[data-testid="plugin-compute-input"]').element.value)
      .toBe('')
    expect(wrapper.get<HTMLInputElement>('[data-testid="plugin-compute-parameter"]').element.value)
      .toBe('5')
    execute.resolve({
      schemaVersion: 1, requestId,
      pluginId: 'com.example.analytics', contributionId: 'analytics.average',
      catalogGeneration: '8', revision: '13', value: 2, inputCount: 3, parameter: 3,
    })
  })

  it('uses collision-resistant request IDs across separate form instances', async () => {
    const base: PluginComputeExecutionIntent = {
      actionId: 'sandbox.computeSeries',
      pluginId: 'com.example.analytics', pluginName: 'Analytics',
      contributionId: 'analytics.average', title: 'Average',
      runtime: 'wasm-v1', abi: 'series-f64-v1',
      parameter: { label: 'Window', default: 3, min: 1, max: 10 },
      expectedCatalogGeneration: '8', expectedRevision: '13',
    }
    const first = mount(PluginComputeDialog, { props: { intent: base } })
    const second = mount(PluginComputeDialog, { props: { intent: base } })
    const randomUUID = vi.spyOn(globalThis.crypto, 'randomUUID')
      .mockReturnValueOnce('00000000-0000-4000-8000-000000000001')
      .mockReturnValueOnce('00000000-0000-4000-8000-000000000002')
    vi.mocked(tauriInvoke).mockImplementation(() => new Promise(() => undefined))
    for (const wrapper of [first, second]) {
      await wrapper.get('[data-testid="plugin-compute-input"]').setValue('1')
      await wrapper.get('[data-testid="plugin-compute-run"]').trigger('click')
    }
    const requestIds = vi.mocked(tauriInvoke).mock.calls.map((call) => (
      (call[1] as { request: { requestId: string } }).request.requestId
    ))
    expect(new Set(requestIds).size).toBe(2)
    first.unmount()
    second.unmount()
    randomUUID.mockRestore()
  })

  it('cancels on command-context invalidation and discards a late success', async () => {
    const { wrapper, store } = await mountLoaded()
    await wrapper.get('[data-testid="plugin-command-button"]').trigger('click')
    const execute = deferred<PluginComputeResult>()
    vi.mocked(tauriInvoke).mockReturnValueOnce(execute.promise)
    await wrapper.get('[data-testid="plugin-compute-input"]').setValue('1 2 3')
    await wrapper.get('[data-testid="plugin-compute-run"]').trigger('click')
    const executeCall = vi.mocked(tauriInvoke).mock.calls[1]
    const requestId = (executeCall?.[1] as { request: { requestId: string } }).request.requestId
    const reload = deferred<PluginCatalogSnapshot>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'reload_plugin_catalog') return reload.promise
      if (command === 'cancel_plugin_compute') {
        return Promise.resolve({ schemaVersion: 1, requestId, cancelled: true })
      }
      return Promise.reject(new Error(`unexpected ${command}`))
    })

    const reloading = store.reload()
    await nextTick()
    await flushPromises()
    expect(wrapper.find('[data-testid="plugin-compute-dialog"]').exists()).toBe(false)
    expect(tauriInvoke).toHaveBeenCalledWith('cancel_plugin_compute', { requestId })

    execute.resolve({
      schemaVersion: 1, requestId,
      pluginId: 'com.example.analytics', contributionId: 'analytics.average',
      catalogGeneration: '8', revision: '13', value: 2, inputCount: 3, parameter: 3,
    })
    reload.resolve(snapshot('14', '9'))
    await reloading
    await flushPromises()
    expect(wrapper.find('[data-testid="plugin-compute-result"]').exists()).toBe(false)
  })
})
