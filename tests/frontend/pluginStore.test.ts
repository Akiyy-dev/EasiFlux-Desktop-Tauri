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
  reloadCatalog: vi.fn(),
  setEnabled: vi.fn(),
}))

vi.mock('../../src/services/pluginService', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../src/services/pluginService')>(),
  getPluginCatalog: serviceMocks.getCatalog,
  reloadPluginCatalog: serviceMocks.reloadCatalog,
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
  catalogGeneration = '1',
): PluginCatalogSnapshot {
  return {
    schemaVersion: 2,
    revision,
    catalogGeneration,
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
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
    schemaVersion: 2,
    revision,
    catalogGeneration: '1',
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
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
  catalogGeneration = '1',
): PluginCatalogMutationResult {
  return {
    schemaVersion: 2,
    revision,
    catalogGeneration,
    plugin: item(id, enabled ? 'enabled' : 'disabled'),
  }
}

async function loadedStore(revision = '1') {
  serviceMocks.getCatalog.mockResolvedValueOnce(snapshot(revision))
  const store = usePluginStore()
  await store.load()
  return store
}

describe('plugin store catalog generations', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    serviceMocks.getCatalog.mockReset()
    serviceMocks.reloadCatalog.mockReset()
    serviceMocks.setEnabled.mockReset()
  })

  it('captures the confirmed generation in the mutation request without fallback', async () => {
    serviceMocks.getCatalog.mockResolvedValueOnce(snapshot('4', undefined, '9007199254740993'))
    const store = usePluginStore()
    await store.load()
    serviceMocks.setEnabled.mockResolvedValueOnce(mutation('5', 'com.easiflux.alpha', true, '9007199254740993'))
    await store.setEnabled('com.easiflux.alpha', true)
    expect(serviceMocks.setEnabled).toHaveBeenCalledExactlyOnceWith('com.easiflux.alpha', true, '9007199254740993')
    expect(store.catalog[0].status).toBe('enabled')
  })

  it('new generation replaces membership and late old mutation cannot revive it', async () => {
    const store = await loadedStore()
    const oldMutation = deferred<PluginCatalogMutationResult>()
    serviceMocks.setEnabled.mockReturnValueOnce(oldMutation.promise)
    const toggle = store.setEnabled('com.easiflux.alpha', true)
    serviceMocks.reloadCatalog.mockResolvedValueOnce(snapshot('1', [], '2'))
    await store.reload()
    oldMutation.resolve(mutation('5', 'com.easiflux.alpha', true))
    await expect(toggle).resolves.toBe(false)
    expect(store.catalog).toEqual([])
    expect(store.catalogGeneration).toBe('2')
    expect(store.revision).toBe('1')
    expect(store.pendingIds.size).toBe(0)
  })

  it('new generation replaces metadata, source, discovery and item revisions even with a lower state revision', async () => {
    const store = await loadedStore('9')
    const replacement = { ...item('com.easiflux.alpha', 'disabled', { name: 'Replacement', version: '2.0.0' }), source: 'localDeclarative' as const }
    serviceMocks.reloadCatalog.mockResolvedValueOnce({
      ...snapshot('2', [replacement], '2'),
      localDiscovery: { status: 'degraded', rejectedPackageCount: 2 },
    })
    await store.reload()
    expect(store.catalog).toEqual([replacement])
    expect(store.catalogGeneration).toBe('2')
    expect(store.localDiscovery).toEqual({ status: 'degraded', rejectedPackageCount: 2 })
    serviceMocks.setEnabled.mockResolvedValueOnce({ ...mutation('3', 'com.easiflux.alpha', true, '2'), plugin: { ...replacement, status: 'enabled' } })
    await store.setEnabled('com.easiflux.alpha', true)
    expect(store.catalog[0].status).toBe('enabled')
  })

  it('ignores older generation snapshots entirely even with larger revisions and precise u64 generations', async () => {
    serviceMocks.getCatalog.mockResolvedValueOnce(snapshot('1', undefined, '9007199254740993'))
    const store = usePluginStore()
    await store.load()
    const confirmed = store.catalog
    serviceMocks.reloadCatalog.mockResolvedValueOnce({
      ...snapshot('100', [], '9007199254740992'),
      localDiscovery: { status: 'unavailable', rejectedPackageCount: 0 },
    })
    await store.reload()
    expect(store.catalog).toBe(confirmed)
    expect(store.catalogGeneration).toBe('9007199254740993')
    expect(store.localDiscovery).toEqual({ status: 'available', rejectedPackageCount: 0 })
    expect(store.revision).toBe('1')
  })

  it('keeps same-generation higher item revisions while adopting newer results for untouched items', async () => {
    const store = await loadedStore('1')
    serviceMocks.setEnabled.mockResolvedValueOnce(mutation('5', 'com.easiflux.alpha', true))
    await store.setEnabled('com.easiflux.alpha', true)
    serviceMocks.reloadCatalog.mockResolvedValueOnce(snapshot('4', [item('com.easiflux.alpha'), item('com.easiflux.beta', 'disabled')]))
    await store.reload()
    expect(store.catalog.map((plugin) => plugin.status)).toEqual(['enabled', 'disabled'])
    expect(store.revision).toBe('5')
  })

  it('does not regress same-generation availability from an older state snapshot', async () => {
    const store = await loadedStore('5')
    serviceMocks.getCatalog.mockResolvedValueOnce(unavailableSnapshot('4'))
    await store.retry()
    expect(store.availability).toBe('available')
    expect(store.availabilityReasonCode).toBeNull()
    expect(store.catalog.map((plugin) => plugin.status)).toEqual(['disabled', 'enabled'])
  })

  it('coalesces load and reload flights separately and ignores a late old initial load', async () => {
    const loadResponse = deferred<PluginCatalogSnapshot>()
    const reloadResponse = deferred<PluginCatalogSnapshot>()
    serviceMocks.getCatalog.mockReturnValueOnce(loadResponse.promise)
    serviceMocks.reloadCatalog.mockReturnValueOnce(reloadResponse.promise)
    const store = usePluginStore()
    const loading = store.load()
    const loadingAgain = store.load()
    const reloading = store.reload()
    const reloadingAgain = store.reload()
    expect(serviceMocks.getCatalog).toHaveBeenCalledTimes(1)
    expect(serviceMocks.reloadCatalog).toHaveBeenCalledTimes(1)
    expect(store.loadStatus).toBe('loading')
    expect(store.reloadStatus).toBe('loading')
    reloadResponse.resolve(snapshot('2', [], '2'))
    await Promise.all([reloading, reloadingAgain])
    loadResponse.resolve(snapshot('3'))
    await Promise.all([loading, loadingAgain])
    expect(store.catalog).toEqual([])
    expect(store.catalogGeneration).toBe('2')
    expect(store.revision).toBe('2')
    expect(store.loadStatus).toBe('ready')
    expect(store.reloadStatus).toBe('ready')
    serviceMocks.reloadCatalog.mockResolvedValueOnce(snapshot('2', [], '2'))
    await store.reload()
    expect(serviceMocks.reloadCatalog).toHaveBeenCalledTimes(2)
  })

  it('keeps catalog ready when an old initial load fails after reload confirms a generation', async () => {
    const response = deferred<PluginCatalogSnapshot>()
    serviceMocks.getCatalog.mockReturnValueOnce(response.promise)
    const store = usePluginStore()
    const loading = store.load()
    serviceMocks.reloadCatalog.mockResolvedValueOnce(snapshot('2', [], '2'))
    await store.reload()
    response.reject({ code: 'plugin_catalog_stale', message: 'private' })
    await loading
    expect(store.loadStatus).toBe('ready')
    expect(store.catalogGeneration).toBe('2')
  })

  it('retains confirmed catalog on stale reload failure and only retries explicitly', async () => {
    const store = await loadedStore('4')
    const confirmed = store.catalog
    serviceMocks.reloadCatalog.mockRejectedValueOnce({ code: 'plugin_catalog_stale', message: 'private' })
    await store.reload()
    expect(store.catalog).toBe(confirmed)
    expect(store.loadStatus).toBe('ready')
    expect(store.reloadStatus).toBe('error')
    expect(store.reloadError).toBe('插件目录已更新，请刷新后重试。')
    expect(serviceMocks.reloadCatalog).toHaveBeenCalledTimes(1)
    serviceMocks.reloadCatalog.mockResolvedValueOnce(snapshot('4', [], '2'))
    await store.reload()
    expect(store.reloadError).toBeNull()
    expect(store.reloadStatus).toBe('ready')
  })

  it('does not automatically replay stale mutations or discard confirmed data', async () => {
    const store = await loadedStore('4')
    const confirmed = store.catalog
    serviceMocks.setEnabled.mockRejectedValueOnce({ code: 'plugin_catalog_stale', message: 'private' })
    await expect(store.setEnabled('com.easiflux.alpha', true)).resolves.toBe(false)
    expect(store.catalog).toBe(confirmed)
    expect(store.actionErrors['com.easiflux.alpha']).toBe('插件目录已更新，请刷新后重试。')
    expect(serviceMocks.setEnabled).toHaveBeenCalledTimes(1)
    expect(serviceMocks.reloadCatalog).not.toHaveBeenCalled()
    expect(serviceMocks.getCatalog).toHaveBeenCalledTimes(1)
  })

  it('rejects a mutation response whose generation differs from the captured request', async () => {
    const store = await loadedStore()
    serviceMocks.setEnabled.mockResolvedValueOnce(mutation('9', 'com.easiflux.alpha', true, '2'))
    await expect(store.setEnabled('com.easiflux.alpha', true)).resolves.toBe(false)
    expect(store.catalog[0].status).toBe('disabled')
    expect(store.revision).toBe('1')
  })

  it('does not publish a late failure onto a new generation when no newer mutation exists', async () => {
    const store = await loadedStore()
    const response = deferred<PluginCatalogMutationResult>()
    serviceMocks.setEnabled.mockReturnValueOnce(response.promise)
    const changing = store.setEnabled('com.easiflux.alpha', true)
    serviceMocks.reloadCatalog.mockResolvedValueOnce(snapshot('1', undefined, '2'))
    await store.reload()
    response.reject({ code: 'plugin_catalog_stale', message: 'private' })
    await changing
    expect(store.actionErrors).toEqual({})
    expect(store.pendingIds.size).toBe(0)
  })

  it('requires the target to still exist even if a same-generation mutation revision is newer', async () => {
    const store = await loadedStore()
    const response = deferred<PluginCatalogMutationResult>()
    serviceMocks.setEnabled.mockReturnValueOnce(response.promise)
    const changing = store.setEnabled('com.easiflux.alpha', true)
    serviceMocks.getCatalog.mockResolvedValueOnce(snapshot('2', []))
    await store.retry()
    response.resolve(mutation('3', 'com.easiflux.alpha', true))
    await expect(changing).resolves.toBe(false)
    expect(store.catalog).toEqual([])
    expect(store.revision).toBe('2')
  })

  it('accepts equal per-item revisions for idempotent mutation confirmations', async () => {
    const store = await loadedStore('4')
    serviceMocks.setEnabled.mockResolvedValueOnce(mutation('4', 'com.easiflux.alpha', false))
    await expect(store.setEnabled('com.easiflux.alpha', false)).resolves.toBe(true)
    expect(store.catalog[0].status).toBe('disabled')
    expect(store.pendingIds.size).toBe(0)
  })

  it.each(['resolve', 'reject'] as const)('old-generation %s cannot overwrite replacement data, errors, or pending ownership', async (outcome) => {
    const store = await loadedStore()
    const oldResponse = deferred<PluginCatalogMutationResult>()
    const newResponse = deferred<PluginCatalogMutationResult>()
    serviceMocks.setEnabled.mockReturnValueOnce(oldResponse.promise).mockReturnValueOnce(newResponse.promise)
    const oldRequest = store.setEnabled('com.easiflux.alpha', true)
    const replacement = item('com.easiflux.alpha', 'disabled', { name: 'New contents' })
    serviceMocks.reloadCatalog.mockResolvedValueOnce(snapshot('1', [replacement], '2'))
    await store.reload()
    expect(store.pendingIds.size).toBe(0)
    const newRequest = store.setEnabled('com.easiflux.alpha', true)
    if (outcome === 'resolve') oldResponse.resolve(mutation('9', 'com.easiflux.alpha', true))
    else oldResponse.reject({ code: 'plugin_not_found', message: 'private' })
    await oldRequest
    expect(store.catalog).toEqual([replacement])
    expect(store.pendingIds.has('com.easiflux.alpha')).toBe(true)
    expect(store.actionErrors).toEqual({})
    newResponse.reject({ code: 'plugin_state_persist_failed', message: 'private' })
    await newRequest
    expect(store.actionErrors['com.easiflux.alpha']).toBe('保存插件状态失败，请重试。')
  })
})

