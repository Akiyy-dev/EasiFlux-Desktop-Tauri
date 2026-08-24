export type NotificationScope =
  | { type: 'global' }
  | { type: 'account'; accountId: string }

export type NotificationCategory = 'trading' | 'riskAccount' | 'connectionSystem'

export type NotificationKind =
  | 'orderFilled'
  | 'orderCanceled'
  | 'orderRejected'
  | 'riskOrderBlocked'
  | 'accountSessionExpired'
  | 'accountRecoveryFailed'
  | 'accountReconciliationFailed'
  | 'connectionUnavailable'
  | 'connectionRecovered'
  | 'environmentUnavailable'
  | 'environmentRecovered'

export type NotificationSeverity = 'success' | 'info' | 'warning' | 'error' | 'critical'
export type NotificationScalar = string | boolean | number

export interface NotificationContent {
  messageKey: string
  params: Readonly<Record<string, NotificationScalar>>
  fallbackTitle: string
  fallbackBody: string
}

export type NotificationEntityType = 'order' | 'account' | 'connection' | 'environment'

export interface NotificationEntity {
  type: NotificationEntityType
  id: string
}

export type AccountNotificationSection = 'api' | 'risk'

export type NotificationAction =
  | { type: 'openTrading'; orderId?: string }
  | { type: 'openAccountSettings'; accountSection: AccountNotificationSection }
  | { type: 'openGeneralSettings' }

export type NotificationUiAction =
  | NotificationAction
  | { type: 'openNotificationSettings' }

export interface NotificationRecord {
  id: string
  scope: NotificationScope
  category: NotificationCategory
  kind: NotificationKind
  severity: NotificationSeverity
  content: NotificationContent
  entity?: NotificationEntity
  action?: NotificationAction
  sourceEventId?: string
  dedupeKey: string
  occurrenceCount: number
  createdAtMs: number
  updatedAtMs: number
  readAtMs?: number
}

export type NotificationFilter = 'all' | 'unread'

export interface ListNotificationsRequest {
  accountId: string | null
  filter: NotificationFilter
  cursor?: string
  limit?: number
}

export interface NotificationPage {
  items: NotificationRecord[]
  nextCursor?: string
  unreadCount: number
  revision: string
}

export interface NotificationSummary {
  unreadCount: number
  revision: string
}

export interface MarkNotificationReadResult {
  notification: NotificationRecord
  unreadCount: number
  revision: string
}

export interface MarkVisibleNotificationsReadResult {
  affectedCount: number
  affectedScopes: NotificationScope[]
  unreadCount: number
  revision: string
}

export interface DeleteNotificationResult {
  unreadCount: number
  revision: string
}

export interface ClearAccountNotificationsResult {
  affectedCount: number
  unreadCount: number
  revision: string
}

export type NotificationChange = 'created' | 'updated' | 'removed' | 'reset'

export interface NotificationToastCandidate {
  id: string
  scope: NotificationScope
  sessionEpoch?: number
  category: NotificationCategory
  severity: NotificationSeverity
  content: NotificationContent
  action?: NotificationAction
}

export interface NotificationChangedEvent {
  previousRevision: string
  revision: string
  change: NotificationChange
  affectedScopes: NotificationScope[]
  notificationId?: string
  toastCandidate?: NotificationToastCandidate
}

export interface NotificationSettings {
  tradingToast: boolean
  riskAccountToast: boolean
  connectionSystemToast: boolean
}

export type ClientNotificationKind =
  | 'accountRecoveryFailed'
  | 'accountReconciliationFailed'

export type ClientNotificationFailedStep =
  | 'config'
  | 'profiles'
  | 'connection'
  | 'bootstrap'

export interface CreateClientNotificationRequest {
  accountId: string
  sessionEpoch: number
  attemptId: string
  kind: ClientNotificationKind
  failedSteps: readonly ClientNotificationFailedStep[]
}

export interface CreateClientNotificationReceipt {
  notification: NotificationRecord
  unreadCount: number
  revision: string
}

export interface DecodedCommandError {
  message: string
  code?: string
  eventId?: string
  notificationId?: string
}
