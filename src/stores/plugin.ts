import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  getPluginCatalog,
  pluginErrorMessage,
  setPluginEnabled,
} from '../services/pluginService'
import type {
  PluginAvailability,
  PluginAvailabilityReason,
  PluginCatalogItem,
  PluginCatalogSnapshot,
  PluginStatus,
} from '../types/plugin'

export type PluginLoadStatus = 'idle' | 'loading' | 'ready' | 'error'
export type PluginStatusFilter = 'all' | PluginStatus

export const usePluginStore = defineStore('plugin', () => {
  const catalog = ref<PluginCatalogItem[]>([])
  const revision = ref('0')
  const availability = ref<PluginAvailability | null>(null)
  const availabilityReasonCode = ref<PluginAvailabilityReason | null>(null)
  const loadStatus = ref<PluginLoadStatus>('idle')
  const loadError = ref<string | null>(null)
  const query = ref('')
  const statusFilter = ref<PluginStatusFilter>('all')
  const pendingIds = ref(new Set<string>())
  const actionErrors = ref<Record<string, string>>({})

  let loadFlight: Promise<void> | null = null
  let hasConfirmedSnapshot = false
  let mutationSequence = 0
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

  function adoptSnapshot(snapshot: PluginCatalogSnapshot): void {
    const snapshotRevision = BigInt(snapshot.revision)
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
    availability.value = snapshot.availability
    availabilityReasonCode.value = snapshot.availabilityReasonCode
    adoptGlobalRevision(snapshot.revision)
  }

  function beginLoad(): Promise<void> {
    const refreshingConfirmedData = hasConfirmedSnapshot
    loadStatus.value = 'loading'
    loadError.value = null

    const request = (async () => {
      try {
        const snapshot = await getPluginCatalog()
        adoptSnapshot(snapshot)
        hasConfirmedSnapshot = true
        loadStatus.value = 'ready'
        loadError.value = null
      } catch (error) {
        loadError.value = pluginErrorMessage(error)
        loadStatus.value = refreshingConfirmedData ? 'ready' : 'error'
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

  async function setEnabled(id: string, enabled: boolean): Promise<boolean> {
    const existing = catalog.value.find((plugin) => plugin.manifest.id === id)
    if (!existing || !existing.canToggle) return false

    const owner = ++mutationSequence
    mutationOwners.set(id, owner)
    replacePending(id, true)
    replaceActionError(id, null)

    try {
      const result = await setPluginEnabled(id, enabled)
      if (mutationOwners.get(id) !== owner) return false

      const resultRevision = BigInt(result.revision)
      const confirmedRevision = confirmedItemRevisions.get(id)
      const itemIndex = catalog.value.findIndex((plugin) => plugin.manifest.id === id)
      if (
        itemIndex !== -1
        && (confirmedRevision === undefined || resultRevision >= confirmedRevision)
      ) {
        catalog.value = catalog.value.map((plugin, index) => (
          index === itemIndex ? result.plugin : plugin
        ))
        confirmedItemRevisions.set(id, resultRevision)
      }
      adoptGlobalRevision(result.revision)
      return true
    } catch (error) {
      if (mutationOwners.get(id) === owner) {
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
    availability,
    availabilityReasonCode,
    loadStatus,
    loadError,
    query,
    statusFilter,
    pendingIds,
    actionErrors,
    visiblePlugins,
    load,
    retry,
    setQuery,
    setStatusFilter,
    setEnabled,
  }
})
