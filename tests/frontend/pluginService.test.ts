import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import {
  getPluginCatalog,
  reloadPluginCatalog,
  pluginErrorMessage,
  setPluginEnabled,
} from '../../src/services/pluginService'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

type WireObject = Record<string, unknown>

const U64_MAX = '18446744073709551615'
const GENERIC_ERROR = '插件操作失败，请重试。'
const INVALID_RESPONSE_ERROR = '插件服务返回的数据无效，请重试。'

function validManifest(
  id = 'com.easiflux.analytics',
  publisherId = 'com.easiflux',
): WireObject {
  return {
    schemaVersion: 1,
    id,
    publisherId,
    publisher: 'EasiFlux',
    name: 'Analytics',
    description: 'Analytics commands',
    version: '1.2.3',
    contributions: [],
    requestedCapabilities: [],
  }
}

function validItem(
  id = 'com.easiflux.analytics',
  status: 'enabled' | 'disabled' = 'disabled',
): WireObject {
  return {
    manifest: validManifest(id),
    source: 'builtIn',
    status,
    statusReasonCode: null,
    canToggle: true,
    grantedCapabilities: [],
  }
}

function blockedItem(
  id = 'com.easiflux.analytics',
  reason: 'stateUnavailable' | 'catalogInvalid' = 'stateUnavailable',
): WireObject {
  return {
    manifest: validManifest(id),
    source: 'builtIn',
    status: 'blocked',
    statusReasonCode: reason,
    canToggle: false,
    grantedCapabilities: [],
  }
}

function validSnapshot(): WireObject {
  return {
    schemaVersion: 2,
    revision: '42',
    catalogGeneration: '2',
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
    availability: 'available',
    availabilityReasonCode: null,
    plugins: [
      validItem('com.easiflux.analytics', 'enabled'),
      validItem('com.easiflux.orders', 'disabled'),
    ],
  }
}

function unavailableSnapshot(
  reason: 'stateUnavailable' | 'catalogInvalid' = 'stateUnavailable',
): WireObject {
  return {
    schemaVersion: 2,
    revision: '42',
    catalogGeneration: '2',
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
    availability: 'unavailable',
    availabilityReasonCode: reason,
    plugins: [blockedItem('com.easiflux.analytics', reason)],
  }
}

function validMutation(
  id = 'com.easiflux.analytics',
  status: 'enabled' | 'disabled' = 'enabled',
): WireObject {
  return { schemaVersion: 2, revision: '43', catalogGeneration: '2', plugin: validItem(id, status) }
}

function firstItem(snapshot: WireObject): WireObject {
  return (snapshot.plugins as WireObject[])[0]
}

function manifestFromItem(item: WireObject): WireObject {
  return item.manifest as WireObject
}

async function expectCatalogRejected(value: unknown): Promise<void> {
  vi.mocked(tauriInvoke).mockResolvedValueOnce(value)
  await expect(getPluginCatalog()).rejects.toThrow(INVALID_RESPONSE_ERROR)
}

async function expectMutationRejected(value: unknown): Promise<void> {
  vi.mocked(tauriInvoke).mockResolvedValueOnce(value)
  await expect(setPluginEnabled('com.easiflux.analytics', true, '2')).rejects.toThrow(
    INVALID_RESPONSE_ERROR,
  )
}

