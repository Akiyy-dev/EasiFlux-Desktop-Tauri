import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import {
  cancelLocalManifestImport,
  commitLocalManifestImport,
  getPluginCatalog,
  pluginErrorCode,
  prepareLocalManifestImport,
  reloadPluginCatalog,
  removeManagedLocalPlugin,
  pluginErrorMessage,
  setPluginEnabled,
} from '../../src/services/pluginService'
import type {
  LocalManifestImportCommitFailure,
  ReadyLocalManifestImport,
} from '../../src/types/plugin'

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
    management: 'builtIn',
    canRemove: false,
    toggleBlockReasonCode: null,
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
    management: 'builtIn',
    canRemove: false,
    toggleBlockReasonCode: null,
    status: 'blocked',
    statusReasonCode: reason,
    canToggle: false,
    grantedCapabilities: [],
  }
}

function validSnapshot(): WireObject {
  return {
    schemaVersion: 3,
    revision: '42',
    catalogGeneration: '2',
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
    managedOwnership: { status: 'available', conflictingEntryCount: 0, rollbackPendingCount: 0, cleanupPendingCount: 0 },
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
    schemaVersion: 3,
    revision: '42',
    catalogGeneration: '2',
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
    managedOwnership: { status: 'available', conflictingEntryCount: 0, rollbackPendingCount: 0, cleanupPendingCount: 0 },
    availability: 'unavailable',
    availabilityReasonCode: reason,
    plugins: [blockedItem('com.easiflux.analytics', reason)],
  }
}

function validMutation(
  id = 'com.easiflux.analytics',
  status: 'enabled' | 'disabled' = 'enabled',
): WireObject {
  return { schemaVersion: 3, revision: '43', catalogGeneration: '2', plugin: validItem(id, status) }
}

function readyImport(
  manifest: WireObject = validManifest('com.example.notes', 'com.example'),
): WireObject {
  return {
    schemaVersion: 1,
    status: 'ready',
    token: 'a'.repeat(32),
    expiresInSeconds: 300,
    catalogGeneration: '2',
    manifest,
  }
}

function importedItem(
  manifest: WireObject = validManifest('com.example.notes', 'com.example'),
): WireObject {
  return {
    manifest,
    source: 'localDeclarative',
    management: 'managed',
    canRemove: true,
    toggleBlockReasonCode: null,
    status: 'disabled',
    statusReasonCode: null,
    canToggle: true,
    grantedCapabilities: [],
  }
}

function importSnapshot(
  manifest: WireObject = validManifest('com.example.notes', 'com.example'),
  localDiscoveryStatus: 'available' | 'degraded' = 'available',
): WireObject {
  return {
    schemaVersion: 3,
    revision: '43',
    catalogGeneration: '3',
    localDiscovery: {
      status: localDiscoveryStatus,
      rejectedPackageCount: localDiscoveryStatus === 'degraded' ? 1 : 0,
    },
    managedOwnership: { status: 'available', conflictingEntryCount: 0, rollbackPendingCount: 0, cleanupPendingCount: 0 },
    availability: 'available',
    availabilityReasonCode: null,
    plugins: [importedItem(manifest)],
  }
}

function importedResult(
  manifest: WireObject = validManifest('com.example.notes', 'com.example'),
): WireObject {
  return {
    schemaVersion: 2,
    status: 'imported',
    pluginId: 'com.example.notes',
    snapshot: importSnapshot(manifest),
  }
}

function notImportedResult(
  reasonCode: LocalManifestImportCommitFailure = 'plugin_import_id_conflict',
  disabledDecisionSaved = false,
): WireObject {
  return {
    schemaVersion: 2,
    status: 'notImported',
    disabledDecisionSaved,
    reasonCode,
    snapshot: validSnapshot(),
  }
}

function importedNotVisibleResult(snapshot: WireObject = validSnapshot()): WireObject {
  return {
    schemaVersion: 2,
    status: 'importedNotVisible',
    pluginId: 'com.example.notes',
    reasonCode: 'plugin_import_publication_unconfirmed',
    snapshot,
  }
}

async function parseReadyThroughPrepare(value: unknown): Promise<ReadyLocalManifestImport> {
  vi.mocked(tauriInvoke).mockResolvedValueOnce(value)
  const parsed = await prepareLocalManifestImport()
  if (parsed.status !== 'ready') throw new Error('ready preview required')
  return parsed
}

async function expectPrepareRejected(value: unknown): Promise<void> {
  vi.mocked(tauriInvoke).mockResolvedValueOnce(value)
  await expect(prepareLocalManifestImport()).rejects.toThrow(INVALID_RESPONSE_ERROR)
}

async function expectCancelRejected(value: unknown): Promise<void> {
  vi.mocked(tauriInvoke).mockResolvedValueOnce(value)
  await expect(cancelLocalManifestImport('a'.repeat(32))).rejects.toThrow(INVALID_RESPONSE_ERROR)
}

