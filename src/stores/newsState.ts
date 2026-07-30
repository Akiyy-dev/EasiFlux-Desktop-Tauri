import type {
  NewsMessageDto, NewsMessagesCommittedEvent, NewsPage,
  NewsStatusSnapshot, NewsUnreadSnapshot,
} from '../types/news'

export function compareDeliveryIds(left: string, right: string): number {
  const a = BigInt(left)
  const b = BigInt(right)
  return a < b ? -1 : a > b ? 1 : 0
}

export function newestDeliveryId(left?: string, right?: string): string | undefined {
  if (left === undefined) return right
  if (right === undefined) return left
  return compareDeliveryIds(left, right) >= 0 ? left : right
}

export function normalizeNewsMessages(messages: readonly NewsMessageDto[]): NewsMessageDto[] {
  const unique = new Map<string, NewsMessageDto>()
  for (const item of messages) unique.set(item.deliveryId, item)
  return [...unique.values()].sort((a, b) => compareDeliveryIds(b.deliveryId, a.deliveryId))
}

export function errorText(error: unknown): string {
  if (error instanceof Error) return error.message
  return typeof error === 'string' ? error : '未知错误'
}

export function reconcilePageUnread(
  knownLatestId: string | undefined,
  knownUnread: number,
  page: NewsPage,
): readonly [string | undefined, number] {
  const latest = page.latestDeliveryId
  if (!knownLatestId || (latest && compareDeliveryIds(latest, knownLatestId) >= 0)) {
    return [newestDeliveryId(knownLatestId, latest), page.unreadCount]
  }
  return [knownLatestId, knownUnread]
}

export function reconcileNewsStatus(
  knownLatestId: string | undefined,
  knownUnread: number,
  incoming: NewsStatusSnapshot,
): NewsStatusSnapshot {
  if (!knownLatestId) return incoming
  if (!incoming.latestDeliveryId) return {
    ...incoming, latestDeliveryId: knownLatestId,
    unreadCount: Math.max(knownUnread, incoming.unreadCount),
  }
  if (compareDeliveryIds(knownLatestId, incoming.latestDeliveryId) <= 0) return incoming
  return {
    ...incoming,
    latestDeliveryId: knownLatestId,
    unreadCount: Math.max(knownUnread, incoming.unreadCount),
  }
}

export function createNewsMetadata() {
  let latestId: string | undefined
  let unread = 0
  let initialSyncComplete = false
  let revision = 0

  function mergeUnread(incomingId: string | undefined, incomingUnread: number, stale: boolean): boolean {
    const previousId = latestId
    const previousUnread = unread
    if (!latestId) {
      if (incomingId) latestId = incomingId
      if (!stale || incomingId) unread = incomingUnread
    } else if (incomingId) {
      const order = compareDeliveryIds(incomingId, latestId)
      if (order > 0) {
        latestId = incomingId
        unread = incomingUnread
      } else if (order === 0) unread = Math.min(unread, incomingUnread)
    }
    return previousId !== latestId || previousUnread !== unread
  }

  function finish(stale: boolean, changed: boolean): void {
    if (!stale || changed) revision += 1
  }

  function status(incoming: NewsStatusSnapshot, requestRevision?: number) {
    const stale = requestRevision !== undefined && requestRevision !== revision
    const wasComplete = initialSyncComplete
    initialSyncComplete ||= incoming.initialSyncComplete
    const changed = mergeUnread(incoming.latestDeliveryId, incoming.unreadCount, stale)
    finish(stale, changed || wasComplete !== initialSyncComplete)
    const snapshot = {
      ...incoming, initialSyncComplete, latestDeliveryId: latestId, unreadCount: unread,
    }
    return { snapshot, becameComplete: !wasComplete && initialSyncComplete, stale }
  }

  function page(incoming: NewsPage, requestRevision: number) {
    const stale = requestRevision !== revision
    const changed = mergeUnread(incoming.latestDeliveryId, incoming.unreadCount, stale)
    finish(stale, changed)
    return { latestId, unread }
  }

  function covers(incoming: NewsPage): boolean {
    return coversDeliveryId(incoming.latestDeliveryId)
  }

  function coversDeliveryId(deliveryId?: string): boolean {
    if (!latestId) return true
    return deliveryId !== undefined && compareDeliveryIds(deliveryId, latestId) >= 0
  }

  function committed(incoming: NewsMessagesCommittedEvent) {
    const wasComplete = initialSyncComplete
    initialSyncComplete ||= incoming.initialSyncComplete
    const previousLatest = latestId
    latestId = newestDeliveryId(latestId, incoming.newestDeliveryId)
    if (!previousLatest || !incoming.newestDeliveryId) unread = incoming.unreadCount
    else {
      const order = compareDeliveryIds(incoming.newestDeliveryId, previousLatest)
      if (order > 0) unread = incoming.unreadCount
      else if (order === 0) unread = Math.min(unread, incoming.unreadCount)
    }
    revision += 1
    return { latestId, unread, initialSyncComplete, wasComplete }
  }

  function seen(incoming: NewsUnreadSnapshot, requestRevision: number, staleOperation: boolean) {
    const stale = staleOperation || requestRevision !== revision
    const changed = mergeUnread(incoming.latestDeliveryId, incoming.unreadCount, stale)
    finish(stale, changed)
    return { latestId, unread }
  }

  return {
    get revision() { return revision },
    get initialSyncComplete() { return initialSyncComplete },
    get latestId() { return latestId },
    get unread() { return unread },
    status, page, covers, coversDeliveryId, committed, seen,
  }
}