describe('plugin service transport validation', () => {
  beforeEach(() => {
    vi.mocked(tauriInvoke).mockReset()
  })

  it('invokes the fixed catalog command and accepts a complete valid snapshot', async () => {
    const snapshot = validSnapshot()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot)

    await expect(getPluginCatalog()).resolves.toEqual(snapshot)
    expect(tauriInvoke).toHaveBeenCalledWith('get_plugin_catalog')
  })

  it('accepts local declarative records and uses the fixed explicit reload command', async () => {
    const snapshotV2 = {
      schemaVersion: 2,
      revision: '4',
      catalogGeneration: '2',
      availability: 'available',
      availabilityReasonCode: null,
      localDiscovery: { status: 'degraded', rejectedPackageCount: 1 },
      plugins: [{ ...validItem('com.example.alpha'), source: 'localDeclarative' }],
    }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshotV2)
    await expect(reloadPluginCatalog()).resolves.toEqual(snapshotV2)
    expect(tauriInvoke).toHaveBeenCalledWith('reload_plugin_catalog')
  })

  it.each([
    { status: 'available', rejectedPackageCount: 0 },
    { status: 'degraded', rejectedPackageCount: 1 },
    { status: 'degraded', rejectedPackageCount: 256 },
    { status: 'unavailable', rejectedPackageCount: 0 },
  ])('accepts the bounded correlated local summary %j independently of top-level health', async (summary) => {
    const snapshot = validSnapshot()
    snapshot.localDiscovery = summary
    snapshot.catalogGeneration = U64_MAX
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot)
    await expect(getPluginCatalog()).resolves.toEqual(snapshot)
  })

  it.each([
    null, [], {},
    { status: 'unknown', rejectedPackageCount: 0 },
    { status: 'degraded', rejectedPackageCount: -1 },
    { status: 'degraded', rejectedPackageCount: 1.5 },
    { status: 'degraded', rejectedPackageCount: 257 },
    { status: 'degraded', rejectedPackageCount: '1' },
    { status: 'degraded', rejectedPackageCount: true },
    { status: 'degraded', rejectedPackageCount: NaN },
    { status: 'degraded', rejectedPackageCount: Infinity },
    { status: 'degraded', rejectedPackageCount: 0 },
    { status: 'available', rejectedPackageCount: 1 },
    { status: 'unavailable', rejectedPackageCount: 1 },
    { status: 'available', rejectedPackageCount: 0, path: 'private' },
    { status: 'available' },
  ])('rejects malformed or contradictory local summary %j', async (summary) => {
    const snapshot = validSnapshot()
    snapshot.localDiscovery = summary
    await expectCatalogRejected(snapshot)
  })

  it.each([1, null, '', '00', '01', '+1', '-1', '1.0', '1e2', ' 1', '1 ', '18446744073709551616', '9'.repeat(100)])(
    'rejects noncanonical generation in both envelopes: %j', async (generation) => {
      const snapshot = validSnapshot()
      snapshot.catalogGeneration = generation
      await expectCatalogRejected(snapshot)
      const mutation = validMutation()
      mutation.catalogGeneration = generation
      await expectMutationRejected(mutation)
    },
  )

  it.each(['schemaVersion', 'catalogGeneration', 'localDiscovery'])('rejects a missing snapshot %s', async (key) => {
    const snapshot = validSnapshot()
    delete snapshot[key]
    await expectCatalogRejected(snapshot)
  })

  it.each(['schemaVersion', 'catalogGeneration', 'revision', 'plugin'])('rejects a missing mutation %s', async (key) => {
    const mutation = validMutation()
    delete mutation[key]
    await expectMutationRejected(mutation)
  })

  it('rejects a v1 mutation envelope', async () => {
    const mutation = validMutation()
    mutation.schemaVersion = 1
    await expectMutationRejected(mutation)
  })

  it('validates reload responses with the same strict parser', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce({ ...validSnapshot(), privatePath: 'secret' })
    await expect(reloadPluginCatalog()).rejects.toThrow(INVALID_RESPONSE_ERROR)
  })

  it('invokes the fixed mutation command with the captured catalog generation', async () => {
    const mutation = validMutation()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(mutation)

    await expect(setPluginEnabled('com.easiflux.analytics', true, '2')).resolves.toEqual(mutation)
    expect(tauriInvoke).toHaveBeenCalledWith('set_plugin_enabled', {
      id: 'com.easiflux.analytics',
      enabled: true,
      expectedCatalogGeneration: '2',
    })
  })

  it('accepts a local mutation without changing its source or generation payload', async () => {
    const mutation = validMutation('com.example.alpha')
    const localItem = mutation.plugin as WireObject
    localItem.source = 'localDeclarative'
    vi.mocked(tauriInvoke).mockResolvedValueOnce(mutation)
    await expect(setPluginEnabled('com.example.alpha', true, '2')).resolves.toEqual(mutation)
    expect(tauriInvoke).toHaveBeenCalledWith('set_plugin_enabled', {
      id: 'com.example.alpha',
      enabled: true,
      expectedCatalogGeneration: '2',
    })
  })

  it('accepts the u64 maximum, identifier boundaries, UTF-8 text bounds, and SemVer metadata', async () => {
    const longestId = `${'a'.repeat(63)}.${'b'.repeat(62)}.c`
    const longestPublisherId = `${'c'.repeat(63)}.${'d'.repeat(62)}.e`
    const snapshot = validSnapshot()
    snapshot.revision = U64_MAX
    snapshot.plugins = [validItem(longestId)]
    const manifest = manifestFromItem(firstItem(snapshot))
    manifest.publisherId = longestPublisherId
    manifest.name = '界'.repeat(26) + 'ab'
    manifest.publisher = '发'.repeat(26) + 'ab'
    manifest.description = '述'.repeat(166) + 'ab'
    manifest.version = '1.2.3-alpha.1+linux-x64'
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot)

    await expect(getPluginCatalog()).resolves.toEqual(snapshot)
  })

  it.each([
    ['snapshot', (snapshot: WireObject) => { snapshot.extra = true }],
    ['catalog item', (snapshot: WireObject) => { firstItem(snapshot).extra = true }],
    ['manifest', (snapshot: WireObject) => { manifestFromItem(firstItem(snapshot)).extra = true }],
  ])('rejects unknown keys on the %s object', async (_label, mutate) => {
    const snapshot = validSnapshot()
    mutate(snapshot)
    await expectCatalogRejected(snapshot)
  })

  it('rejects an unknown key on a mutation envelope', async () => {
    const mutation = validMutation()
    mutation.extra = true
    await expectMutationRejected(mutation)
  })

  it.each([
    ['snapshot availabilityReasonCode', (snapshot: WireObject) => {
      delete snapshot.availabilityReasonCode
    }],
    ['item statusReasonCode', (snapshot: WireObject) => {
      delete firstItem(snapshot).statusReasonCode
    }],
  ])('rejects a missing required nullable field: %s', async (_label, mutate) => {
    const snapshot = validSnapshot()
    mutate(snapshot)
    await expectCatalogRejected(snapshot)
  })

  it.each([
    ['snapshot schema', (snapshot: WireObject) => { snapshot.schemaVersion = 1 }],
    ['manifest schema', (snapshot: WireObject) => {
      manifestFromItem(firstItem(snapshot)).schemaVersion = '1'
    }],
    ['source', (snapshot: WireObject) => { firstItem(snapshot).source = 'local' }],
    ['status', (snapshot: WireObject) => { firstItem(snapshot).status = 'loading' }],
    ['availability', (snapshot: WireObject) => { snapshot.availability = 'loading' }],
    ['availability reason', (snapshot: WireObject) => {
      snapshot.availabilityReasonCode = 'policyBlocked'
    }],
    ['item reason', (snapshot: WireObject) => {
      firstItem(snapshot).statusReasonCode = 'policyBlocked'
    }],
  ])('rejects an invalid %s enum or schema value', async (_label, mutate) => {
    const snapshot = validSnapshot()
    mutate(snapshot)
    await expectCatalogRejected(snapshot)
  })

  it.each([
    1,
    '',
    '00',
    '01',
    '+1',
    '-1',
    '1.0',
    ' 1',
    '18446744073709551616',
  ])('rejects a non-canonical or overflowing revision: %j', async (revision) => {
    const snapshot = validSnapshot()
    snapshot.revision = revision
    await expectCatalogRejected(snapshot)
    const mutation = validMutation()
    mutation.revision = revision
    await expectMutationRejected(mutation)
  })

  it('rejects an oversized revision without attempting unbounded BigInt parsing', async () => {
    const snapshot = validSnapshot()
    snapshot.revision = '9'.repeat(10_000)
    const bigIntSpy = vi.spyOn(globalThis, 'BigInt')

    try {
      await expectCatalogRejected(snapshot)
      expect(bigIntSpy).not.toHaveBeenCalled()
    } finally {
      bigIntSpy.mockRestore()
    }
  })

  it.each([
    '1',
    '01.2.3',
    '1.02.3',
    '1.2.03',
    '1.2.3-01',
    '1.2.3-',
    '1.2.3+',
    'v1.2.3',
    '1.2.3 alpha',
    '18446744073709551616.0.0',
    '0.18446744073709551616.0',
    '0.0.18446744073709551616',
  ])('rejects invalid SemVer %s', async (version) => {
    const snapshot = validSnapshot()
    manifestFromItem(firstItem(snapshot)).version = version
    await expectCatalogRejected(snapshot)
  })

  it('accepts u64 maximum values for all three SemVer core components', async () => {
    const snapshot = validSnapshot()
    manifestFromItem(firstItem(snapshot)).version = `${U64_MAX}.${U64_MAX}.${U64_MAX}`
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot)

    await expect(getPluginCatalog()).resolves.toEqual(snapshot)
  })

  it.each([
    ['plugin id uppercase', 'id', 'com.EasiFlux.analytics'],
    ['plugin id with one segment', 'id', 'analytics'],
    ['plugin id with an empty segment', 'id', 'com..analytics'],
    ['plugin id with an edge hyphen', 'id', 'com.-analytics'],
    ['plugin id with non-ASCII', 'id', 'com.分析'],
    ['plugin id oversized segment', 'id', `com.${'a'.repeat(64)}`],
    ['plugin id oversized total', 'id', `${'a'.repeat(63)}.${'b'.repeat(63)}.a`],
    ['publisher id uppercase', 'publisherId', 'com.EasiFlux'],
    ['publisher id with one segment', 'publisherId', 'easiflux'],
  ])('rejects %s', async (_label, field, invalid) => {
    const snapshot = validSnapshot()
    manifestFromItem(firstItem(snapshot))[field] = invalid
    await expectCatalogRejected(snapshot)
  })

  it.each([
    ['blank name', 'name', '   '],
    ['blank publisher', 'publisher', '\n\t'],
    ['blank description', 'description', '  '],
    ['name over 80 UTF-8 bytes', 'name', '界'.repeat(27)],
    ['publisher over 80 UTF-8 bytes', 'publisher', '发'.repeat(27)],
    ['description over 500 UTF-8 bytes', 'description', '述'.repeat(167)],
    ['version over 64 UTF-8 bytes', 'version', `1.2.3-${'a'.repeat(59)}`],
  ])('rejects %s', async (_label, field, invalid) => {
    const snapshot = validSnapshot()
    manifestFromItem(firstItem(snapshot))[field] = invalid
    await expectCatalogRejected(snapshot)
  })

  it.each([
    ['non-empty contributions', 'manifest', 'contributions', [{}]],
    ['non-array contributions', 'manifest', 'contributions', {}],
    ['non-empty requested capabilities', 'manifest', 'requestedCapabilities', ['network']],
    ['non-array requested capabilities', 'manifest', 'requestedCapabilities', null],
    ['non-empty granted capabilities', 'item', 'grantedCapabilities', ['network']],
    ['non-array granted capabilities', 'item', 'grantedCapabilities', 'none'],
  ])('rejects %s', async (_label, target, field, invalid) => {
    const snapshot = validSnapshot()
    const item = firstItem(snapshot)
    const object = target === 'manifest' ? manifestFromItem(item) : item
    object[field] = invalid
    await expectCatalogRejected(snapshot)
  })

  it.each([
    ['enabled with false canToggle', (item: WireObject) => { item.canToggle = false }],
    ['disabled with a reason', (item: WireObject) => {
      item.status = 'disabled'
      item.statusReasonCode = 'stateUnavailable'
    }],
    ['blocked with true canToggle', (item: WireObject) => {
      item.status = 'blocked'
      item.canToggle = true
      item.statusReasonCode = 'stateUnavailable'
    }],
    ['blocked without a reason', (item: WireObject) => {
      item.status = 'blocked'
      item.canToggle = false
      item.statusReasonCode = null
    }],
  ])('rejects contradictory item state: %s', async (_label, mutate) => {
    const snapshot = validSnapshot()
    mutate(firstItem(snapshot))
    await expectCatalogRejected(snapshot)
  })

  it('accepts a consistent unavailable snapshot', async () => {
    const snapshot = unavailableSnapshot('catalogInvalid')
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot)
    await expect(getPluginCatalog()).resolves.toEqual(snapshot)
  })

  it.each([
    ['available snapshot with a reason', (snapshot: WireObject) => {
      snapshot.availabilityReasonCode = 'stateUnavailable'
    }],
    ['available snapshot with a blocked item', (snapshot: WireObject) => {
      snapshot.plugins = [blockedItem()]
    }],
    ['unavailable snapshot without a reason', (snapshot: WireObject) => {
      snapshot.availabilityReasonCode = null
    }],
    ['unavailable snapshot with a toggleable item', (snapshot: WireObject) => {
      snapshot.plugins = [validItem()]
    }],
    ['unavailable snapshot with mismatched item reason', (snapshot: WireObject) => {
      firstItem(snapshot).statusReasonCode = 'catalogInvalid'
    }],
  ])('rejects inconsistent snapshot state: %s', async (label, mutate) => {
    const snapshot = label.startsWith('available') ? validSnapshot() : unavailableSnapshot()
    mutate(snapshot)
    await expectCatalogRejected(snapshot)
  })

  it.each([
    [
      'duplicate ids',
      [validItem('com.easiflux.analytics'), validItem('com.easiflux.analytics')],
    ],
    [
      'non-increasing ids',
      [validItem('com.easiflux.orders'), validItem('com.easiflux.analytics')],
    ],
  ])('rejects catalog %s', async (_label, plugins) => {
    const snapshot = validSnapshot()
    snapshot.plugins = plugins
    await expectCatalogRejected(snapshot)
  })

  it.each([
    ['blocked mutation item', (() => {
      const mutation = validMutation()
      mutation.plugin = blockedItem()
      return mutation
    })()],
    ['wrong plugin id', validMutation('com.easiflux.orders')],
    ['wrong resulting status', validMutation('com.easiflux.analytics', 'disabled')],
  ])('rejects a %s', async (_label, mutation) => {
    await expectMutationRejected(mutation)
  })
})

