import { computed, ref, shallowRef } from 'vue'
import { defineStore } from 'pinia'
import {
  cancelLocalManifestImport,
  commitLocalManifestImport,
  getPluginCatalog,
  pluginErrorCode,
  pluginErrorMessage,
  prepareLocalManifestImport,
  reloadPluginCatalog,
  removeManagedLocalPlugin,
  setPluginEnabled,
} from '../services/pluginService'
import type {
  CommitLocalManifestImportResult,
  ManagedOwnershipSummary,
  PluginAvailability,
  PluginAvailabilityReason,
  PluginCatalogItem,
  PluginCatalogSnapshot,
  PluginLocalDiscoverySummary,
  PluginStatus,
  ReadyLocalManifestImport,
  RemoveManagedLocalPluginResult,
} from '../types/plugin'

export type PluginLoadStatus = 'idle' | 'loading' | 'ready' | 'error'
export type PluginImportStatus = 'idle' | 'choosing' | 'preview' | 'committing' | 'result'
export type PluginRemovalStatus = 'idle' | 'confirming' | 'removing' | 'result'
export type PluginStatusFilter = 'all' | PluginStatus

export interface RemovalTarget {
  plugin: PluginCatalogItem
  revision: string
  catalogGeneration: string
}

function hasSameImmutableContent(current: PluginCatalogItem, returned: PluginCatalogItem): boolean {
  const currentFields = Object.entries(current.manifest)
  return current.source === returned.source
    && currentFields.length === Object.keys(returned.manifest).length
    && currentFields.every(([key, value]) => (
      Object.prototype.hasOwnProperty.call(returned.manifest, key)
      // Strictly parsed v1 values are scalars or empty arrays. Compare each field
      // independently so equivalent manifest key insertion orders do not matter.
      && JSON.stringify(value) === JSON.stringify(
        returned.manifest[key as keyof PluginCatalogItem['manifest']],
      )
    ))
    && current.management === returned.management
    && current.toggleBlockReasonCode === returned.toggleBlockReasonCode
    && JSON.stringify(current.grantedCapabilities) === JSON.stringify(returned.grantedCapabilities)
}

function hasSameManagedOwnership(
  current: ManagedOwnershipSummary,
  returned: ManagedOwnershipSummary,
): boolean {
  return current.status === returned.status
    && current.conflictingEntryCount === returned.conflictingEntryCount
    && current.rollbackPendingCount === returned.rollbackPendingCount
    && current.cleanupPendingCount === returned.cleanupPendingCount
}

function cloneCatalogItem(plugin: PluginCatalogItem): PluginCatalogItem {
  return {
    ...plugin,
    manifest: {
      ...plugin.manifest,
      contributions: [],
      requestedCapabilities: [],
    },
    grantedCapabilities: [],
  }
}

