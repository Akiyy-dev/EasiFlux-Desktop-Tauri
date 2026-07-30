import type { Ref } from 'vue'
import type { NewsMessagesCommittedEvent } from '../types/news'
import { newestDeliveryId } from './newsState'

export function createNewsPending(pendingCount: Ref<number>) {
  let inactiveInsertedCount = 0
  let coverageLatestId: string | undefined

  function clear(renderedTopId?: string): void {
    pendingCount.value = 0
    inactiveInsertedCount = 0
    coverageLatestId = newestDeliveryId(coverageLatestId, renderedTopId)
  }
  function resetInitialSync(): void {
    pendingCount.value = 0
    inactiveInsertedCount = 0
  }
  function mergeInactive(): void {
    pendingCount.value += inactiveInsertedCount
    inactiveInsertedCount = 0
  }
  function recovered(count: number, latestId?: string): void {
    pendingCount.value = count
    inactiveInsertedCount = 0
    coverageLatestId = latestId
  }
  function needsRecovery(
    anchorId: string,
    covers: (deliveryId?: string) => boolean,
  ): boolean {
    return !covers(newestDeliveryId(anchorId, coverageLatestId))
  }
  function committed(
    event: NewsMessagesCommittedEvent,
    wasComplete: boolean,
    pageActive: boolean,
  ): void {
    coverageLatestId = newestDeliveryId(coverageLatestId, event.newestDeliveryId)
    if (!wasComplete) return
    if (pageActive) pendingCount.value += event.insertedCount
    else inactiveInsertedCount += event.insertedCount
  }

  return { clear, resetInitialSync, mergeInactive, recovered, needsRecovery, committed }
}
