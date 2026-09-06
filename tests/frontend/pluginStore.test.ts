import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type {
  PluginAvailabilityReason,
  PluginCatalogItem,
  PluginCatalogMutationResult,
  PluginCatalogSnapshot,
  PluginStatus,
} from '../../src/types/plugin'
import { usePluginStore } from '../../src/stores/plugin'

const serviceMocks = vi.hoisted(() => ({
  getCatalog: vi.fn(),
  setEnabled: vi.fn(),
}))

vi.mock('../../src/services/pluginService', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../src/services/pluginService')>(),
  getPluginCatalog: serviceMocks.getCatalog,
  setPluginEnabled: serviceMocks.setEnabled,
}))

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

function item(
  id: string,
  status: PluginStatus = 'disabled',
  overrides: Partial<PluginCatalogItem['manifest']> = {},
  reason: PluginAvailabilityReason | null = status === 'blocked' ? 'stateUnavailable' : null,
): PluginCatalogItem {
  return {
    manifest: {
      schemaVersion: 1,
      id,
      publisherId: 'com.easiflux',
      publisher: 'EasiFlux',
      name: id.endsWith('alpha') ? 'Alpha Tools' : 'Beta Tools',
      description: id.endsWith('alpha') ? 'Chart research' : 'Order workflow',
      version: '1.0.0',
      contributions: [],
      requestedCapabilities: [],
      ...overrides,
    },
    source: 'builtIn',
    status,
    statusReasonCode: reason,
    canToggle: status !== 'blocked',
    grantedCapabilities: [],
  }
}

function snapshot(
  revision: string,
  plugins: PluginCatalogItem[] = [
    item('com.easiflux.alpha', 'disabled'),
    item('com.easiflux.beta', 'enabled'),
  ],
): PluginCatalogSnapshot {
  return {
    schemaVersion: 1,
    revision,
    availability: 'available',
    availabilityReasonCode: null,
    plugins,
  }
}

function unavailableSnapshot(
  revision: string,
  reason: PluginAvailabilityReason = 'stateUnavailable',
): PluginCatalogSnapshot {
  return {
    schemaVersion: 1,
    revision,
    availability: 'unavailable',
    availabilityReasonCode: reason,
    plugins: [
      item('com.easiflux.alpha', 'blocked', {}, reason),
      item('com.easiflux.beta', 'blocked', {}, reason),
    ],
  }
}

function mutation(
  revision: string,
  id: string,
  enabled: boolean,
): PluginCatalogMutationResult {
  return {
    revision,
    plugin: item(id, enabled ? 'enabled' : 'disabled'),
  }
}

async function loadedStore(revision = '1') {
  serviceMocks.getCatalog.mockResolvedValueOnce(snapshot(revision))
  const store = usePluginStore()
  await store.load()
  return store
}