async function expectCommitRejected(value: unknown): Promise<void> {
  const preview = await parseReadyThroughPrepare(readyImport())
  vi.mocked(tauriInvoke).mockResolvedValueOnce(value)
  await expect(commitLocalManifestImport(preview)).rejects.toThrow(INVALID_RESPONSE_ERROR)
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
      schemaVersion: 3,
      revision: '4',
      catalogGeneration: '2',
      availability: 'available',
      availabilityReasonCode: null,
      localDiscovery: { status: 'degraded', rejectedPackageCount: 1 },
      managedOwnership: { status: 'available', conflictingEntryCount: 0, rollbackPendingCount: 0, cleanupPendingCount: 0 },
      plugins: [{ ...validItem('com.example.alpha'), source: 'localDeclarative', management: 'external' }],
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

  describe.each([
    ['get', getPluginCatalog],
    ['reload', reloadPluginCatalog],
  ] as const)('%s local discovery consistency', (_command, request) => {
    it.each(['available', 'unavailable'])('rejects local records with unavailable discovery when state is %s', async (availability) => {
      const snapshot = availability === 'available' ? validSnapshot() : unavailableSnapshot()
      snapshot.localDiscovery = { status: 'unavailable', rejectedPackageCount: 0 }
      firstItem(snapshot).source = 'localDeclarative'
      firstItem(snapshot).management = 'external'
      vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot)
      await expect(request()).rejects.toThrow(INVALID_RESPONSE_ERROR)
    })

    it.each(['available', 'unavailable'])('permits only built-ins with unavailable discovery when state is %s', async (availability) => {
      const snapshot = availability === 'available' ? validSnapshot() : unavailableSnapshot()
      snapshot.localDiscovery = { status: 'unavailable', rejectedPackageCount: 0 }
      vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot)
      await expect(request()).resolves.toEqual(snapshot)
    })

    it.each([
      { status: 'available', rejectedPackageCount: 0 },
      { status: 'degraded', rejectedPackageCount: 1 },
    ])('preserves discovered locals when state is unavailable and discovery is %j', async (summary) => {
      const snapshot = unavailableSnapshot()
      snapshot.localDiscovery = summary
      firstItem(snapshot).source = 'localDeclarative'
      firstItem(snapshot).management = 'external'
      vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot)
      await expect(request()).resolves.toEqual(snapshot)
    })
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
    localItem.management = 'external'
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

  it.each(['name', 'publisher', 'description'])('rejects Unicode whitespace and U+FEFF-only %s in get and reload', async (field) => {
    for (const blank of ['\uFEFF', ' \uFEFF\t\u0085\n', '\u0085']) {
      const snapshot = validSnapshot()
      manifestFromItem(firstItem(snapshot))[field] = blank
      for (const request of [getPluginCatalog, reloadPluginCatalog]) {
        vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot)
        await expect(request()).rejects.toThrow(INVALID_RESPONSE_ERROR)
      }
    }
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

describe('local manifest import transport validation', () => {
  beforeEach(() => {
    vi.mocked(tauriInvoke).mockReset()
  })

  it('invokes prepare without a frontend path and accepts native cancellation', async () => {
    const cancelled = { schemaVersion: 1, status: 'cancelled' }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(cancelled)

    await expect(prepareLocalManifestImport()).resolves.toEqual(cancelled)
    expect(tauriInvoke).toHaveBeenCalledTimes(1)
    expect(tauriInvoke).toHaveBeenCalledWith('prepare_local_manifest_import')
  })

  it('accepts an exact ready preview at the canonical u64 maximum', async () => {
    const ready = readyImport()
    ready.catalogGeneration = U64_MAX

    await expect(parseReadyThroughPrepare(ready)).resolves.toEqual(ready)
  })

  it('invokes cancel with only the opaque token and validates its result', async () => {
    const cancelled = { schemaVersion: 1, status: 'cancelled' }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(cancelled)

    await expect(cancelLocalManifestImport('a'.repeat(32))).resolves.toEqual(cancelled)
    expect(tauriInvoke).toHaveBeenCalledTimes(1)
    expect(tauriInvoke).toHaveBeenCalledWith('cancel_local_manifest_import', {
      token: 'a'.repeat(32),
    })
  })

  it('commits only a preview token and its bound generation', async () => {
    const manifest = validManifest('com.example.notes', 'com.example')
    const preview = {
      schemaVersion: 1,
      status: 'ready',
      token: 'a'.repeat(32),
      expiresInSeconds: 300,
      catalogGeneration: '2',
      manifest,
    }
    const parsed = await parseReadyThroughPrepare(preview)
    const result = notImportedResult()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(result)

    await expect(commitLocalManifestImport(parsed)).resolves.toEqual(result)
    expect(tauriInvoke).toHaveBeenLastCalledWith('commit_local_manifest_import', {
      token: 'a'.repeat(32),
      expectedCatalogGeneration: '2',
    })
    expect(tauriInvoke).toHaveBeenCalledTimes(2)
  })

  it('accepts imported only when the degraded authoritative snapshot has the exact disabled item', async () => {
    const preview = await parseReadyThroughPrepare(readyImport())
    const result = importedResult()
    const snapshot = result.snapshot as WireObject
    snapshot.localDiscovery = { status: 'degraded', rejectedPackageCount: 1 }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(result)

    await expect(commitLocalManifestImport(preview)).resolves.toEqual(result)
  })

  it.each([
    'plugin_catalog_stale',
    'plugin_catalog_invalid',
    'plugin_catalog_generation_exhausted',
    'plugin_state_unavailable',
    'plugin_state_persist_failed',
    'plugin_state_capacity_exceeded',
    'plugin_revision_exhausted',
    'plugin_import_id_conflict',
    'plugin_import_discovery_unavailable',
    'plugin_import_capacity_exceeded',
    'plugin_import_staging_capacity_exceeded',
    'plugin_import_write_failed',
  ] satisfies LocalManifestImportCommitFailure[])(
    'accepts the closed notImported reason %s before a disabled decision',
    async (reasonCode) => {
      const preview = await parseReadyThroughPrepare(readyImport())
      const result = notImportedResult(reasonCode)
      vi.mocked(tauriInvoke).mockResolvedValueOnce(result)

      await expect(commitLocalManifestImport(preview)).resolves.toEqual(result)
    },
  )

  it.each(['plugin_state_persist_failed', 'plugin_import_write_failed'] as const)('accepts partial notImported %s after a disabled write decision', async (reasonCode) => {
    const preview = await parseReadyThroughPrepare(readyImport())
    const result = notImportedResult(reasonCode, true)
    vi.mocked(tauriInvoke).mockResolvedValueOnce(result)

    await expect(commitLocalManifestImport(preview)).resolves.toEqual(result)
  })

  it('accepts importedNotVisible with an unavailable snapshot that omits the target', async () => {
    const preview = await parseReadyThroughPrepare(readyImport())
    const result = importedNotVisibleResult(unavailableSnapshot())
    vi.mocked(tauriInvoke).mockResolvedValueOnce(result)

    await expect(commitLocalManifestImport(preview)).resolves.toEqual(result)
  })

  it.each([
    ['cancelled', { schemaVersion: 1, status: 'cancelled', token: 'a'.repeat(32) }],
    ['ready', { ...readyImport(), privatePath: 'C:\\secret\\manifest.json' }],
  ])('rejects extra keys on a prepare %s branch', async (_branch, value) => {
    await expectPrepareRejected(value)
  })

  it.each([
    ['null envelope', null],
    ['array envelope', []],
    ['unknown status', { schemaVersion: 1, status: 'unknown' }],
    ['cancelled with wrong schema', { schemaVersion: 2, status: 'cancelled' }],
    ['ready with wrong schema', { ...readyImport(), schemaVersion: 2 }],
  ])('rejects a malformed prepare result: %s', async (_label, value) => {
    await expectPrepareRejected(value)
  })

  it.each(['schemaVersion', 'status'])(
    'rejects a missing prepare-cancelled %s',
    async (key) => {
      const value: WireObject = { schemaVersion: 1, status: 'cancelled' }
      delete value[key]
      await expectPrepareRejected(value)
    },
  )

  it.each([
    'schemaVersion',
    'status',
    'token',
    'expiresInSeconds',
    'catalogGeneration',
    'manifest',
  ])('rejects a missing prepare-ready %s', async (key) => {
    const value = readyImport()
    delete value[key]
    await expectPrepareRejected(value)
  })

  it.each([
    ['ttl below the fixed value', 299],
    ['ttl above the fixed value', 301],
    ['ttl as a string', '300'],
    ['ttl as null', null],
  ])('rejects %s', async (_label, expiresInSeconds) => {
    const value = readyImport()
    value.expiresInSeconds = expiresInSeconds
    await expectPrepareRejected(value)
  })

  it.each([
    ['uppercase', 'A'.repeat(32)],
    ['non-hex', 'g'.repeat(32)],
    ['31 characters', 'a'.repeat(31)],
    ['33 characters', 'a'.repeat(33)],
    ['non-string', 7],
  ])('rejects a ready token that is %s', async (_label, token) => {
    const value = readyImport()
    value.token = token
    await expectPrepareRejected(value)
  })

  it.each(['00', '+1', '18446744073709551616', '9'.repeat(10_000)])(
    'rejects a ready preview with noncanonical or overflowing generation %s',
    async (catalogGeneration) => {
      const value = readyImport()
      value.catalogGeneration = catalogGeneration
      await expectPrepareRejected(value)
    },
  )

  it('rejects an oversized ready generation before invoking BigInt', async () => {
    const value = readyImport()
    value.catalogGeneration = '9'.repeat(10_000)
    const bigIntSpy = vi.spyOn(globalThis, 'BigInt')

    try {
      await expectPrepareRejected(value)
      expect(bigIntSpy).not.toHaveBeenCalled()
    } finally {
      bigIntSpy.mockRestore()
    }
  })

  it.each([
    ['contributions', [{}]],
    ['requestedCapabilities', ['network']],
  ])('rejects a ready manifest with non-empty %s', async (field, content) => {
    const value = readyImport()
    const manifest = value.manifest as WireObject
    manifest[field] = content
    await expectPrepareRejected(value)
  })

  it('rejects an extra key on a cancel response', async () => {
    await expectCancelRejected({ schemaVersion: 1, status: 'cancelled', tokenAccepted: true })
  })

  it.each(['schemaVersion', 'status'])('rejects a missing cancel response %s', async (key) => {
    const value: WireObject = { schemaVersion: 1, status: 'cancelled' }
    delete value[key]
    await expectCancelRejected(value)
  })

  it.each([
    ['wrong schema', { schemaVersion: 2, status: 'cancelled' }],
    ['wrong status', { schemaVersion: 1, status: 'ready' }],
  ])('rejects a cancel response with %s', async (_label, value) => {
    await expectCancelRejected(value)
  })

  it.each([
    ['imported', () => importedResult()],
    ['notImported', () => notImportedResult()],
    ['importedNotVisible', () => importedNotVisibleResult()],
  ] as const)('rejects extra keys on a commit %s branch', async (_branch, makeResult) => {
    const value = makeResult()
    value.privatePath = 'C:\\secret\\manifest.json'
    await expectCommitRejected(value)
  })

  it.each([
    ['null envelope', null],
    ['array envelope', []],
    ['unknown status', { schemaVersion: 1, status: 'unknown' }],
    ['imported with wrong schema', { ...importedResult(), schemaVersion: 1 }],
    ['notImported with wrong schema', { ...notImportedResult(), schemaVersion: 1 }],
    [
      'importedNotVisible with wrong schema',
      { ...importedNotVisibleResult(), schemaVersion: 1 },
    ],
  ])('rejects a malformed commit result: %s', async (_label, value) => {
    await expectCommitRejected(value)
  })

  it.each([
    ['imported schemaVersion', () => importedResult(), 'schemaVersion'],
    ['imported status', () => importedResult(), 'status'],
    ['imported pluginId', () => importedResult(), 'pluginId'],
    ['imported snapshot', () => importedResult(), 'snapshot'],
    ['notImported schemaVersion', () => notImportedResult(), 'schemaVersion'],
    ['notImported status', () => notImportedResult(), 'status'],
    ['notImported disabledDecisionSaved', () => notImportedResult(), 'disabledDecisionSaved'],
    ['notImported reasonCode', () => notImportedResult(), 'reasonCode'],
    ['notImported snapshot', () => notImportedResult(), 'snapshot'],
    ['importedNotVisible schemaVersion', () => importedNotVisibleResult(), 'schemaVersion'],
    ['importedNotVisible status', () => importedNotVisibleResult(), 'status'],
    ['importedNotVisible pluginId', () => importedNotVisibleResult(), 'pluginId'],
    ['importedNotVisible reasonCode', () => importedNotVisibleResult(), 'reasonCode'],
    ['importedNotVisible snapshot', () => importedNotVisibleResult(), 'snapshot'],
  ] as const)('rejects a missing commit %s', async (_label, makeResult, key) => {
    const value = makeResult()
    delete value[key]
    await expectCommitRejected(value)
  })

  it.each([
    ['plugin id', (value: WireObject) => { value.pluginId = 'com.example.other' }],
    ['target item', (value: WireObject) => {
      const snapshot = value.snapshot as WireObject
      snapshot.plugins = [validItem('com.easiflux.analytics')]
    }],
    ['top-level availability', (value: WireObject) => {
      value.snapshot = unavailableSnapshot()
    }],
    ['local source', (value: WireObject) => {
      const snapshot = value.snapshot as WireObject
      firstItem(snapshot).source = 'builtIn'
    }],
    ['disabled status', (value: WireObject) => {
      const snapshot = value.snapshot as WireObject
      firstItem(snapshot).status = 'enabled'
    }],
  ])('rejects imported with the wrong %s', async (_label, mutate) => {
    const value = importedResult()
    mutate(value)
    await expectCommitRejected(value)
  })

  it.each([
    ['id', 'com.example.other'],
    ['publisherId', 'com.other'],
    ['publisher', 'Other Publisher'],
    ['name', 'Other Name'],
    ['description', 'Other description'],
    ['version', '1.2.4'],
  ])('rejects imported when manifest %s differs from the confirmed preview', async (field, changed) => {
    const value = importedResult()
    const snapshot = value.snapshot as WireObject
    manifestFromItem(firstItem(snapshot))[field] = changed
    await expectCommitRejected(value)
  })

  it.each([
    'plugin_catalog_stale',
    'plugin_catalog_invalid',
    'plugin_catalog_generation_exhausted',
    'plugin_state_unavailable',
    'plugin_state_capacity_exceeded',
    'plugin_revision_exhausted',
    'plugin_import_id_conflict',
    'plugin_import_discovery_unavailable',
    'plugin_import_capacity_exceeded',
    'plugin_import_staging_capacity_exceeded',
  ] satisfies LocalManifestImportCommitFailure[])(
    'rejects disabledDecisionSaved=true with pre-decision reason %s',
    async (reasonCode) => {
      await expectCommitRejected(notImportedResult(reasonCode, true))
    },
  )

  it.each([
    ['publication reason in notImported', 'plugin_import_publication_unconfirmed'],
    ['unknown reason', 'plugin_import_other'],
  ])('rejects %s', async (_label, reasonCode) => {
    const value = notImportedResult()
    value.reasonCode = reasonCode
    await expectCommitRejected(value)
  })

  it.each([null, 0, 'false'])('rejects non-boolean disabledDecisionSaved %j', async (saved) => {
    const value = notImportedResult()
    value.disabledDecisionSaved = saved
    await expectCommitRejected(value)
  })

  it.each([
    ['wrong publication reason', (value: WireObject) => {
      value.reasonCode = 'plugin_import_write_failed'
    }],
    ['wrong plugin id', (value: WireObject) => {
      value.pluginId = 'com.example.other'
    }],
  ])('rejects importedNotVisible with %s', async (_label, mutate) => {
    const value = importedNotVisibleResult()
    mutate(value)
    await expectCommitRejected(value)
  })

  it.each([
    ['imported', () => importedResult()],
    ['notImported', () => notImportedResult()],
    ['importedNotVisible', () => importedNotVisibleResult()],
  ] as const)('strictly parses nested snapshots for %s', async (_branch, makeResult) => {
    const value = makeResult()
    const snapshot = value.snapshot as WireObject
    snapshot.catalogGeneration = '00'
    await expectCommitRejected(value)
  })

  it('does not retry a rejected commit or fabricate a result', async () => {
    const preview = await parseReadyThroughPrepare(readyImport())
    const backendError = { code: 'plugin_import_token_invalid', message: 'expired' }
    vi.mocked(tauriInvoke).mockRejectedValueOnce(backendError)

    await expect(commitLocalManifestImport(preview)).rejects.toBe(backendError)
    expect(tauriInvoke).toHaveBeenCalledTimes(2)
  })
})

function ownershipSummary(overrides: WireObject = {}): WireObject {
  return { status: 'available', conflictingEntryCount: 0, rollbackPendingCount: 0, cleanupPendingCount: 0, ...overrides }
}

function localItem(management: string, status = 'disabled'): WireObject {
  return {
    ...validItem('com.example.notes'),
    source: 'localDeclarative', management, status,
    canRemove: management === 'managed' && status === 'disabled',
    canToggle: management !== 'removalPending',
    toggleBlockReasonCode: management === 'removalPending' ? 'removalPending' : null,
  }
}

function externalImportResult(): WireObject {
  const result = importedResult()
  const item = firstItem(result.snapshot as WireObject)
  item.management = 'external'
  item.canRemove = false
  return { ...result, status: 'importedExternal', reasonCode: 'plugin_import_ownership_not_registered' }
}

const FALSE_ONLY_REMOVE_CODES = [
  'plugin_catalog_stale', 'plugin_catalog_invalid', 'plugin_catalog_generation_exhausted',
  'plugin_state_unavailable', 'plugin_state_persist_failed', 'plugin_state_capacity_exceeded',
  'plugin_revision_exhausted', 'plugin_ownership_unavailable', 'plugin_ownership_capacity_exceeded',
  'plugin_ownership_revision_exhausted', 'plugin_ownership_conflict',
  'plugin_remove_discovery_unavailable', 'plugin_remove_not_managed', 'plugin_remove_requires_disabled',
  'plugin_remove_storage_unavailable', 'plugin_remove_staging_capacity_exceeded',
] as const
const TRUE_ONLY_REMOVE_CODES = ['plugin_ownership_persist_failed', 'plugin_remove_write_failed'] as const

function notRemoved(reasonCode = 'plugin_catalog_stale', disabledDecisionSaved = false): WireObject {
  return { schemaVersion: 1, status: 'notRemoved', reasonCode, disabledDecisionSaved, snapshot: validSnapshot() }
}

function removed(status = 'removed'): WireObject {
  const snapshot = validSnapshot()
  if (status === 'removedCleanupPending') {
    snapshot.managedOwnership = ownershipSummary({ status: 'degraded', cleanupPendingCount: 1 })
  }
  return {
    schemaVersion: 1, status, pluginId: 'com.example.notes', snapshot,
    ...(status === 'removedCatalogUnconfirmed' ? { reasonCode: 'plugin_remove_publication_unconfirmed' } : {}),
  }
}

async function expectRemoveRejected(value: unknown): Promise<void> {
  vi.mocked(tauriInvoke).mockResolvedValueOnce(value)
  await expect(removeManagedLocalPlugin('com.example.notes', '2')).rejects.toThrow(INVALID_RESPONSE_ERROR)
}

async function expectRemoveAccepted(value: unknown): Promise<void> {
  vi.mocked(tauriInvoke).mockResolvedValueOnce(value)
  await expect(removeManagedLocalPlugin('com.example.notes', '2')).resolves.toEqual(value)
}

describe('catalog v3 ownership invariants', () => {
  beforeEach(() => vi.mocked(tauriInvoke).mockReset())

  it.each(['management', 'canRemove', 'toggleBlockReasonCode'])('requires item %s', async (key) => {
    const snapshot = validSnapshot()
    delete firstItem(snapshot)[key]
    await expectCatalogRejected(snapshot)
  })

  it('requires v3 and the ownership summary on snapshots and mutations', async () => {
    await expectCatalogRejected({ ...validSnapshot(), schemaVersion: 2 })
    await expectMutationRejected({ ...validMutation(), schemaVersion: 2 })
    const snapshot = validSnapshot()
    delete snapshot.managedOwnership
    await expectCatalogRejected(snapshot)
  })

  it.each(['status', 'conflictingEntryCount', 'rollbackPendingCount', 'cleanupPendingCount'])('requires summary %s', async (key) => {
    const summary = ownershipSummary()
    delete summary[key]
    await expectCatalogRejected({ ...validSnapshot(), managedOwnership: summary })
  })

  it.each([
    null, [], {}, ownershipSummary({ extra: true }), ownershipSummary({ status: 'unknown' }),
    ownershipSummary({ status: 'degraded' }), ownershipSummary({ cleanupPendingCount: 1 }),
    ownershipSummary({ status: 'unavailable', conflictingEntryCount: 1 }),
    ...['conflictingEntryCount', 'rollbackPendingCount', 'cleanupPendingCount'].flatMap((field) => (
      [-1, 0.5, NaN, Infinity, '1', null, Number.MAX_SAFE_INTEGER + 1].map((count) => ownershipSummary({ status: 'degraded', [field]: count }))
    )),
    ownershipSummary({ status: 'degraded', conflictingEntryCount: 177 }),
    ownershipSummary({ status: 'degraded', rollbackPendingCount: 161 }),
    ownershipSummary({ status: 'degraded', cleanupPendingCount: 161 }),
    ownershipSummary({ status: 'degraded', rollbackPendingCount: 160, cleanupPendingCount: 17 }),
  ])('rejects malformed or inconsistent ownership summary %j', async (summary) => {
    await expectCatalogRejected({ ...validSnapshot(), managedOwnership: summary })
  })

  it.each([
    ownershipSummary(), ownershipSummary({ status: 'unavailable' }),
    ownershipSummary({ status: 'degraded', conflictingEntryCount: 176 }),
    ownershipSummary({ status: 'degraded', rollbackPendingCount: 160 }),
    ownershipSummary({ status: 'degraded', cleanupPendingCount: 160 }),
    ownershipSummary({ status: 'degraded', conflictingEntryCount: 16, rollbackPendingCount: 80, cleanupPendingCount: 80 }),
  ])('accepts bounded summary %j independently of plugin-state health', async (summary) => {
    for (const snapshot of [validSnapshot(), unavailableSnapshot()]) {
      snapshot.managedOwnership = summary
      vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot)
      await expect(getPluginCatalog()).resolves.toEqual(snapshot)
    }
  })

  it.each(['managed', 'external', 'ownershipConflict', 'ownershipUnavailable', 'removalPending'])('accepts local %s and the exact toggle/remove rules', async (management) => {
    for (const status of management === 'removalPending' ? ['disabled'] : ['enabled', 'disabled']) {
      const snapshot = { ...validSnapshot(), plugins: [localItem(management, status)] }
      vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot)
      await expect(getPluginCatalog()).resolves.toEqual(snapshot)
    }
  })

  it.each([
    { management: 'unknown' }, { management: 'external' }, { source: 'localDeclarative' },
    { canRemove: true }, { canRemove: 'false' }, { toggleBlockReasonCode: 'other' },
    { toggleBlockReasonCode: 'removalPending', canToggle: false },
  ])('rejects inconsistent built-in fields %j', async (fields) => {
    await expectCatalogRejected({ ...validSnapshot(), plugins: [{ ...validItem(), ...fields }] })
  })

  it.each([
    { management: 'managed', status: 'disabled', canRemove: false },
    { management: 'managed', status: 'enabled', canRemove: true },
    { management: 'external', canRemove: true },
    { management: 'ownershipConflict', canRemove: true },
    { management: 'ownershipUnavailable', canRemove: true },
    { management: 'removalPending', canRemove: true },
    { management: 'removalPending', toggleBlockReasonCode: null },
    { management: 'removalPending', canToggle: true },
    { management: 'removalPending', status: 'enabled' },
    { management: 'managed', toggleBlockReasonCode: 'removalPending', canToggle: false },
  ])('rejects inconsistent local ownership %j', async (fields) => {
    await expectCatalogRejected({ ...validSnapshot(), plugins: [{ ...localItem(fields.management), ...fields }] })
  })

  it.each(['managed', 'external', 'ownershipConflict', 'ownershipUnavailable'])('enforces global unavailable state for %s', async (management) => {
    const item = { ...localItem(management), status: 'blocked', statusReasonCode: 'stateUnavailable', canToggle: false, canRemove: false }
    const snapshot = { ...unavailableSnapshot(), plugins: [item] }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(snapshot)
    await expect(getPluginCatalog()).resolves.toEqual(snapshot)
    await expectCatalogRejected({ ...validSnapshot(), plugins: [item] })
    await expectCatalogRejected({ ...snapshot, plugins: [{ ...item, canRemove: true }] })
  })

  it('rejects removalPending unless disabled, including globally unavailable snapshots', async () => {
    const pending = { ...localItem('removalPending'), status: 'blocked', statusReasonCode: 'stateUnavailable' }
    await expectCatalogRejected({ ...unavailableSnapshot(), plugins: [pending] })
  })

  it('rejects a mutation that changes generation and permits status-derived canRemove at the same generation', async () => {
    await expectMutationRejected({ ...validMutation(), catalogGeneration: '3' })
    for (const enabled of [false, true]) {
      const result = { schemaVersion: 3, revision: enabled ? '45' : '44', catalogGeneration: '2', plugin: localItem('managed', enabled ? 'enabled' : 'disabled') }
      vi.mocked(tauriInvoke).mockResolvedValueOnce(result)
      await expect(setPluginEnabled('com.example.notes', enabled, '2')).resolves.toEqual(result)
    }
  })

  it('rejects a toggle-blocked mutation', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce({ ...validMutation(), plugin: localItem('removalPending') })
    await expect(setPluginEnabled('com.example.notes', false, '2')).rejects.toThrow(INVALID_RESPONSE_ERROR)
  })
})