describe('plugin store snapshot tie arbitration', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    serviceMocks.getCatalog.mockReset()
    serviceMocks.reloadCatalog.mockReset()
    serviceMocks.setEnabled.mockReset()
  })

  it.each(['load-first', 'reload-first'] as const)(
    'later-started reload wins tied counters regardless of completion order: %s', async (completion) => {
      const getResponse = deferred<PluginCatalogSnapshot>()
      const reloadResponse = deferred<PluginCatalogSnapshot>()
      serviceMocks.getCatalog.mockReturnValueOnce(getResponse.promise)
      serviceMocks.reloadCatalog.mockReturnValueOnce(reloadResponse.promise)
      const store = usePluginStore()
      const loading = store.load()
      const reloading = store.reload()
      const stale = unavailableSnapshot('0')
      stale.localDiscovery = { status: 'unavailable', rejectedPackageCount: 0 }
      const fresh = snapshot('0', [item('com.easiflux.alpha', 'disabled', { name: 'Recovered' })])
      fresh.localDiscovery = { status: 'degraded', rejectedPackageCount: 1 }
      if (completion === 'load-first') {
        getResponse.resolve(stale)
        await loading
        reloadResponse.resolve(fresh)
        await reloading
      } else {
        reloadResponse.resolve(fresh)
        await reloading
        getResponse.resolve(stale)
        await loading
      }
      expect(store.catalog).toEqual(fresh.plugins)
      expect(store.availability).toBe('available')
      expect(store.availabilityReasonCode).toBeNull()
      expect(store.localDiscovery).toEqual({ status: 'degraded', rejectedPackageCount: 1 })
      expect(store.catalogGeneration).toBe('1')
      expect(store.revision).toBe('0')
    },
  )

  it('a later explicit retry owns tied metadata without coalescing the reload flight', async () => {
    const store = await loadedStore('0')
    const reloadResponse = deferred<PluginCatalogSnapshot>()
    const retryResponse = deferred<PluginCatalogSnapshot>()
    serviceMocks.reloadCatalog.mockReturnValueOnce(reloadResponse.promise)
    serviceMocks.getCatalog.mockReturnValueOnce(retryResponse.promise)
    const reloading = store.reload()
    const retrying = store.retry()
    const sameReload = store.reload()
    const sameRetry = store.retry()
    expect(serviceMocks.reloadCatalog).toHaveBeenCalledTimes(1)
    expect(serviceMocks.getCatalog).toHaveBeenCalledTimes(2)
    const fresh = snapshot('0', [item('com.easiflux.alpha', 'disabled', { name: 'Explicit retry' })])
    retryResponse.resolve(fresh)
    await Promise.all([retrying, sameRetry])
    reloadResponse.resolve(unavailableSnapshot('0'))
    await Promise.all([reloading, sameReload])
    expect(store.catalog).toEqual(fresh.plugins)
    expect(store.availability).toBe('available')
  })

  it.each([
    ['generation', '2', '0'],
    ['revision', '1', '2'],
  ])('a higher %s wins even if its request started earlier', async (_label, generation, revision) => {
    const getResponse = deferred<PluginCatalogSnapshot>()
    serviceMocks.getCatalog.mockReturnValueOnce(getResponse.promise)
    serviceMocks.reloadCatalog.mockResolvedValueOnce(snapshot('1'))
    const store = usePluginStore()
    const loading = store.load()
    await store.reload()
    getResponse.resolve(snapshot(revision, [], generation))
    await loading
    expect(store.catalog).toEqual([])
    expect(store.catalogGeneration).toBe(generation)
    expect(store.revision).toBe(revision)
  })

  it('a failed later request cannot supersede an earlier successful tied response', async () => {
    const store = await loadedStore('0')
    const retryResponse = deferred<PluginCatalogSnapshot>()
    serviceMocks.getCatalog.mockReturnValueOnce(retryResponse.promise)
    const retrying = store.retry()
    serviceMocks.reloadCatalog.mockRejectedValueOnce({ code: 'plugin_catalog_stale', message: 'private' })
    await store.reload()
    const fresh = snapshot('0', [item('com.easiflux.alpha', 'disabled', { name: 'Retry result' })])
    retryResponse.resolve(fresh)
    await retrying
    expect(store.catalog).toEqual(fresh.plugins)
    expect(store.reloadError).toBe('插件目录已更新，请刷新后重试。')
  })

  it('snapshot tie ownership survives a newer mutation revision on another item', async () => {
    const store = await loadedStore('0')
    const retryResponse = deferred<PluginCatalogSnapshot>()
    serviceMocks.getCatalog.mockReturnValueOnce(retryResponse.promise)
    const retrying = store.retry()
    const fresh = snapshot('1', [
      item('com.easiflux.alpha'),
      item('com.easiflux.beta', 'enabled', { name: 'Fresh Beta' }),
    ])
    serviceMocks.reloadCatalog.mockResolvedValueOnce(fresh)
    await store.reload()
    serviceMocks.setEnabled.mockResolvedValueOnce(mutation('2', 'com.easiflux.alpha', true))
    await store.setEnabled('com.easiflux.alpha', true)
    retryResponse.resolve(snapshot('1'))
    await retrying
    expect(store.catalog[0].status).toBe('enabled')
    expect(store.catalog[1].manifest.name).toBe('Fresh Beta')
    expect(store.revision).toBe('2')
  })
})

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

  it.each([
    ['source', 'item', { source: 'localDeclarative' }],
    ['id', 'manifest', { id: 'com.easiflux.beta' }],
    ['schema', 'manifest', { schemaVersion: 2 }],
    ['publisher identity', 'manifest', { publisherId: 'com.other' }],
    ['publisher display', 'manifest', { publisher: 'Another publisher' }],
    ['version', 'manifest', { version: '2.0.0' }],
    ['name', 'manifest', { name: 'Replacement name' }],
    ['description', 'manifest', { description: 'Replacement description' }],
    ['unexpected icon', 'manifest', { icon: 'replacement.svg' }],
    ['contributions', 'manifest', { contributions: [{ command: 'unexpected' }] }],
    ['requested capabilities', 'manifest', { requestedCapabilities: ['network'] }],
    ['granted capabilities', 'item', { grantedCapabilities: ['network'] }],
  ])('ignores a mutation with changed immutable %s without altering confirmed state', async (_label, target, changes) => {
    const store = await loadedStore('4')
    const confirmed = store.catalog
    const response = deferred<PluginCatalogMutationResult>()
    serviceMocks.setEnabled.mockReturnValueOnce(response.promise)
    const changing = store.setEnabled('com.easiflux.alpha', true)
    const result = mutation('5', 'com.easiflux.alpha', true)
    // The mocked service deliberately bypasses its strict parser for context-defense cases.
    Object.assign(target === 'manifest' ? result.plugin.manifest : result.plugin, changes)
    expect(store.pendingIds.has('com.easiflux.alpha')).toBe(true)
    response.resolve(result)
    await expect(changing).resolves.toBe(false)
    expect(store.catalog).toBe(confirmed)
    expect(store.revision).toBe('4')
    expect(store.catalogGeneration).toBe('1')
    expect(store.pendingIds.size).toBe(0)
    expect(store.actionErrors).toEqual({})
  })

  it('accepts unchanged manifest content regardless of object key insertion order', async () => {
    const store = await loadedStore('4')
    const result = mutation('5', 'com.easiflux.alpha', true)
    const { version, ...remaining } = result.plugin.manifest
    result.plugin.manifest = { version, ...remaining }
    serviceMocks.setEnabled.mockResolvedValueOnce(result)
    await expect(store.setEnabled('com.easiflux.alpha', true)).resolves.toBe(true)
    expect(store.catalog[0].status).toBe('enabled')
    expect(store.revision).toBe('5')
    expect(store.pendingIds.size).toBe(0)
    expect(store.actionErrors).toEqual({})
  })

  it('an ignored identity mismatch preserves another item error and pending owner', async () => {
    const store = await loadedStore('4')
    serviceMocks.setEnabled.mockRejectedValueOnce({ code: 'plugin_not_found', message: 'private' })
    await store.setEnabled('com.easiflux.beta', false)
    const errorBefore = { ...store.actionErrors }
    const alphaResponse = deferred<PluginCatalogMutationResult>()
    serviceMocks.setEnabled.mockReturnValueOnce(alphaResponse.promise)
    const oldAlpha = store.setEnabled('com.easiflux.alpha', true)
    const invalid = mutation('5', 'com.easiflux.alpha', true)
    invalid.plugin.source = 'localDeclarative'
    alphaResponse.resolve(invalid)
    await expect(oldAlpha).resolves.toBe(false)
    expect(store.actionErrors).toEqual(errorBefore)

    const betaResponse = deferred<PluginCatalogMutationResult>()
    serviceMocks.setEnabled.mockReturnValueOnce(betaResponse.promise)
    const betaRequest = store.setEnabled('com.easiflux.beta', false)
    serviceMocks.setEnabled.mockResolvedValueOnce(invalid)
    await expect(store.setEnabled('com.easiflux.alpha', true)).resolves.toBe(false)
    expect(store.pendingIds).toEqual(new Set(['com.easiflux.beta']))
    expect(store.revision).toBe('4')
    betaResponse.resolve(mutation('5', 'com.easiflux.beta', false))
    await betaRequest
    expect(store.pendingIds.size).toBe(0)
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
