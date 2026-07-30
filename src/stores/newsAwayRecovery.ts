import type { NewsPage } from '../types/news'
import { compareDeliveryIds, newestDeliveryId } from './newsState'

interface NewsAwayRecoveryOptions {
  anchorDeliveryId: string
  currentLatestId: () => string | undefined
  isCurrent: () => boolean
  listPage: (beforeDeliveryId?: string) => Promise<NewsPage>
}

export async function countNewsAfterAnchor({
  anchorDeliveryId, currentLatestId, isCurrent, listPage,
}: NewsAwayRecoveryOptions): Promise<number | undefined> {
  for (let restart = 0; restart < 3 && isCurrent(); restart += 1) {
    let beforeDeliveryId: string | undefined
    let scannedLatestId: string | undefined
    let count = 0
    const seen = new Set<string>()

    while (isCurrent()) {
      const result = await listPage(beforeDeliveryId)
      if (!isCurrent()) return undefined
      if (!beforeDeliveryId) {
        scannedLatestId = newestDeliveryId(result.latestDeliveryId, result.items[0]?.deliveryId)
      }
      const currentLatest = currentLatestId()
      if (currentLatest && (!scannedLatestId
        || compareDeliveryIds(scannedLatestId, currentLatest) < 0)) break

      let reachedAnchor = false
      for (const item of result.items) {
        if (compareDeliveryIds(item.deliveryId, anchorDeliveryId) <= 0) reachedAnchor = true
        else if (!seen.has(item.deliveryId)) {
          seen.add(item.deliveryId)
          count += 1
        }
      }
      if (reachedAnchor || !result.hasMore) {
        const latest = currentLatestId()
        if (!latest || (scannedLatestId
          && compareDeliveryIds(scannedLatestId, latest) >= 0)) return count
        break
      }
      const nextBefore = result.items[result.items.length - 1]?.deliveryId
      if (!nextBefore || nextBefore === beforeDeliveryId) {
        throw new Error('新闻缓存分页未推进')
      }
      beforeDeliveryId = nextBefore
    }
  }
  if (isCurrent()) throw new Error('新闻缓存快照持续落后于同步状态')
  return undefined
}