describe('plugin error sanitization', () => {
  it.each([
    ['plugin_catalog_stale', '插件目录已更新，请刷新后重试。'],
    ['plugin_catalog_generation_exhausted', '插件目录版本已达到上限，请联系支持。'],
    ['plugin_state_capacity_exceeded', '插件状态容量已达到上限，请联系支持。'],
    ['plugin_invalid_id', '插件标识无效。'],
    ['plugin_not_found', '未找到该插件。'],
    ['plugin_catalog_invalid', '插件目录不可用，请稍后重试。'],
    ['plugin_state_unavailable', '插件状态暂不可用，请重试。'],
    ['plugin_state_persist_failed', '保存插件状态失败，请重试。'],
    ['plugin_revision_exhausted', '插件状态版本已达到上限，请联系支持。'],
    ['plugin_not_toggleable', '该插件当前无法更改启用状态。'],
  ])('maps known closed code %s without reflecting the backend message', (code, expected) => {
    const backendMessage = 'C:\\Users\\secret\\plugins\\state.json: raw JSON'
    const message = pluginErrorMessage({ code, message: backendMessage })

    expect(message).toBe(expected)
    expect(message).not.toContain('secret')
    expect(message).not.toContain('state.json')
  })

  it.each([
    ['extra field', { code: 'plugin_not_found', message: 'secret', path: 'C:\\secret' }],
    ['unknown code', { code: 'plugin_other', message: 'secret' }],
    ['non-string code', { code: 7, message: 'secret' }],
    ['non-string message', { code: 'plugin_not_found', message: 7 }],
    ['raw Error', new Error('C:\\secret\\stack')],
    ['raw string', '{"path":"C:\\\\secret"}'],
    ['stack object', { stack: 'C:\\secret', message: 'raw' }],
    ['null', null],
  ])('uses the fixed generic copy for %s', (_label, error) => {
    const message = pluginErrorMessage(error)

    expect(message).toBe(GENERIC_ERROR)
    expect(message).not.toContain('secret')
    expect(message).not.toContain('stack')
  })
})
