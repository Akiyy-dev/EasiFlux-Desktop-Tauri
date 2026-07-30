import type { NewsMessageDto, NewsPage } from '../types/news'
import { normalizeNewsMessages } from './newsState'

export function mergeNewsLatestPage(
  current: readonly NewsMessageDto[],
  currentHasMore: boolean,
  result: NewsPage,
): { messages: NewsMessageDto[]; hasMore: boolean } {
  const incoming = normalizeNewsMessages(result.items)
  if (current.length === 0) return { messages: incoming, hasMore: result.hasMore }
  const currentIds = new Set(current.map(({ deliveryId }) => deliveryId))
  const overlaps = incoming.some(({ deliveryId }) => currentIds.has(deliveryId))
  if (incoming.length > 0 && !overlaps && result.hasMore) {
    return { messages: incoming, hasMore: true }
  }
  return {
    messages: normalizeNewsMessages([...incoming, ...current]),
    hasMore: currentHasMore,
  }
}
