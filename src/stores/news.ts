import { defineStore } from 'pinia'
import { ref } from 'vue'
import { getNewsStatus, listNewsMessages, markNewsSeen, recheckNewsCredentials, retryNewsSync } from '../services/newsService'
import type { NewsMessageDto, NewsMessagesCommittedEvent, NewsPage, NewsStatusSnapshot } from '../types/news'
import { countNewsAfterAnchor } from './newsAwayRecovery'
import { createNewsLatestCoordinator } from './newsLatestCoordinator'
import { createNewsLoadMore } from './newsLoadMore'
import { createNewsMetadata, errorText, normalizeNewsMessages } from './newsState'
import { createNewsPending } from './newsPending'
import { createNewsStatusOrder } from './newsStatusOrder'
import { mergeNewsLatestPage } from './newsViewMerge'
export const useNewsStore = defineStore('news', () => {
  const messages = ref<NewsMessageDto[]>([])
  const status = ref<NewsStatusSnapshot | null>(null)
  const hasMore = ref(true)
  const unreadCount = ref(0)
  const pendingNewCount = ref(0)
  const isPageActive = ref(false)
  const isAtLatest = ref(true)
  const initialLoading = ref(false)
  const refreshingLatest = ref(false)
  const loadingMore = ref(false)
  const initialError = ref<string | null>(null)
  const loadMoreError = ref<string | null>(null)
  const metadata = createNewsMetadata()
  const statusOrder = createNewsStatusOrder()
  const latest = createNewsLatestCoordinator((busy) => { refreshingLatest.value = busy })
  let pageSession = 0
  let enterOperation = 0
  let markOperation = 0
  let appliedMarkOperation = 0
  const pending = createNewsPending(pendingNewCount)
  function syncStatusMetadata(): void {
    if (status.value) status.value = {
      ...status.value, initialSyncComplete: metadata.initialSyncComplete,
      latestDeliveryId: metadata.latestId, unreadCount: metadata.unread,
    }
  }
  function adoptStatus(next: NewsStatusSnapshot, requestRevision?: number): boolean {
    const adopted = metadata.status(next, requestRevision)
    status.value = statusOrder.select(status.value, adopted.snapshot, adopted.stale)
    if (metadata.initialSyncComplete) unreadCount.value = metadata.unread
    else {
      messages.value = []; unreadCount.value = 0; pendingNewCount.value = 0
      pending.resetInitialSync()
    }
    return adopted.becameComplete
  }
  function adoptPage(result: NewsPage, revision: number): void {
    const adopted = metadata.page(result, revision)
    unreadCount.value = adopted.unread
    syncStatusMetadata()
  }
  const more = createNewsLoadMore({
    target: () => isPageActive.value && hasMore.value
      ? messages.value[messages.value.length - 1]?.deliveryId : undefined,
    list: (before) => listNewsMessages(before, 50),
    apply: (result) => {
      messages.value = normalizeNewsMessages([...messages.value, ...result.items])
      hasMore.value = result.hasMore
    },
    setBusy: (busy) => { loadingMore.value = busy },
    setError: (message) => { loadMoreError.value = message },
    errorText,
  })
  async function initialize(): Promise<void> { const revision = metadata.revision; adoptStatus(await getNewsStatus(), revision) }
  function resetPageSession(active: boolean): number {
    pageSession += 1; enterOperation += 1
    latest.reset(); more.reset()
    isPageActive.value = active
    return pageSession
  }
  async function replaceLatest(clearPending: boolean, session: number): Promise<string | undefined> {
    while (isPageActive.value && session === pageSession) {
      const revision = metadata.revision
      const result = await listNewsMessages(undefined, 50)
      if (!isPageActive.value || session !== pageSession) return undefined
      if (!metadata.covers(result)) continue
      more.reset()
      messages.value = normalizeNewsMessages(result.items)
      hasMore.value = result.hasMore
      adoptPage(result, revision)
      if (clearPending) pending.clear(messages.value[0]?.deliveryId)
      isAtLatest.value = true
      return messages.value[0]?.deliveryId
    }
    return undefined
  }
  async function recoverAway(anchor: string, session: number, operation: number): Promise<void> {
    const recovered = await countNewsAfterAnchor({
      anchorDeliveryId: anchor,
      currentLatestId: () => metadata.latestId,
      isCurrent: () => isPageActive.value && !isAtLatest.value
        && session === pageSession && operation === enterOperation,
      listPage: (before) => listNewsMessages(before, 50),
    })
    if (recovered === undefined || !isPageActive.value || isAtLatest.value
      || session !== pageSession || operation !== enterOperation) return
    pending.recovered(recovered, metadata.latestId)
  }
  async function enterPage(): Promise<void> {
    const resumeHistory = messages.value.length > 0
    const resumeAtLatest = isAtLatest.value
    const resumeAnchor = messages.value[0]?.deliveryId
    const session = resetPageSession(true)
    const operation = ++enterOperation
    const revision = metadata.revision
    initialLoading.value = true; initialError.value = null
    if (resumeHistory && !resumeAtLatest) pending.mergeInactive()
    try {
      const snapshot = await getNewsStatus()
      if (!isPageActive.value || session !== pageSession || operation !== enterOperation) return
      adoptStatus(snapshot, revision)
      if (metadata.initialSyncComplete) {
        if (!resumeHistory) await latest.enqueue(() => replaceLatest(true, session))
        else if (resumeAtLatest) await refreshLatest()
        else if (resumeAnchor && pending.needsRecovery(resumeAnchor, metadata.coversDeliveryId)) {
          await recoverAway(resumeAnchor, session, operation)
        }
      }
    } catch (error) {
      if (session === pageSession && operation === enterOperation) initialError.value = errorText(error)
      throw error
    } finally {
      if (session === pageSession && operation === enterOperation) initialLoading.value = false
    }
  }
  function leavePage(): void { resetPageSession(false) }
  function setAtLatest(value: boolean): void { isAtLatest.value = value }
  function refreshLatest(): Promise<void> {
    if (!isPageActive.value || !isAtLatest.value || !metadata.initialSyncComplete) return Promise.resolve()
    const session = pageSession
    return latest.automatic(async () => {
      if (!isPageActive.value || !isAtLatest.value || session !== pageSession) return
      if (metadata.coversDeliveryId(messages.value[0]?.deliveryId)) {
        pending.clear(messages.value[0]?.deliveryId); return
      }
      const revision = metadata.revision
      const result = await listNewsMessages(undefined, 50)
      if (!isPageActive.value || !isAtLatest.value || session !== pageSession) return
      more.reset()
      const merged = mergeNewsLatestPage(messages.value, hasMore.value, result)
      messages.value = merged.messages; hasMore.value = merged.hasMore
      adoptPage(result, revision)
      if (metadata.coversDeliveryId(messages.value[0]?.deliveryId)) {
        pending.clear(messages.value[0]?.deliveryId)
      }
    })
  }
  async function handleMessagesCommitted(event: NewsMessagesCommittedEvent): Promise<void> {
    const adopted = metadata.committed(event)
    pending.committed(event, adopted.wasComplete, isPageActive.value)
    syncStatusMetadata()
    if (!adopted.initialSyncComplete) {
      messages.value = []; unreadCount.value = 0; pendingNewCount.value = 0
      return
    }
    unreadCount.value = adopted.unread
    if (!isPageActive.value) return
    if (!adopted.wasComplete || isAtLatest.value) return refreshLatest()
  }
  async function loadAfterCompletion(becameComplete: boolean): Promise<void> {
    if (!becameComplete || !isPageActive.value) return
    const session = pageSession
    await latest.enqueue(() => replaceLatest(true, session))
  }
  async function handleStatusChanged(next: NewsStatusSnapshot): Promise<void> { await loadAfterCompletion(adoptStatus(next)) }
  async function showLatest(): Promise<string | undefined> {
    const session = pageSession
    try { return await latest.enqueue(() => replaceLatest(true, session)) }
    catch (error) {
      if (session === pageSession) initialError.value = errorText(error)
      throw error
    }
  }
  async function markLatestSeen(expectedTopId?: string): Promise<void> {
    const renderedId = expectedTopId ?? messages.value[0]?.deliveryId
    if (!renderedId || !isPageActive.value || !isAtLatest.value) return
    const revision = metadata.revision; const operation = ++markOperation
    const snapshot = await markNewsSeen(renderedId)
    const staleOperation = operation < appliedMarkOperation
    if (!staleOperation) appliedMarkOperation = operation
    const adopted = metadata.seen(snapshot, revision, staleOperation)
    unreadCount.value = adopted.unread
    syncStatusMetadata()
  }
  async function applyStatusCommand(command: () => Promise<NewsStatusSnapshot>): Promise<void> {
    const revision = metadata.revision
    await loadAfterCompletion(adoptStatus(await command(), revision))
  }
  function recheckCredentials(): Promise<void> { return applyStatusCommand(recheckNewsCredentials) }
  function retrySync(): Promise<void> { return applyStatusCommand(retryNewsSync) }
  return {
    messages, status, hasMore, unreadCount, pendingNewCount, isPageActive, isAtLatest,
    initialLoading, refreshingLatest, loadingMore, initialError, loadMoreError,
    initialize, enterPage, leavePage, setAtLatest, handleMessagesCommitted, handleStatusChanged,
    refreshLatest, showLatest, loadMore: more.load, markLatestSeen, recheckCredentials, retrySync,
  }
})