describe('plugin store loading and filtering', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    serviceMocks.getCatalog.mockReset()
    serviceMocks.setEnabled.mockReset()
  })

  it('shares the active request across concurrent load and retry callers', async () => {
    const response = deferred<PluginCatalogSnapshot>()
    serviceMocks.getCatalog.mockReturnValueOnce(response.promise)
    const store = usePluginStore()

    const first = store.load()
    const second = store.load()
    const retry = store.retry()

    expect(serviceMocks.getCatalog).toHaveBeenCalledTimes(1)
    response.resolve(snapshot('1'))
    await Promise.all([first, second, retry])
    expect(store.loadStatus).toBe('ready')
    expect(store.revision).toBe('1')
  })

  it('allows ordinary load only from idle after either initial success or failure', async () => {
    serviceMocks.getCatalog.mockResolvedValueOnce(snapshot('1'))
    const successful = usePluginStore()
    await successful.load()
    await successful.load()
    expect(serviceMocks.getCatalog).toHaveBeenCalledTimes(1)

    setActivePinia(createPinia())
    serviceMocks.getCatalog.mockReset()
    serviceMocks.getCatalog.mockRejectedValueOnce({
      code: 'plugin_state_unavailable',
      message: 'private path',
    })
    const failed = usePluginStore()
    await failed.load()
    await failed.load()

    expect(serviceMocks.getCatalog).toHaveBeenCalledTimes(1)
    expect(failed.loadStatus).toBe('error')
    expect(failed.loadError).toBe('插件状态暂不可用，请重试。')
  })

  it('recovers an initial failure only through explicit retry', async () => {
    serviceMocks.getCatalog
      .mockRejectedValueOnce(new Error('C:\\private\\state.json'))
      .mockResolvedValueOnce(snapshot('2'))
    const store = usePluginStore()

    await store.load()
    expect(store.loadStatus).toBe('error')
    expect(store.loadError).toBe('插件操作失败，请重试。')

    await store.retry()
    expect(store.loadStatus).toBe('ready')
    expect(store.loadError).toBeNull()
    expect(store.revision).toBe('2')
    expect(store.catalog).toHaveLength(2)
  })

  it('preserves confirmed catalog and ready state when a refresh fails', async () => {
    const store = await loadedStore()
    const confirmedCatalog = store.catalog
    serviceMocks.getCatalog.mockRejectedValueOnce({
      code: 'plugin_catalog_invalid',
      message: 'C:\\private\\catalog.json',
    })

    await store.retry()

    expect(store.catalog).toBe(confirmedCatalog)
    expect(store.loadStatus).toBe('ready')
    expect(store.loadError).toBe('插件目录不可用，请稍后重试。')
    expect(store.revision).toBe('1')
  })

  it('stores unavailable snapshot metadata even when every item is blocked', async () => {
    serviceMocks.getCatalog.mockResolvedValueOnce(unavailableSnapshot('7', 'catalogInvalid'))
    const store = usePluginStore()

    await store.load()

    expect(store.loadStatus).toBe('ready')
    expect(store.availability).toBe('unavailable')
    expect(store.availabilityReasonCode).toBe('catalogInvalid')
    expect(store.catalog.every((plugin) => plugin.status === 'blocked')).toBe(true)
  })

  it('filters case-insensitively across name, id, publisher, publisher id, and description', async () => {
    serviceMocks.getCatalog.mockResolvedValueOnce(snapshot('1', [
      item('com.easiflux.alpha', 'enabled', {
        name: 'Alpha Charts',
        publisher: 'Market Lab',
        publisherId: 'com.market-lab',
        description: 'Momentum Research',
      }),
      item('com.easiflux.beta', 'disabled', {
        name: 'Beta Orders',
        publisher: 'Execution House',
        publisherId: 'com.execution-house',
        description: 'Trading Workflow',
      }),
    ]))
    const store = usePluginStore()
    await store.load()
    const originalOrder = store.catalog.map((plugin) => plugin.manifest.id)

    for (const query of ['ALPHA CHARTS', 'EASIFLUX.ALPHA', 'market lab', 'MARKET-LAB', 'momentum']) {
      store.setQuery(query)
      expect(store.visiblePlugins.map((plugin) => plugin.manifest.id)).toEqual([
        'com.easiflux.alpha',
      ])
    }

    store.setQuery('orders')
    store.setStatusFilter('enabled')
    expect(store.visiblePlugins).toEqual([])
    store.setStatusFilter('disabled')
    expect(store.visiblePlugins.map((plugin) => plugin.manifest.id)).toEqual([
      'com.easiflux.beta',
    ])
    expect(store.catalog.map((plugin) => plugin.manifest.id)).toEqual(originalOrder)
    expect(store.visiblePlugins).not.toBe(store.catalog)
  })

  it('intersects the blocked filter with query without changing catalog order', async () => {
    serviceMocks.getCatalog.mockResolvedValueOnce(unavailableSnapshot('4'))
    const store = usePluginStore()
    await store.load()
    const originalOrder = store.catalog.map((plugin) => plugin.manifest.id)

    store.setQuery('alpha')
    store.setStatusFilter('blocked')

    expect(store.visiblePlugins.map((plugin) => plugin.manifest.id)).toEqual([
      'com.easiflux.alpha',
    ])
    expect(store.catalog.map((plugin) => plugin.manifest.id)).toEqual(originalOrder)
  })
})

