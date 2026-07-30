import { tauriInvoke } from '../composables/useTauriCommand'
import type { NewsPage, NewsStatusSnapshot, NewsUnreadSnapshot } from '../types/news'

export function getNewsStatus(): Promise<NewsStatusSnapshot> {
  return tauriInvoke<NewsStatusSnapshot>('get_news_status')
}

export function listNewsMessages(beforeDeliveryId?: string, limit = 50): Promise<NewsPage> {
  const args: Record<string, unknown> = { limit }
  if (beforeDeliveryId !== undefined) args.beforeDeliveryId = beforeDeliveryId
  return tauriInvoke<NewsPage>('list_news_messages', args)
}

export function markNewsSeen(throughDeliveryId: string): Promise<NewsUnreadSnapshot> {
  return tauriInvoke<NewsUnreadSnapshot>('mark_news_seen', { throughDeliveryId })
}

export function recheckNewsCredentials(): Promise<NewsStatusSnapshot> {
  return tauriInvoke<NewsStatusSnapshot>('recheck_news_credentials')
}

export function retryNewsSync(): Promise<NewsStatusSnapshot> {
  return tauriInvoke<NewsStatusSnapshot>('retry_news_sync')
}
