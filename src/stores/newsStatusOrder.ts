import type { NewsStatusSnapshot } from '../types/news'

export function createNewsStatusOrder() {
  function select(
    current: NewsStatusSnapshot | null,
    incoming: NewsStatusSnapshot,
    staleRequest: boolean,
  ): NewsStatusSnapshot {
    if (!staleRequest || !current) return incoming
    return {
      ...current,
      initialSyncComplete: incoming.initialSyncComplete,
      syncedCount: Math.max(current.syncedCount, incoming.syncedCount),
      latestDeliveryId: incoming.latestDeliveryId,
      unreadCount: incoming.unreadCount,
    }
  }

  return { select }
}