describe('import v2 ownership outcomes', () => {
  beforeEach(() => vi.mocked(tauriInvoke).mockReset())

  it('accepts the exact external import outcome', async () => {
    const preview = await parseReadyThroughPrepare(readyImport())
    const result = externalImportResult()
    vi.mocked(tauriInvoke).mockResolvedValueOnce(result)
    await expect(commitLocalManifestImport(preview)).resolves.toEqual(result)
  })

  it.each(['plugin_ownership_unavailable', 'plugin_ownership_capacity_exceeded', 'plugin_ownership_revision_exhausted'] as const)('permits preflight %s only before the disabled decision', async (reasonCode) => {
    const preview = await parseReadyThroughPrepare(readyImport())
    const result = { ...notImportedResult(), reasonCode }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(result)
    await expect(commitLocalManifestImport(preview)).resolves.toEqual(result)
    await expectCommitRejected({ ...result, disabledDecisionSaved: true })
  })

  it.each(['plugin_ownership_persist_failed', 'plugin_import_ownership_not_registered'])('rejects post-promotion %s from notImported', async (reasonCode) => {
    for (const disabledDecisionSaved of [false, true]) {
      await expectCommitRejected({ ...notImportedResult(), reasonCode, disabledDecisionSaved })
    }
  })

  it('rejects interchangeable managed/external claims even with individually valid snapshots', async () => {
    const external = externalImportResult()
    await expectCommitRejected({ ...importedResult(), snapshot: external.snapshot })
    await expectCommitRejected({ ...external, snapshot: importedResult().snapshot })
  })

  it('requires every exact external branch key and rejects extras, schema 1, wrong reason, target and manifest', async () => {
    for (const key of Object.keys(externalImportResult())) {
      const value = externalImportResult()
      delete value[key]
      await expectCommitRejected(value)
    }
    for (const fields of [
      { extra: true }, { schemaVersion: 1 }, { reasonCode: 'plugin_import_publication_unconfirmed' },
      { pluginId: 'com.example.other' }, { snapshot: validSnapshot() },
    ]) await expectCommitRejected({ ...externalImportResult(), ...fields })
    const value = externalImportResult()
    manifestFromItem(firstItem(value.snapshot as WireObject)).version = '9.0.0'
    await expectCommitRejected(value)
    const invalidNested = externalImportResult()
    ;(invalidNested.snapshot as WireObject).managedOwnership = ownershipSummary({ status: 'degraded' })
    await expectCommitRejected(invalidNested)
  })
})

