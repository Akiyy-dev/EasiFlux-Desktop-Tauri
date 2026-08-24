import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  clearAccountNotifications,
  decodeCommandError,
  deleteNotification,
  getNotificationSettings,
  getNotificationSummary,
  listenNotificationChanges,
  listNotifications,
  markNotificationRead,
  markVisibleNotificationsRead,
  updateNotificationSettings,
} from '../services/notificationService'
import {
  shouldToast,
  showNotificationToast,
} from '../services/notificationToastService'
import type {
  NotificationChangedEvent,
  NotificationFilter,
  NotificationRecord,
  NotificationSettings,
} from '../types/notification'

const PAGE_LIMIT = 50
const seenCreatedIds = new Set<string>()

function copySettings(settings: NotificationSettings): NotificationSettings {
  return { ...settings }
}

function localErrorMessage(error: unknown): string {
  return decodeCommandError(error).message
}

export const useNotificationStore = defineStore('notification', () => {
  const accountId = ref<string | null>(null)
  const filter = ref<NotificationFilter>('all')
  const items = ref<NotificationRecord[]>([])
  const nextCursor = ref<string>()
  const unreadCount = ref<number | null>(null)
  const observedRevision = ref<string | null>(null)
  const initialLoading = ref(false)
  const loadingMore = ref(false)
  const dataError = ref<string | null>(null)
  const listenerError = ref<string | null>(null)
  const error = computed(() => dataError.value ?? listenerError.value)
  const pageError = ref<string | null>(null)
  const loadGeneration = ref(0)
  const settingsDraft = ref<NotificationSettings | null>(null)
  const settingsCommitted = ref<NotificationSettings | null>(null)
  const settingsSaveStatus = ref<'idle' | 'loading' | 'saving' | 'saved' | 'error'>('idle')
  const settingsError = ref<string | null>(null)
  const markReadPendingIds = ref(new Set<string>())
  const removePendingIds = ref(new Set<string>())
  const markAllReadPending = ref(false)
  const clearCurrentAccountPending = ref(false)

  let lifecycleId = 0
  let active = false
  let initialized = false
  let accountReady = false
  let pageInitialized = false
  let unlisten: (() => void) | null = null
  let startFlight: Promise<void> | null = null
  let summaryRequestId = 0
  let pageRequestId = 0
  let moreRequestId = 0
  let readAuthorityEpoch = 0
  let firstPageNeedsRetry = false
  let activePageRequest: {
    requestId: number
    authorityEpoch: number
    owner: number
    generation: number
    accountId: string | null
    filter: NotificationFilter
  } | null = null
  let settingsLoadRequestId = 0
  let settingsVersion = 0
  let settingsDrain: Promise<void> | null = null
  let eventRefreshScheduled = false
  let scheduledEventGeneration = 0
  let scheduledEventLifecycle = 0
  let pageAuthorityGeneration = -1
  let mutationOwnerSequence = 0
  const markReadOwners = new Map<string, number>()
  const removeOwners = new Map<string, number>()
  let markAllReadOwner: number | null = null
  let clearCurrentAccountOwner: number | null = null

  function ownsLifecycle(owner: number): boolean {
    return active && lifecycleId === owner
  }

  function ownsContext(
    owner: number,
    generation: number,
    capturedAccountId: string | null,
    capturedFilter: NotificationFilter,
  ): boolean {
    return ownsLifecycle(owner)
      && loadGeneration.value === generation
      && accountId.value === capturedAccountId
      && filter.value === capturedFilter
  }

  function replacePendingId(target: typeof markReadPendingIds, id: string, pending: boolean): void {
    const next = new Set(target.value)
    if (pending) next.add(id)
    else next.delete(id)
    target.value = next
  }

  function canAdoptRevision(capturedRevision: string | null, revision: string): boolean {
    return observedRevision.value === capturedRevision || observedRevision.value === revision
  }

  function adoptRevision(capturedRevision: string | null, revision: string): void {
    if (canAdoptRevision(capturedRevision, revision)) observedRevision.value = revision
  }

  function clearPage(clearUnread: boolean): void {
    items.value = []
    nextCursor.value = undefined
    firstPageNeedsRetry = false
    pageError.value = null
    dataError.value = null
    if (clearUnread) unreadCount.value = null
  }

  function invalidateLoadMore(): void {
    ++moreRequestId
    loadingMore.value = false
    pageError.value = null
  }

  function invalidateReadAuthority(): boolean {
    const pageRequest = activePageRequest
    const canceledFirstPage = pageRequest !== null
      && pageRequest.requestId === pageRequestId
      && pageRequest.authorityEpoch === readAuthorityEpoch
      && ownsContext(
        pageRequest.owner,
        pageRequest.generation,
        pageRequest.accountId,
        pageRequest.filter,
      )
    ++readAuthorityEpoch
    ++summaryRequestId
    ++pageRequestId
    ++moreRequestId
    activePageRequest = null
    initialLoading.value = false
    loadingMore.value = false
    pageError.value = null
    return canceledFirstPage
  }

  function invalidateMutationOwnership(): void {
    markReadOwners.clear()
    removeOwners.clear()
    markAllReadOwner = null
    clearCurrentAccountOwner = null
    markReadPendingIds.value = new Set()
    removePendingIds.value = new Set()
    markAllReadPending.value = false
    clearCurrentAccountPending.value = false
  }

  async function refreshSummary(
    owner = lifecycleId,
    generation = loadGeneration.value,
  ): Promise<void> {
    const requestId = ++summaryRequestId
    const authorityEpoch = readAuthorityEpoch
    const capturedAccountId = accountId.value
    const capturedFilter = filter.value
    const capturedRevision = observedRevision.value
    try {
      const summary = await getNotificationSummary(capturedAccountId)
      if (
        requestId !== summaryRequestId
        || authorityEpoch !== readAuthorityEpoch
        || !ownsContext(owner, generation, capturedAccountId, capturedFilter)
        || pageAuthorityGeneration === generation
      ) return
      unreadCount.value = summary.unreadCount
      adoptRevision(capturedRevision, summary.revision)
      if (!firstPageNeedsRetry) dataError.value = null
    } catch (summaryError) {
      if (
        requestId === summaryRequestId
        && authorityEpoch === readAuthorityEpoch
        && ownsContext(owner, generation, capturedAccountId, capturedFilter)
        && pageAuthorityGeneration !== generation
      ) {
        dataError.value = localErrorMessage(summaryError)
      }
    }
  }

  async function queryFirstPage(
    owner: number,
    generation: number,
  ): Promise<void> {
    const requestId = ++pageRequestId
    const authorityEpoch = readAuthorityEpoch
    const capturedAccountId = accountId.value
    const capturedFilter = filter.value
    const capturedRevision = observedRevision.value
    activePageRequest = {
      requestId,
      authorityEpoch,
      owner,
      generation,
      accountId: capturedAccountId,
      filter: capturedFilter,
    }
    initialLoading.value = true
    try {
      const result = await listNotifications({
        accountId: capturedAccountId,
        filter: capturedFilter,
        cursor: undefined,
        limit: PAGE_LIMIT,
      })
      if (
        requestId !== pageRequestId
        || authorityEpoch !== readAuthorityEpoch
        || !ownsContext(owner, generation, capturedAccountId, capturedFilter)
      ) return
      pageAuthorityGeneration = generation
      firstPageNeedsRetry = false
      items.value = result.items
      nextCursor.value = result.nextCursor
      unreadCount.value = result.unreadCount
      adoptRevision(capturedRevision, result.revision)
      pageError.value = null
      dataError.value = null
    } catch (pageLoadError) {
      if (
        requestId === pageRequestId
        && authorityEpoch === readAuthorityEpoch
        && ownsContext(owner, generation, capturedAccountId, capturedFilter)
      ) {
        firstPageNeedsRetry = true
        dataError.value = localErrorMessage(pageLoadError)
      }
    } finally {
      if (
        requestId === pageRequestId
        && authorityEpoch === readAuthorityEpoch
        && ownsContext(owner, generation, capturedAccountId, capturedFilter)
      ) {
        initialLoading.value = false
      }
      if (activePageRequest?.requestId === requestId) activePageRequest = null
    }
  }

  function beginPageReload(hard: boolean, clearUnread = hard): Promise<void> {
    const owner = lifecycleId
    const generation = loadGeneration.value
    invalidateLoadMore()
    if (hard) clearPage(clearUnread)
    return queryFirstPage(owner, generation)
  }

  function scheduleEventRefresh(): void {
    scheduledEventGeneration = loadGeneration.value
    scheduledEventLifecycle = lifecycleId
    if (eventRefreshScheduled) return
    eventRefreshScheduled = true
    void Promise.resolve().then(async () => {
      eventRefreshScheduled = false
      const generation = scheduledEventGeneration
      const owner = scheduledEventLifecycle
      if (!ownsLifecycle(owner) || generation !== loadGeneration.value) return
      if (pageInitialized) await queryFirstPage(owner, generation)
      else await refreshSummary(owner, generation)
    })
  }

  function isRelevantEvent(event: NotificationChangedEvent, capturedAccountId: string | null): boolean {
    return event.affectedScopes.some((scope) => (
      scope.type === 'global'
      || (scope.type === 'account' && scope.accountId === capturedAccountId)
    ))
  }

  function handleChangedEvent(event: NotificationChangedEvent, listenerOwner: number): void {
    if (!ownsLifecycle(listenerOwner)) return
    const capturedEvent = event
    const capturedAccountId = accountId.value
    const capturedRevision = observedRevision.value
    const capturedGeneration = loadGeneration.value
    const capturedAccountReady = accountReady
    const capturedSettings = settingsCommitted.value
      ? copySettings(settingsCommitted.value)
      : null
    const toastCandidate = capturedEvent.toastCandidate
    let firstCandidateReceipt = false
    if (toastCandidate && !seenCreatedIds.has(toastCandidate.id)) {
      seenCreatedIds.add(toastCandidate.id)
      firstCandidateReceipt = true
    }

    if (
      firstCandidateReceipt
      && capturedAccountReady
      && capturedSettings
      && shouldToast(toastCandidate!, capturedSettings, capturedAccountId)
    ) {
      showNotificationToast(toastCandidate!)
    }

    if (!ownsLifecycle(lifecycleId) || capturedGeneration !== loadGeneration.value) return
    if (capturedEvent.revision === capturedRevision) return

    const continuous = capturedEvent.change !== 'reset'
      && capturedEvent.previousRevision === capturedRevision
    const relevant = isRelevantEvent(capturedEvent, capturedAccountId)
    if (continuous && !relevant) {
      if (observedRevision.value === capturedRevision) {
        observedRevision.value = capturedEvent.revision
      }
      return
    }

    if (observedRevision.value !== capturedRevision) return
    observedRevision.value = capturedEvent.revision
    ++loadGeneration.value
    invalidateLoadMore()
    invalidateMutationOwnership()
    if (!continuous) clearPage(true)
    scheduleEventRefresh()
  }

  async function loadSettings(): Promise<void> {
    const owner = lifecycleId
    const requestId = ++settingsLoadRequestId
    const capturedVersion = settingsVersion
    const saveOwnsAuthority = settingsDrain !== null
    if (!saveOwnsAuthority) {
      settingsSaveStatus.value = 'loading'
      settingsError.value = null
    }
    try {
      const loaded = await getNotificationSettings()
      if (
        requestId !== settingsLoadRequestId
        || !ownsLifecycle(owner)
        || capturedVersion !== settingsVersion
        || saveOwnsAuthority
      ) return
      settingsDraft.value = copySettings(loaded)
      settingsCommitted.value = copySettings(loaded)
      settingsSaveStatus.value = 'idle'
    } catch (loadError) {
      if (
        requestId === settingsLoadRequestId
        && ownsLifecycle(owner)
        && capturedVersion === settingsVersion
        && !saveOwnsAuthority
      ) {
        settingsSaveStatus.value = 'error'
        settingsError.value = localErrorMessage(loadError)
      }
    }
  }

  function start(): Promise<void> {
    if (startFlight) return startFlight
    if (active && unlisten) return Promise.resolve()
    if (!active) {
      active = true
      ++lifecycleId
    }
    const owner = lifecycleId
    const flight = (async () => {
      try {
        const installedUnlisten = await listenNotificationChanges((event) => {
          handleChangedEvent(event, owner)
        })
        if (!ownsLifecycle(owner)) {
          installedUnlisten()
          return
        }
        unlisten = installedUnlisten
        listenerError.value = null
      } catch (listenError) {
        if (ownsLifecycle(owner)) listenerError.value = localErrorMessage(listenError)
      }
      if (!ownsLifecycle(owner) || initialized) return
      initialized = true
      await Promise.all([
        refreshSummary(owner, loadGeneration.value),
        loadSettings(),
      ])
    })()
    startFlight = flight
    void flight.finally(() => {
      if (startFlight === flight) startFlight = null
    })
    return flight
  }

  function stop(): void {
    if (!active) return
    active = false
    ++lifecycleId
    startFlight = null
    settingsDrain = null
    ++loadGeneration.value
    ++summaryRequestId
    ++pageRequestId
    ++moreRequestId
    ++readAuthorityEpoch
    ++settingsLoadRequestId
    ++settingsVersion
    initialized = false
    accountReady = false
    pageInitialized = false
    eventRefreshScheduled = false
    accountId.value = null
    filter.value = 'all'
    items.value = []
    nextCursor.value = undefined
    unreadCount.value = null
    observedRevision.value = null
    initialLoading.value = false
    loadingMore.value = false
    dataError.value = null
    listenerError.value = null
    pageError.value = null
    settingsDraft.value = null
    settingsCommitted.value = null
    settingsSaveStatus.value = 'idle'
    settingsError.value = null
    pageAuthorityGeneration = -1
    firstPageNeedsRetry = false
    activePageRequest = null
    invalidateMutationOwnership()
    const installedUnlisten = unlisten
    unlisten = null
    if (installedUnlisten) installedUnlisten()
  }

  async function setAccount(nextAccountId: string | null): Promise<void> {
    if (accountReady && accountId.value === nextAccountId) return
    const contextChanged = accountId.value !== nextAccountId
    accountReady = true
    if (!contextChanged) return
    const hadInitializedPage = pageInitialized
    accountId.value = nextAccountId
    const owner = lifecycleId
    const generation = ++loadGeneration.value
    invalidateLoadMore()
    invalidateMutationOwnership()
    clearPage(true)
    const loads: Promise<void>[] = [refreshSummary(owner, generation)]
    if (hadInitializedPage) loads.push(queryFirstPage(owner, generation))
    await Promise.all(loads)
  }

  async function open(): Promise<void> {
    if (pageInitialized && !dataError.value && !pageError.value) return
    pageInitialized = true
    await beginPageReload(false)
  }

  async function reload(): Promise<void> {
    pageInitialized = true
    await beginPageReload(false)
  }

  async function loadMore(): Promise<void> {
    const cursor = nextCursor.value
    if (!cursor || loadingMore.value) return
    const owner = lifecycleId
    const generation = loadGeneration.value
    const capturedAccountId = accountId.value
    const capturedFilter = filter.value
    const capturedRevision = observedRevision.value
    const requestId = ++moreRequestId
    const authorityEpoch = readAuthorityEpoch
    loadingMore.value = true
    pageError.value = null
    try {
      const result = await listNotifications({
        accountId: capturedAccountId,
        filter: capturedFilter,
        cursor,
        limit: PAGE_LIMIT,
      })
      if (
        requestId !== moreRequestId
        || authorityEpoch !== readAuthorityEpoch
        || !ownsContext(owner, generation, capturedAccountId, capturedFilter)
      ) return
      const knownIds = new Set(items.value.map((item) => item.id))
      items.value = [
        ...items.value,
        ...result.items.filter((item) => !knownIds.has(item.id)),
      ]
      nextCursor.value = result.nextCursor
      unreadCount.value = result.unreadCount
      adoptRevision(capturedRevision, result.revision)
    } catch (loadError) {
      if (
        requestId === moreRequestId
        && authorityEpoch === readAuthorityEpoch
        && ownsContext(owner, generation, capturedAccountId, capturedFilter)
      ) {
        pageError.value = localErrorMessage(loadError)
      }
    } finally {
      if (requestId === moreRequestId && authorityEpoch === readAuthorityEpoch) {
        loadingMore.value = false
      }
    }
  }

  async function setFilter(nextFilter: NotificationFilter): Promise<void> {
    if (filter.value === nextFilter) return
    filter.value = nextFilter
    const owner = lifecycleId
    const generation = ++loadGeneration.value
    invalidateLoadMore()
    invalidateMutationOwnership()
    clearPage(false)
    if (pageInitialized) await queryFirstPage(owner, generation)
  }

  async function markRead(id: string): Promise<boolean> {
    if (markReadPendingIds.value.has(id)) return false
    const owner = lifecycleId
    const generation = loadGeneration.value
    const capturedAccountId = accountId.value
    const capturedFilter = filter.value
    const capturedRevision = observedRevision.value
    const pendingOwner = ++mutationOwnerSequence
    if (!firstPageNeedsRetry) dataError.value = null
    markReadOwners.set(id, pendingOwner)
    replacePendingId(markReadPendingIds, id, true)
    try {
      const result = await markNotificationRead(capturedAccountId, id)
      if (!ownsContext(owner, generation, capturedAccountId, capturedFilter)) return false
      if (capturedFilter === 'unread') {
        items.value = items.value.filter((item) => item.id !== id)
      } else {
        items.value = items.value.map((item) => (
          item.id === id ? result.notification : item
        ))
      }
      unreadCount.value = result.unreadCount
      adoptRevision(capturedRevision, result.revision)
      const canceledFirstPage = invalidateReadAuthority()
      if (canceledFirstPage || firstPageNeedsRetry) scheduleEventRefresh()
      return true
    } catch (mutationError) {
      if (ownsContext(owner, generation, capturedAccountId, capturedFilter)) {
        dataError.value = localErrorMessage(mutationError)
      }
      return false
    } finally {
      if (markReadOwners.get(id) === pendingOwner) {
        markReadOwners.delete(id)
        replacePendingId(markReadPendingIds, id, false)
      }
    }
  }

  async function remove(id: string): Promise<boolean> {
    if (removePendingIds.value.has(id)) return false
    const owner = lifecycleId
    const generation = loadGeneration.value
    const capturedAccountId = accountId.value
    const capturedFilter = filter.value
    const capturedRevision = observedRevision.value
    const pendingOwner = ++mutationOwnerSequence
    if (!firstPageNeedsRetry) dataError.value = null
    removeOwners.set(id, pendingOwner)
    replacePendingId(removePendingIds, id, true)
    try {
      const result = await deleteNotification(capturedAccountId, id)
      if (!ownsContext(owner, generation, capturedAccountId, capturedFilter)) return false
      items.value = items.value.filter((item) => item.id !== id)
      unreadCount.value = result.unreadCount
      adoptRevision(capturedRevision, result.revision)
      const canceledFirstPage = invalidateReadAuthority()
      if (canceledFirstPage || firstPageNeedsRetry) scheduleEventRefresh()
      return true
    } catch (mutationError) {
      if (ownsContext(owner, generation, capturedAccountId, capturedFilter)) {
        dataError.value = localErrorMessage(mutationError)
      }
      return false
    } finally {
      if (removeOwners.get(id) === pendingOwner) {
        removeOwners.delete(id)
        replacePendingId(removePendingIds, id, false)
      }
    }
  }

  async function refreshAfterMutation(hard: boolean, clearUnread = false): Promise<void> {
    const owner = lifecycleId
    const generation = loadGeneration.value
    invalidateLoadMore()
    if (hard) clearPage(clearUnread)
    if (pageInitialized) await queryFirstPage(owner, generation)
    else await refreshSummary(owner, generation)
  }

  async function markAllRead(): Promise<boolean> {
    if (markAllReadPending.value) return false
    const owner = lifecycleId
    const generation = loadGeneration.value
    const capturedAccountId = accountId.value
    const capturedFilter = filter.value
    const capturedRevision = observedRevision.value
    const pendingOwner = ++mutationOwnerSequence
    dataError.value = null
    markAllReadOwner = pendingOwner
    markAllReadPending.value = true
    try {
      const result = await markVisibleNotificationsRead(capturedAccountId)
      if (!ownsContext(owner, generation, capturedAccountId, capturedFilter)) return false
      invalidateReadAuthority()
      unreadCount.value = result.unreadCount
      adoptRevision(capturedRevision, result.revision)
      await refreshAfterMutation(false)
      return true
    } catch (mutationError) {
      if (ownsContext(owner, generation, capturedAccountId, capturedFilter)) {
        dataError.value = localErrorMessage(mutationError)
      }
      return false
    } finally {
      if (markAllReadOwner === pendingOwner) {
        markAllReadOwner = null
        markAllReadPending.value = false
      }
    }
  }

  async function clearCurrentAccount(): Promise<boolean> {
    if (clearCurrentAccountPending.value) return false
    const capturedAccountId = accountId.value
    dataError.value = null
    if (capturedAccountId === null) {
      dataError.value = '当前没有可清空通知的账户'
      return false
    }
    const owner = lifecycleId
    const generation = loadGeneration.value
    const capturedFilter = filter.value
    const capturedRevision = observedRevision.value
    const pendingOwner = ++mutationOwnerSequence
    clearCurrentAccountOwner = pendingOwner
    clearCurrentAccountPending.value = true
    try {
      const result = await clearAccountNotifications(capturedAccountId)
      if (!ownsContext(owner, generation, capturedAccountId, capturedFilter)) return false
      invalidateReadAuthority()
      unreadCount.value = result.unreadCount
      adoptRevision(capturedRevision, result.revision)
      await refreshAfterMutation(true, false)
      return true
    } catch (mutationError) {
      if (ownsContext(owner, generation, capturedAccountId, capturedFilter)) {
        dataError.value = localErrorMessage(mutationError)
      }
      return false
    } finally {
      if (clearCurrentAccountOwner === pendingOwner) {
        clearCurrentAccountOwner = null
        clearCurrentAccountPending.value = false
      }
    }
  }

  function updateSettings(nextSettings: NotificationSettings): Promise<void> {
    settingsDraft.value = copySettings(nextSettings)
    ++settingsVersion
    settingsSaveStatus.value = 'saving'
    settingsError.value = null
    if (settingsDrain) return settingsDrain
    const owner = lifecycleId
    const drain = (async () => {
      while (ownsLifecycle(owner) && settingsDraft.value) {
        const snapshot = copySettings(settingsDraft.value)
        const version = settingsVersion
        settingsSaveStatus.value = 'saving'
        try {
          const committed = await updateNotificationSettings(snapshot)
          if (!ownsLifecycle(owner)) return
          settingsCommitted.value = copySettings(committed)
          if (version === settingsVersion) {
            settingsSaveStatus.value = 'saved'
            settingsError.value = null
            return
          }
        } catch (saveError) {
          if (ownsLifecycle(owner)) {
            settingsSaveStatus.value = 'error'
            settingsError.value = localErrorMessage(saveError)
          }
          return
        }
      }
    })()
    settingsDrain = drain
    void drain.finally(() => {
      if (settingsDrain === drain) settingsDrain = null
    })
    return drain
  }

  return {
    accountId,
    filter,
    items,
    nextCursor,
    unreadCount,
    observedRevision,
    initialLoading,
    loadingMore,
    error,
    pageError,
    loadGeneration,
    settingsDraft,
    settingsCommitted,
    settingsSaveStatus,
    settingsError,
    markReadPendingIds,
    removePendingIds,
    markAllReadPending,
    clearCurrentAccountPending,
    start,
    stop,
    setAccount,
    open,
    reload,
    loadMore,
    setFilter,
    markRead,
    markAllRead,
    remove,
    clearCurrentAccount,
    loadSettings,
    updateSettings,
  }
})