export const usePluginStore = defineStore('plugin', () => {
  const catalog = ref<PluginCatalogItem[]>([])
  const revision = ref('0')
  const catalogGeneration = ref('0')
  const localDiscovery = ref<PluginLocalDiscoverySummary | null>(null)
  const managedOwnership = ref<ManagedOwnershipSummary | null>(null)
  const availability = ref<PluginAvailability | null>(null)
  const availabilityReasonCode = ref<PluginAvailabilityReason | null>(null)
  const loadStatus = ref<PluginLoadStatus>('idle')
  const loadError = ref<string | null>(null)
  const reloadStatus = ref<PluginLoadStatus>('idle')
  const reloadError = ref<string | null>(null)
  const query = ref('')
  const statusFilter = ref<PluginStatusFilter>('all')
  const pendingIds = ref(new Set<string>())
  const actionErrors = ref<Record<string, string>>({})
  const importStatus = ref<PluginImportStatus>('idle')
  const importPreview = shallowRef<ReadyLocalManifestImport | null>(null)
  const importPreviewStale = ref(false)
  const importPreviewDeadline = ref<number | null>(null)
  const importError = ref<string | null>(null)
  const importResult = shallowRef<CommitLocalManifestImportResult | null>(null)
  const importOutcomeUnknown = ref(false)
  const removalStatus = ref<PluginRemovalStatus>('idle')
  const removalTarget = shallowRef<RemovalTarget | null>(null)
  const removalConfirmationStale = ref(false)
  const removalError = ref<string | null>(null)
  const removalResult = shallowRef<RemoveManagedLocalPluginResult | null>(null)
  const removalOutcomeUnknown = ref(false)

  let loadFlight: Promise<void> | null = null
  let reloadFlight: Promise<void> | null = null
  let importCommitFlight: Promise<void> | null = null
  let removalFlight: Promise<void> | null = null
  let hasConfirmedSnapshot = false
  let snapshotSequence = 0n
  let confirmedSnapshotRevision = 0n
  let confirmedSnapshotOrder = 0n
  let latestAdoptedRequestOrder = 0n
  let mutationSequence = 0
  let importOwnerSequence = 0n
  let activeImportOwner: bigint | null = null
  let removalOwnerSequence = 0n
  let activeRemovalOwner: bigint | null = null
  const mutationOwners = new Map<string, number>()
  const confirmedItemRevisions = new Map<string, bigint>()

  const visiblePlugins = computed(() => {
    const normalizedQuery = query.value.trim().toLowerCase()
    return catalog.value.filter((plugin) => {
      if (statusFilter.value !== 'all' && plugin.status !== statusFilter.value) return false
      if (normalizedQuery.length === 0) return true
      const manifest = plugin.manifest
      return [
        manifest.name,
        manifest.id,
        manifest.publisher,
        manifest.publisherId,
        manifest.description,
      ].some((value) => value.toLowerCase().includes(normalizedQuery))
    })
  })

  function adoptGlobalRevision(nextRevision: string): void {
    if (BigInt(nextRevision) > BigInt(revision.value)) revision.value = nextRevision
  }

  function hasSameGenerationStructure(snapshot: PluginCatalogSnapshot): boolean {
    if (
      managedOwnership.value === null
      || !hasSameManagedOwnership(managedOwnership.value, snapshot.managedOwnership)
      || snapshot.plugins.length !== catalog.value.length
    ) return false
    const currentById = new Map(
      catalog.value.map((plugin) => [plugin.manifest.id, plugin] as const),
    )
    return snapshot.plugins.every((plugin) => {
      const current = currentById.get(plugin.manifest.id)
      return current !== undefined && hasSameImmutableContent(current, plugin)
    })
  }

  function adoptSnapshot(snapshot: PluginCatalogSnapshot, requestOrder: bigint): boolean {
    const nextGeneration = BigInt(snapshot.catalogGeneration)
    const currentGeneration = BigInt(catalogGeneration.value)
    const snapshotRevision = BigInt(snapshot.revision)
    if (nextGeneration < currentGeneration) return false
    if (nextGeneration > currentGeneration || !hasConfirmedSnapshot) {
      // Membership and content identity belong to the generation, not the state revision.
      catalog.value = snapshot.plugins
      catalogGeneration.value = snapshot.catalogGeneration
      revision.value = snapshot.revision
      availability.value = snapshot.availability
      availabilityReasonCode.value = snapshot.availabilityReasonCode
      localDiscovery.value = snapshot.localDiscovery
      managedOwnership.value = snapshot.managedOwnership
      confirmedSnapshotRevision = snapshotRevision
      confirmedSnapshotOrder = requestOrder
      if (requestOrder > latestAdoptedRequestOrder) latestAdoptedRequestOrder = requestOrder
      confirmedItemRevisions.clear()
      for (const plugin of snapshot.plugins) {
        confirmedItemRevisions.set(plugin.manifest.id, snapshotRevision)
      }
      mutationOwners.clear()
      pendingIds.value = new Set()
      actionErrors.value = {}
      return true
    }

    // A generation owns membership and immutable/ownership structure. Reject a
    // contradictory same-generation response before it can advance arbitration.
    if (!hasSameGenerationStructure(snapshot)) return false

    // A full snapshot confirms every item at least at its revision. Compare against
    // that confirmation, not the global revision that unrelated mutations can advance.
    if (
      snapshotRevision < confirmedSnapshotRevision
      || (snapshotRevision === confirmedSnapshotRevision && requestOrder < confirmedSnapshotOrder)
    ) return false
    confirmedSnapshotRevision = snapshotRevision
    confirmedSnapshotOrder = requestOrder
    if (requestOrder > latestAdoptedRequestOrder) latestAdoptedRequestOrder = requestOrder
    const currentById = new Map(
      catalog.value.map((plugin) => [plugin.manifest.id, plugin] as const),
    )
    const nextConfirmedRevisions = new Map<string, bigint>()

    const nextCatalog = snapshot.plugins.map((plugin) => {
      const id = plugin.manifest.id
      const current = currentById.get(id)
      const currentRevision = confirmedItemRevisions.get(id)
      if (current && currentRevision !== undefined && currentRevision > snapshotRevision) {
        nextConfirmedRevisions.set(id, currentRevision)
        return current
      }
      nextConfirmedRevisions.set(id, snapshotRevision)
      return plugin
    })

    const snapshotIds = new Set(snapshot.plugins.map((plugin) => plugin.manifest.id))
    for (const [id, current] of currentById) {
      const currentRevision = confirmedItemRevisions.get(id)
      if (
        !snapshotIds.has(id)
        && currentRevision !== undefined
        && currentRevision > snapshotRevision
      ) {
        nextCatalog.push(current)
        nextConfirmedRevisions.set(id, currentRevision)
      }
    }
    nextCatalog.sort((left, right) => {
      if (left.manifest.id < right.manifest.id) return -1
      if (left.manifest.id > right.manifest.id) return 1
      return 0
    })

    confirmedItemRevisions.clear()
    for (const [id, itemRevision] of nextConfirmedRevisions) {
      confirmedItemRevisions.set(id, itemRevision)
    }
    catalog.value = nextCatalog
    if (snapshotRevision >= BigInt(revision.value)) {
      availability.value = snapshot.availability
      availabilityReasonCode.value = snapshot.availabilityReasonCode
      localDiscovery.value = snapshot.localDiscovery
      managedOwnership.value = snapshot.managedOwnership
    }
    adoptGlobalRevision(snapshot.revision)
    return true
  }

  function markReadyPreviewStaleAfterAdoption(): void {
    const preview = importPreview.value
    if (
      importStatus.value === 'preview'
      && preview
      && BigInt(catalogGeneration.value) > BigInt(preview.catalogGeneration)
    ) {
      importPreviewStale.value = true
    }
  }

  function currentRemovalTargetMatches(target: RemovalTarget): boolean {
    if (catalogGeneration.value !== target.catalogGeneration) return false
    const current = catalog.value.find(
      (plugin) => plugin.manifest.id === target.plugin.manifest.id,
    )
    return current !== undefined
      && current.management === 'managed'
      && current.status === 'disabled'
      && current.canRemove === true
      && hasSameImmutableContent(target.plugin, current)
  }

  function markOpenRemovalConfirmationStaleAfterAdoption(): void {
    if (
      removalStatus.value === 'confirming'
      && removalTarget.value
      && !removalConfirmationStale.value
      && !currentRemovalTargetMatches(removalTarget.value)
    ) {
      removalConfirmationStale.value = true
    }
  }

  function confirmAuthoritativeSnapshot(
    snapshot: PluginCatalogSnapshot,
    requestOrder: bigint,
  ): void {
    if (adoptSnapshot(snapshot, requestOrder)) {
      markReadyPreviewStaleAfterAdoption()
      markOpenRemovalConfirmationStaleAfterAdoption()
    }
    hasConfirmedSnapshot = true
    loadStatus.value = 'ready'
    loadError.value = null
  }

  function beginLoad(): Promise<void> {
    const requestOrder = ++snapshotSequence
    loadStatus.value = 'loading'
    loadError.value = null

    const request = (async () => {
      try {
        const snapshot = await getPluginCatalog()
        confirmAuthoritativeSnapshot(snapshot, requestOrder)
      } catch (error) {
        // Separate flights can settle out of order. Only an adopted newer
        // snapshot supersedes this failure; an ignored response does not.
        if (requestOrder < latestAdoptedRequestOrder) {
          loadStatus.value = 'ready'
          return
        }
        loadError.value = pluginErrorMessage(error)
        loadStatus.value = hasConfirmedSnapshot ? 'ready' : 'error'
      }
    })()
    loadFlight = request
    void request.finally(() => {
      if (loadFlight === request) loadFlight = null
    })
    return request
  }

  function load(): Promise<void> {
    if (loadFlight) return loadFlight
    if (loadStatus.value !== 'idle') return Promise.resolve()
    return beginLoad()
  }

  function retry(): Promise<void> {
    if (loadFlight) return loadFlight
    return beginLoad()
  }

  function reload(): Promise<void> {
    if (reloadFlight) return reloadFlight
    const requestOrder = ++snapshotSequence
    reloadStatus.value = 'loading'
    reloadError.value = null
    const request = (async () => {
      try {
        const snapshot = await reloadPluginCatalog()
        confirmAuthoritativeSnapshot(snapshot, requestOrder)
        reloadStatus.value = 'ready'
      } catch (error) {
        if (requestOrder < latestAdoptedRequestOrder) {
          reloadStatus.value = 'ready'
          return
        }
        reloadError.value = pluginErrorMessage(error)
        reloadStatus.value = 'error'
      }
    })()
    reloadFlight = request
    void request.finally(() => {
      if (reloadFlight === request) reloadFlight = null
    })
    return request
  }

  function setQuery(value: string): void {
    query.value = value
  }

  function setStatusFilter(value: PluginStatusFilter): void {
    statusFilter.value = value
  }

  function replacePending(id: string, pending: boolean): void {
    const next = new Set(pendingIds.value)
    if (pending) next.add(id)
    else next.delete(id)
    pendingIds.value = next
  }

  function replaceActionError(id: string, error: string | null): void {
    const next = { ...actionErrors.value }
    if (error === null) delete next[id]
    else next[id] = error
    actionErrors.value = next
  }

  function clearReadyImportState(): void {
    importPreview.value = null
    importPreviewStale.value = false
    importPreviewDeadline.value = null
  }

  async function cancelImportTokenBestEffort(token: string): Promise<void> {
    try {
      await cancelLocalManifestImport(token)
    } catch {
      // Cancellation is intentionally idempotent and does not surface transport details.
    }
  }

  async function prepareImport(): Promise<void> {
    if (importStatus.value !== 'idle') return

    const owner = ++importOwnerSequence
    activeImportOwner = owner
    importStatus.value = 'choosing'
    clearReadyImportState()
    importError.value = null
    importResult.value = null
    importOutcomeUnknown.value = false

    try {
      const prepared = await prepareLocalManifestImport()
      if (activeImportOwner !== owner || importStatus.value !== 'choosing') {
        if (prepared.status === 'ready') {
          await cancelImportTokenBestEffort(prepared.token)
        }
        return
      }

      if (prepared.status === 'cancelled') {
        activeImportOwner = null
        importStatus.value = 'idle'
        return
      }

      importPreview.value = prepared
      importPreviewDeadline.value = performance.now() + prepared.expiresInSeconds * 1_000
      importPreviewStale.value = hasConfirmedSnapshot
        && BigInt(catalogGeneration.value) > BigInt(prepared.catalogGeneration)
      importStatus.value = 'preview'
    } catch (error) {
      if (activeImportOwner !== owner || importStatus.value !== 'choosing') return
      activeImportOwner = null
      importError.value = pluginErrorMessage(error)
      importStatus.value = 'result'
    }
  }

  async function cancelImport(): Promise<void> {
    if (importStatus.value !== 'preview' || !importPreview.value) return

    const token = importPreview.value.token
    activeImportOwner = null
    clearReadyImportState()
    importStatus.value = 'idle'
    importError.value = null
    importOutcomeUnknown.value = false
    await cancelImportTokenBestEffort(token)
  }

  function commitImport(): Promise<void> {
    if (importCommitFlight) return importCommitFlight
    const preview = importPreview.value
    if (importStatus.value !== 'preview' || !preview || importPreviewStale.value) {
      return Promise.resolve()
    }

    importStatus.value = 'committing'
    importError.value = null
    importResult.value = null
    importOutcomeUnknown.value = false
    const requestOrder = ++snapshotSequence
    const request = (async () => {
      try {
        const result = await commitLocalManifestImport(preview)
        confirmAuthoritativeSnapshot(result.snapshot, requestOrder)
        importResult.value = result
        importOutcomeUnknown.value = false
      } catch (error) {
        const code = pluginErrorCode(error)
        if (code === 'plugin_import_token_invalid' || code === 'plugin_import_busy') {
          importOutcomeUnknown.value = false
          importError.value = pluginErrorMessage(error)
        } else {
          importOutcomeUnknown.value = true
          importError.value = '结果尚未确认，请重新扫描。'
        }
      } finally {
        activeImportOwner = null
        clearReadyImportState()
        importStatus.value = 'result'
      }
    })()
    importCommitFlight = request
    void request.finally(() => {
      if (importCommitFlight === request) importCommitFlight = null
    })
    return request
  }

  function releaseImportView(): void {
    if (importStatus.value === 'choosing') {
      activeImportOwner = null
      clearReadyImportState()
      importError.value = null
      importOutcomeUnknown.value = false
      importStatus.value = 'idle'
      return
    }

    if (importStatus.value === 'preview' && importPreview.value) {
      const token = importPreview.value.token
      activeImportOwner = null
      clearReadyImportState()
      importError.value = null
      importOutcomeUnknown.value = false
      importStatus.value = 'idle'
      void cancelImportTokenBestEffort(token)
    }
  }

  function clearImportResult(): void {
    if (importStatus.value !== 'result') return
    importResult.value = null
    importError.value = null
    importOutcomeUnknown.value = false
    importStatus.value = 'idle'
  }

  function clearRemovalState(): void {
    removalTarget.value = null
    removalConfirmationStale.value = false
    removalError.value = null
    removalResult.value = null
    removalOutcomeUnknown.value = false
  }

  function beginRemoval(id: string): boolean {
    if (
      !hasConfirmedSnapshot
      || activeRemovalOwner !== null
      || removalFlight !== null
      || removalStatus.value !== 'idle'
    ) return false
    const plugin = catalog.value.find((candidate) => candidate.manifest.id === id)
    if (
      !plugin
      || plugin.management !== 'managed'
      || plugin.status !== 'disabled'
      || plugin.canRemove !== true
    ) return false

    const owner = ++removalOwnerSequence
    activeRemovalOwner = owner
    clearRemovalState()
    removalTarget.value = {
      plugin: cloneCatalogItem(plugin),
      revision: revision.value,
      catalogGeneration: catalogGeneration.value,
    }
    removalStatus.value = 'confirming'
    return true
  }

  function cancelRemoval(): void {
    if (removalStatus.value !== 'confirming') return
    activeRemovalOwner = null
    clearRemovalState()
    removalStatus.value = 'idle'
  }

  function confirmRemoval(): Promise<void> {
    if (removalFlight) return removalFlight
    const target = removalTarget.value
    const owner = activeRemovalOwner
    if (
      removalStatus.value !== 'confirming'
      || !target
      || owner === null
      || removalConfirmationStale.value
    ) return Promise.resolve()
    if (!currentRemovalTargetMatches(target)) {
      removalConfirmationStale.value = true
      return Promise.resolve()
    }

    removalStatus.value = 'removing'
    removalError.value = null
    removalResult.value = null
    removalOutcomeUnknown.value = false
    // Equal generation/revision snapshots are owned by request start order, so
    // allocate before awaiting the native operation.
    const requestOrder = ++snapshotSequence
    const request = (async () => {
      try {
        const result = await removeManagedLocalPlugin(
          target.plugin.manifest.id,
          target.catalogGeneration,
        )
        if (activeRemovalOwner !== owner) return
        confirmAuthoritativeSnapshot(result.snapshot, requestOrder)
        removalResult.value = result
      } catch {
        if (activeRemovalOwner !== owner) return
        removalOutcomeUnknown.value = true
        removalError.value = '移除结果尚未确认，请重新扫描。'
      } finally {
        if (activeRemovalOwner === owner) {
          activeRemovalOwner = null
          removalStatus.value = 'result'
        }
      }
    })()
    removalFlight = request
    void request.finally(() => {
      if (removalFlight === request) removalFlight = null
    })
    return request
  }

  function releaseRemovalView(): void {
    if (removalStatus.value === 'confirming') cancelRemoval()
  }

  function clearRemovalResult(): void {
    if (removalStatus.value !== 'result') return
    clearRemovalState()
    removalStatus.value = 'idle'
  }

  async function setEnabled(id: string, enabled: boolean): Promise<boolean> {
    const existing = catalog.value.find((plugin) => plugin.manifest.id === id)
    if (!existing || !existing.canToggle) return false

    const owner = ++mutationSequence
    const requestGeneration = catalogGeneration.value
    mutationOwners.set(id, owner)
    replacePending(id, true)
    replaceActionError(id, null)

    try {
      const result = await setPluginEnabled(id, enabled, requestGeneration)
      if (
        mutationOwners.get(id) !== owner
        || result.catalogGeneration !== requestGeneration
        || catalogGeneration.value !== requestGeneration
      ) return false

      const resultRevision = BigInt(result.revision)
      const confirmedRevision = confirmedItemRevisions.get(id)
      const itemIndex = catalog.value.findIndex((plugin) => plugin.manifest.id === id)
      if (
        itemIndex === -1
        || (confirmedRevision !== undefined && resultRevision < confirmedRevision)
        || !hasSameImmutableContent(catalog.value[itemIndex], result.plugin)
      ) return false
      catalog.value = catalog.value.map((plugin, index) => (
        index === itemIndex ? result.plugin : plugin
      ))
      confirmedItemRevisions.set(id, resultRevision)
      adoptGlobalRevision(result.revision)
      markOpenRemovalConfirmationStaleAfterAdoption()
      return true
    } catch (error) {
      if (mutationOwners.get(id) === owner && catalogGeneration.value === requestGeneration) {
        replaceActionError(id, pluginErrorMessage(error))
      }
      return false
    } finally {
      if (mutationOwners.get(id) === owner) {
        mutationOwners.delete(id)
        replacePending(id, false)
      }
    }
  }

  return {
    catalog,
    revision,
    catalogGeneration,
    localDiscovery,
    managedOwnership,
    availability,
    availabilityReasonCode,
    loadStatus,
    loadError,
    reloadStatus,
    reloadError,
    query,
    statusFilter,
    pendingIds,
    actionErrors,
    importStatus,
    importPreview,
    importPreviewStale,
    importPreviewDeadline,
    importError,
    importResult,
    importOutcomeUnknown,
    removalStatus,
    removalTarget,
    removalConfirmationStale,
    removalError,
    removalResult,
    removalOutcomeUnknown,
    visiblePlugins,
    load,
    retry,
    reload,
    setQuery,
    setStatusFilter,
    setEnabled,
    prepareImport,
    cancelImport,
    commitImport,
    releaseImportView,
    clearImportResult,
    beginRemoval,
    cancelRemoval,
    confirmRemoval,
    releaseRemovalView,
    clearRemovalResult,
  }
})