describe('removal v1 transport validation', () => {
  beforeEach(() => vi.mocked(tauriInvoke).mockReset())

  it('invokes only the removal command with the exact ID and generation payload', async () => {
    await expectRemoveAccepted(removed())
    expect(tauriInvoke).toHaveBeenCalledExactlyOnceWith('remove_managed_local_plugin', { id: 'com.example.notes', expectedCatalogGeneration: '2' })
  })

  it('accepts every legal closed failure pairing', async () => {
    for (const code of FALSE_ONLY_REMOVE_CODES) await expectRemoveAccepted(notRemoved(code, false))
    for (const code of TRUE_ONLY_REMOVE_CODES) await expectRemoveAccepted(notRemoved(code, true))
    await expectRemoveAccepted(notRemoved('plugin_remove_identity_changed', false))
    await expectRemoveAccepted(notRemoved('plugin_remove_identity_changed', true))
  })

  it('rejects every illegal remove code and disabledDecisionSaved pairing', async () => {
    for (const code of FALSE_ONLY_REMOVE_CODES) await expectRemoveRejected(notRemoved(code, true))
    for (const code of TRUE_ONLY_REMOVE_CODES) await expectRemoveRejected(notRemoved(code, false))
    for (const code of ['plugin_remove_publication_unconfirmed', 'plugin_import_write_failed', 'plugin_remove_other', 'plugin_not_found']) {
      for (const saved of [false, true]) await expectRemoveRejected(notRemoved(code, saved))
    }
  })

  it.each([null, 'false', 0])('rejects non-boolean disabledDecisionSaved %j', async (saved) => {
    await expectRemoveRejected({ ...notRemoved(), disabledDecisionSaved: saved })
  })

  it.each(['removed', 'removedCleanupPending', 'removedCatalogUnconfirmed', 'notRemoved'])('validates every exact %s branch and all nested snapshot fields atomically', async (status) => {
    const makeResult = () => status === 'notRemoved' ? notRemoved() : removed(status)
    await expectRemoveAccepted(makeResult())
    await expectRemoveRejected({ ...makeResult(), extra: true })
    await expectRemoveRejected({ ...makeResult(), schemaVersion: 2 })
    for (const key of Object.keys(makeResult())) {
      const value = makeResult()
      delete value[key]
      await expectRemoveRejected(value)
    }
    for (const fields of [{ catalogGeneration: '00' }, { schemaVersion: 2 }, { managedOwnership: ownershipSummary({ status: 'degraded' }) }]) {
      const value = makeResult()
      value.snapshot = { ...(value.snapshot as WireObject), ...fields }
      await expectRemoveRejected(value)
    }
  })

  it('forbids pluginId on notRemoved and binds every other branch to the requested ID', async () => {
    await expectRemoveRejected({ ...notRemoved(), pluginId: 'com.example.notes' })
    for (const status of ['removed', 'removedCleanupPending', 'removedCatalogUnconfirmed']) {
      await expectRemoveRejected({ ...removed(status), pluginId: 'com.example.other' })
      await expectRemoveRejected({ ...removed(status), pluginId: 'invalid' })
    }
  })

  it('rejects a same-ID managed or removalPending item in confirmed outcomes but permits external replacement', async () => {
    for (const status of ['removed', 'removedCleanupPending']) {
      for (const management of ['managed', 'removalPending', 'external']) {
        const value = removed(status)
        ;(value.snapshot as WireObject).plugins = [localItem(management)]
        if (management === 'external') await expectRemoveAccepted(value)
        else await expectRemoveRejected(value)
      }
    }
  })

  it('requires a nonzero cleanup count and the sole unconfirmed publication reason', async () => {
    await expectRemoveRejected({ ...removed('removedCleanupPending'), snapshot: validSnapshot() })
    for (const reasonCode of ['plugin_remove_write_failed', 'plugin_import_publication_unconfirmed', null]) {
      await expectRemoveRejected({ ...removed('removedCatalogUnconfirmed'), reasonCode })
    }
    const value = removed('removedCatalogUnconfirmed')
    ;(value.snapshot as WireObject).plugins = [localItem('managed')]
    await expectRemoveAccepted(value)
  })

  it.each([null, [], {}, { schemaVersion: 1, status: 'unknown' }])('rejects invalid envelopes %j', async (value) => {
    await expectRemoveRejected(value)
  })

  it('does not retry transport failure or adopt an invalid result snapshot', async () => {
    const failure = { code: 'plugin_remove_write_failed', message: 'private path' }
    vi.mocked(tauriInvoke).mockRejectedValueOnce(failure)
    await expect(removeManagedLocalPlugin('com.example.notes', '2')).rejects.toBe(failure)
    const adopt = vi.fn()
    vi.mocked(tauriInvoke).mockResolvedValueOnce({ ...removed(), extra: 'private path' })
    await expect(removeManagedLocalPlugin('com.example.notes', '2').then(adopt)).rejects.toThrow(INVALID_RESPONSE_ERROR)
    expect(adopt).not.toHaveBeenCalled()
    expect(tauriInvoke).toHaveBeenCalledTimes(2)
  })
})

