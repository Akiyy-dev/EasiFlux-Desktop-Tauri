export type NewsStatusKind =
  | 'deploymentMisconfigured' | 'notConfigured' | 'credentialStoreUnavailable'
  | 'initialSync' | 'live' | 'retrying' | 'credentialInvalid'
  | 'contractError' | 'storageError' | 'stopped'

export interface NewsMessageDto { deliveryId: string; createdAt: string; text: string }

export interface NewsPage {
  items: NewsMessageDto[]
  hasMore: boolean
  latestDeliveryId?: string
  unreadCount: number
}

export interface NewsUnreadSnapshot { latestDeliveryId?: string; unreadCount: number }

export interface NewsStatusSnapshot {
  kind: NewsStatusKind
  initialSyncComplete: boolean
  syncedCount: number
  latestDeliveryId?: string
  unreadCount: number
  retryAt?: string
  message?: string
}

export interface NewsMessagesCommittedEvent {
  insertedCount: number
  newestDeliveryId?: string
  unreadCount: number
  initialSyncComplete: boolean
}

export interface NewsTextSegment { kind: 'text' | 'link'; value: string; href?: string }