describe('plugin store mutation ownership and revisions', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    serviceMocks.getCatalog.mockReset()
    serviceMocks.setEnabled.mockReset()
  })

  it('replaces pending and error containers while leaving state unmodified until confirmation', async () => {
    const store = await loadedStore()
    const response = deferred<PluginCatalogMutationResult>()
    serviceMocks.setEnabled.mockReturnValueOnce(response.promise)
    const pendingBefore = store.pendingIds
    const errorsBefore = store.actionErrors
    const changing = store.setEnabled('com.easiflux.alpha', true)

    expect(store.pendingIds).not.toBe(pendingBefore)
    expect(store.pendingIds.has('com.easiflux.alpha')).toBe(true)
    expect(store.catalog[0].status).toBe('disabled')

    response.reject({ code: 'plugin_state_persist_failed', message: 'private path' })
    await expect(changing).resolves.toBe(false)
    expect(store.pendingIds.has('com.easiflux.alpha')).toBe(false)
    expect(store.actionErrors).not.toBe(errorsBefore)
    expect(store.actionErrors['com.easiflux.alpha']).toBe('保存插件状态失败，请重试。')
    expect(store.catalog[0].status).toBe('disabled')
  })

  it('updates only the confirmed target and adopts a greater revision with BigInt precision', async () => {
    const store = await loadedStore('9007199254740992')
    const untouched = store.catalog[1]
    serviceMocks.setEnabled.mockResolvedValueOnce(
      mutation('9007199254740993', 'com.easiflux.alpha', true),
    )

    await expect(store.setEnabled('com.easiflux.alpha', true)).resolves.toBe(true)

    expect(store.catalog[0].status).toBe('enabled')
    expect(store.catalog[1]).toBe(untouched)
    expect(store.revision).toBe('9007199254740993')
    expect(store.actionErrors['com.easiflux.alpha']).toBeUndefined()
  })

  it('preserves confirmed state and revision when mutation fails', async () => {
    const store = await loadedStore('8')
    const confirmed = store.catalog
    serviceMocks.setEnabled.mockRejectedValueOnce(new Error('C:\\private\\stack'))

    await expect(store.setEnabled('com.easiflux.alpha', true)).resolves.toBe(false)

    expect(store.catalog).toBe(confirmed)
    expect(store.catalog[0].status).toBe('disabled')
    expect(store.revision).toBe('8')
    expect(store.actionErrors['com.easiflux.alpha']).toBe('插件操作失败，请重试。')
  })

  it('keeps a newer same-id request pending and lets only its failure publish', async () => {
    const store = await loadedStore()
    const oldResponse = deferred<PluginCatalogMutationResult>()
    const newResponse = deferred<PluginCatalogMutationResult>()
    serviceMocks.setEnabled
      .mockReturnValueOnce(oldResponse.promise)
      .mockReturnValueOnce(newResponse.promise)

    const oldRequest = store.setEnabled('com.easiflux.alpha', true)
    const newRequest = store.setEnabled('com.easiflux.alpha', true)
    oldResponse.resolve(mutation('3', 'com.easiflux.alpha', true))
    await oldRequest

    expect(store.catalog[0].status).toBe('disabled')
    expect(store.pendingIds.has('com.easiflux.alpha')).toBe(true)
    expect(store.actionErrors['com.easiflux.alpha']).toBeUndefined()

    newResponse.reject({ code: 'plugin_state_persist_failed', message: 'private' })
    await newRequest
    expect(store.catalog[0].status).toBe('disabled')
    expect(store.pendingIds.has('com.easiflux.alpha')).toBe(false)
    expect(store.actionErrors['com.easiflux.alpha']).toBe('保存插件状态失败，请重试。')
  })

  it('ignores a late same-id failure after the newer success owns data and pending', async () => {
    const store = await loadedStore()
    const oldResponse = deferred<PluginCatalogMutationResult>()
    const newResponse = deferred<PluginCatalogMutationResult>()
    serviceMocks.setEnabled
      .mockReturnValueOnce(oldResponse.promise)
      .mockReturnValueOnce(newResponse.promise)

    const oldRequest = store.setEnabled('com.easiflux.alpha', true)
    const newRequest = store.setEnabled('com.easiflux.alpha', true)
    newResponse.resolve(mutation('3', 'com.easiflux.alpha', true))
    await newRequest
    expect(store.catalog[0].status).toBe('enabled')
    expect(store.pendingIds.has('com.easiflux.alpha')).toBe(false)

    oldResponse.reject({ code: 'plugin_not_found', message: 'private' })
    await oldRequest
    expect(store.catalog[0].status).toBe('enabled')
    expect(store.actionErrors['com.easiflux.alpha']).toBeUndefined()
    expect(store.pendingIds.has('com.easiflux.alpha')).toBe(false)
  })

  it('keeps different ids independent when older revisions resolve later', async () => {
    const store = await loadedStore('1')
    const alphaResponse = deferred<PluginCatalogMutationResult>()
    const betaResponse = deferred<PluginCatalogMutationResult>()
    serviceMocks.setEnabled
      .mockReturnValueOnce(alphaResponse.promise)
      .mockReturnValueOnce(betaResponse.promise)

    const alphaRequest = store.setEnabled('com.easiflux.alpha', true)
    const betaRequest = store.setEnabled('com.easiflux.beta', false)
    expect(store.pendingIds).toEqual(new Set(['com.easiflux.alpha', 'com.easiflux.beta']))

    alphaResponse.resolve(mutation('3', 'com.easiflux.alpha', true))
    await alphaRequest
    expect(store.catalog[0].status).toBe('enabled')
    expect(store.pendingIds).toEqual(new Set(['com.easiflux.beta']))

    betaResponse.resolve(mutation('2', 'com.easiflux.beta', false))
    await betaRequest
    expect(store.catalog.map((plugin) => plugin.status)).toEqual(['enabled', 'disabled'])
    expect(store.revision).toBe('3')
    expect(store.pendingIds.size).toBe(0)
  })

  it('does not let a stale full snapshot overwrite a newer confirmed mutation', async () => {
    const store = await loadedStore('1')
    const refresh = deferred<PluginCatalogSnapshot>()
    serviceMocks.getCatalog.mockReturnValueOnce(refresh.promise)
    const refreshing = store.retry()
    serviceMocks.setEnabled.mockResolvedValueOnce(mutation('2', 'com.easiflux.alpha', true))

    await store.setEnabled('com.easiflux.alpha', true)
    refresh.resolve(snapshot('1', [item('com.easiflux.beta', 'enabled')]))
    await refreshing

    expect(store.catalog.map((plugin) => plugin.manifest.id)).toEqual([
      'com.easiflux.alpha',
      'com.easiflux.beta',
    ])
    expect(store.catalog.find((plugin) => plugin.manifest.id === 'com.easiflux.alpha')?.status)
      .toBe('enabled')
    expect(store.revision).toBe('2')
  })

  it('lets a newer full snapshot supersede an older in-flight item response', async () => {
    const store = await loadedStore('1')
    const oldMutation = deferred<PluginCatalogMutationResult>()
    serviceMocks.setEnabled.mockReturnValueOnce(oldMutation.promise)
    const changing = store.setEnabled('com.easiflux.alpha', true)
    serviceMocks.getCatalog.mockResolvedValueOnce(snapshot('3'))

    await store.retry()
    oldMutation.resolve(mutation('2', 'com.easiflux.alpha', true))
    await changing

    expect(store.catalog[0].status).toBe('disabled')
    expect(store.revision).toBe('3')
    expect(store.pendingIds.has('com.easiflux.alpha')).toBe(false)
  })

  it('applies an older-revision response for a different untouched id without lowering global revision', async () => {
    const store = await loadedStore('1')
    serviceMocks.setEnabled
      .mockResolvedValueOnce(mutation('5', 'com.easiflux.alpha', true))
      .mockResolvedValueOnce(mutation('4', 'com.easiflux.beta', false))

    await store.setEnabled('com.easiflux.alpha', true)
    await store.setEnabled('com.easiflux.beta', false)

    expect(store.catalog.map((plugin) => plugin.status)).toEqual(['enabled', 'disabled'])
    expect(store.revision).toBe('5')
  })

  it('does not call the service or alter any state for an unknown id', async () => {
    const store = await loadedStore('4')
    const confirmedCatalog = store.catalog
    const confirmedPending = store.pendingIds
    const confirmedErrors = store.actionErrors

    await expect(store.setEnabled('com.easiflux.unknown', true)).resolves.toBe(false)

    expect(serviceMocks.setEnabled).not.toHaveBeenCalled()
    expect(store.catalog).toBe(confirmedCatalog)
    expect(store.revision).toBe('4')
    expect(store.pendingIds).toBe(confirmedPending)
    expect(store.actionErrors).toBe(confirmedErrors)
  })
})