describe('ownership and removal error sanitization', () => {
  it.each([
    ...FALSE_ONLY_REMOVE_CODES, ...TRUE_ONLY_REMOVE_CODES, 'plugin_remove_identity_changed',
    'plugin_remove_publication_unconfirmed', 'plugin_import_ownership_not_registered',
  ])('maps %s locally without exposing backend messages', (code) => {
    expect(pluginErrorCode({ code, message: 'C:\\secret\\receipt.json' })).toBe(code)
    const message = pluginErrorMessage({ code, message: 'C:\\secret\\receipt.json' })
    expect(message).not.toBe(GENERIC_ERROR)
    expect(message).toMatch(/[\u4e00-\u9fff]/)
    expect(message).not.toContain('secret')
    expect(pluginErrorCode({ code, message: 'secret', extra: true })).toBeNull()
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
    ['plugin_import_busy', '已有清单导入操作，请先完成或取消。'],
    ['plugin_import_dialog_unavailable', '无法打开文件选择器，请重试。'],
    ['plugin_import_source_rejected', '无法安全读取所选文件，请选择普通本地 JSON 文件。'],
    ['plugin_import_manifest_invalid', '清单格式或内容不符合当前插件要求。'],
    ['plugin_import_token_invalid', '预览已过期或失效，请重新选择清单。'],
    ['plugin_import_id_conflict', '已存在相同插件 ID；当前不支持覆盖或更新。'],
    ['plugin_import_discovery_unavailable', '请先修复本地插件发现问题，再导入清单。'],
    ['plugin_import_capacity_exceeded', '本地插件数量或读取预算已达上限。'],
    ['plugin_import_staging_capacity_exceeded', '导入暂存区需要人工检查和清理。'],
    ['plugin_import_write_failed', '无法完成清单写入，请检查后重试。'],
    ['plugin_import_publication_unconfirmed', '清单已写入，但目录结果尚未确认，请重新扫描。'],
  ])('maps known closed code %s without reflecting the backend message', (code, expected) => {
    const backendMessage = 'C:\\Users\\secret\\plugins\\state.json: raw JSON'
    const message = pluginErrorMessage({ code, message: backendMessage })

    expect(message).toBe(expected)
    expect(message).not.toContain('secret')
    expect(message).not.toContain('state.json')
  })

  it.each([
    'plugin_invalid_id',
    'plugin_not_found',
    'plugin_catalog_invalid',
    'plugin_state_unavailable',
    'plugin_state_persist_failed',
    'plugin_revision_exhausted',
    'plugin_not_toggleable',
    'plugin_catalog_stale',
    'plugin_catalog_generation_exhausted',
    'plugin_state_capacity_exceeded',
    'plugin_import_busy',
    'plugin_import_dialog_unavailable',
    'plugin_import_source_rejected',
    'plugin_import_manifest_invalid',
    'plugin_import_token_invalid',
    'plugin_import_id_conflict',
    'plugin_import_discovery_unavailable',
    'plugin_import_capacity_exceeded',
    'plugin_import_staging_capacity_exceeded',
    'plugin_import_write_failed',
    'plugin_import_publication_unconfirmed',
  ])('returns the known exact code %s from a valid transport error', (code) => {
    expect(pluginErrorCode({ code, message: 'untrusted backend text' })).toBe(code)
  })

  it.each([
    ['unknown code', { code: 'plugin_other', message: 'secret' }],
    ['extra key', { code: 'plugin_not_found', message: 'secret', path: 'C:\\secret' }],
    ['missing code', { message: 'secret' }],
    ['missing message', { code: 'plugin_not_found' }],
    ['non-string code', { code: 7, message: 'secret' }],
    ['non-string message', { code: 'plugin_not_found', message: 7 }],
    ['raw Error', new Error('C:\\secret\\stack')],
    ['raw string', '{"path":"C:\\\\secret"}'],
    ['null', null],
  ])('rejects %s as an error-code transport', (_label, error) => {
    expect(pluginErrorCode(error)).toBeNull()
  })

  it.each([
    ['extra field', { code: 'plugin_not_found', message: 'secret', path: 'C:\\secret' }],
    ['unknown code', { code: 'plugin_other', message: 'secret' }],
    ['missing code', { message: 'secret' }],
    ['missing message', { code: 'plugin_not_found' }],
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
