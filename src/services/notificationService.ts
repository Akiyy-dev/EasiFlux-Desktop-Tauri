import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { tauriInvoke } from '../composables/useTauriCommand'
import type {
  ClearAccountNotificationsResult,
  CreateClientNotificationReceipt,
  CreateClientNotificationRequest,
  DecodedCommandError,
  DeleteNotificationResult,
  ListNotificationsRequest,
  MarkNotificationReadResult,
  MarkVisibleNotificationsReadResult,
  NotificationAction,
  NotificationChangedEvent,
  NotificationContent,
  NotificationEntity,
  NotificationPage,
  NotificationRecord,
  NotificationScope,
  NotificationSettings,
  NotificationSummary,
  NotificationToastCandidate,
} from '../types/notification'

const DEFAULT_PAGE_LIMIT = 50
const categories = new Set(['trading', 'riskAccount', 'connectionSystem'])
const kinds = new Set([
  'orderFilled',
  'orderCanceled',
  'orderRejected',
  'riskOrderBlocked',
  'accountSessionExpired',
  'accountRecoveryFailed',
  'accountReconciliationFailed',
  'connectionUnavailable',
  'connectionRecovered',
  'environmentUnavailable',
  'environmentRecovered',
])
const severities = new Set(['success', 'info', 'warning', 'error', 'critical'])
const changes = new Set(['created', 'updated', 'removed', 'reset'])

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isCount(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) >= 0
}

function isOptionalString(value: unknown): value is string | undefined {
  return value === undefined || typeof value === 'string'
}

function isScope(value: unknown): value is NotificationScope {
  if (!isObject(value)) return false
  if (value.type === 'global') return true
  return value.type === 'account' && typeof value.accountId === 'string'
}

function isContent(value: unknown): value is NotificationContent {
  if (!isObject(value)
    || typeof value.messageKey !== 'string'
    || typeof value.fallbackTitle !== 'string'
    || typeof value.fallbackBody !== 'string'
    || !isObject(value.params)) return false
  return Object.values(value.params).every((parameter) => (
    typeof parameter === 'string'
    || typeof parameter === 'boolean'
    || (typeof parameter === 'number' && Number.isFinite(parameter))
  ))
}

function isEntity(value: unknown): value is NotificationEntity {
  return isObject(value)
    && ['order', 'account', 'connection', 'environment'].includes(String(value.type))
    && typeof value.id === 'string'
}

function isAction(value: unknown): value is NotificationAction {
  if (!isObject(value)) return false
  if (value.type === 'openTrading') return isOptionalString(value.orderId)
  if (value.type === 'openAccountSettings') {
    return value.accountSection === 'api' || value.accountSection === 'risk'
  }
  return value.type === 'openGeneralSettings'
}

function isRecord(value: unknown): value is NotificationRecord {
  return isObject(value)
    && typeof value.id === 'string'
    && isScope(value.scope)
    && categories.has(String(value.category))
    && kinds.has(String(value.kind))
    && severities.has(String(value.severity))
    && isContent(value.content)
    && (value.entity === undefined || isEntity(value.entity))
    && (value.action === undefined || isAction(value.action))
    && isOptionalString(value.sourceEventId)
    && typeof value.dedupeKey === 'string'
    && isCount(value.occurrenceCount)
    && value.occurrenceCount >= 1
    && isCount(value.createdAtMs)
    && isCount(value.updatedAtMs)
    && (value.readAtMs === undefined || isCount(value.readAtMs))
}

function isToastCandidate(value: unknown): value is NotificationToastCandidate {
  return isObject(value)
    && typeof value.id === 'string'
    && isScope(value.scope)
    && (value.sessionEpoch === undefined || isCount(value.sessionEpoch))
    && categories.has(String(value.category))
    && severities.has(String(value.severity))
    && isContent(value.content)
    && (value.action === undefined || isAction(value.action))
}

function isChangedEvent(value: unknown): value is NotificationChangedEvent {
  if (!isObject(value)
    || typeof value.previousRevision !== 'string'
    || typeof value.revision !== 'string'
    || !changes.has(String(value.change))
    || !Array.isArray(value.affectedScopes)
    || !value.affectedScopes.every(isScope)
    || !isOptionalString(value.notificationId)
    || (value.toastCandidate !== undefined && !isToastCandidate(value.toastCandidate))) return false
  return value.change === 'created' || value.toastCandidate === undefined
}

function isSettings(value: unknown): value is NotificationSettings {
  return isObject(value)
    && typeof value.tradingToast === 'boolean'
    && typeof value.riskAccountToast === 'boolean'
    && typeof value.connectionSystemToast === 'boolean'
}

function invalidResponse(): never {
  throw new Error('INVALID_NOTIFICATION_RESPONSE')
}

function requireRevisionResult(value: unknown): Record<string, unknown> {
  if (!isObject(value) || typeof value.revision !== 'string') invalidResponse()
  return value
}

function requirePage(value: unknown): NotificationPage {
  if (!isObject(value)
    || !Array.isArray(value.items)
    || !value.items.every(isRecord)
    || !isOptionalString(value.nextCursor)
    || !isCount(value.unreadCount)
    || typeof value.revision !== 'string') invalidResponse()
  return value as unknown as NotificationPage
}

function requireSummary(value: unknown): NotificationSummary {
  if (!isObject(value) || !isCount(value.unreadCount) || typeof value.revision !== 'string') {
    invalidResponse()
  }
  return value as unknown as NotificationSummary
}

function requireMarkReadResult(value: unknown): MarkNotificationReadResult {
  const result = requireRevisionResult(value)
  if (!isRecord(result.notification) || !isCount(result.unreadCount)) invalidResponse()
  return value as MarkNotificationReadResult
}

function requireMarkVisibleResult(value: unknown): MarkVisibleNotificationsReadResult {
  const result = requireRevisionResult(value)
  if (!isCount(result.affectedCount)
    || !Array.isArray(result.affectedScopes)
    || !result.affectedScopes.every(isScope)
    || !isCount(result.unreadCount)) invalidResponse()
  return value as MarkVisibleNotificationsReadResult
}

function requireDeleteResult(value: unknown): DeleteNotificationResult {
  const result = requireRevisionResult(value)
  if (!isCount(result.unreadCount)) invalidResponse()
  return value as DeleteNotificationResult
}

function requireClearResult(value: unknown): ClearAccountNotificationsResult {
  const result = requireRevisionResult(value)
  if (!isCount(result.affectedCount) || !isCount(result.unreadCount)) invalidResponse()
  return value as ClearAccountNotificationsResult
}

function requireClientReceipt(value: unknown): CreateClientNotificationReceipt {
  const result = requireRevisionResult(value)
  if (!isRecord(result.notification) || !isCount(result.unreadCount)) invalidResponse()
  return value as CreateClientNotificationReceipt
}

export async function listNotifications(
  request: ListNotificationsRequest,
): Promise<NotificationPage> {
  const response = await tauriInvoke<unknown>('list_notifications', {
    request: {
      accountId: request.accountId,
      filter: request.filter,
      cursor: request.cursor,
      limit: request.limit ?? DEFAULT_PAGE_LIMIT,
    },
  })
  return requirePage(response)
}

export async function getNotificationSummary(
  accountId: string | null,
): Promise<NotificationSummary> {
  return requireSummary(await tauriInvoke<unknown>('get_notification_summary', { accountId }))
}

export async function markNotificationRead(
  contextAccountId: string | null,
  id: string,
): Promise<MarkNotificationReadResult> {
  return requireMarkReadResult(await tauriInvoke<unknown>('mark_notification_read', {
    contextAccountId,
    id,
  }))
}

export async function markVisibleNotificationsRead(
  contextAccountId: string | null,
): Promise<MarkVisibleNotificationsReadResult> {
  return requireMarkVisibleResult(await tauriInvoke<unknown>('mark_visible_notifications_read', {
    contextAccountId,
  }))
}

export async function deleteNotification(
  contextAccountId: string | null,
  id: string,
): Promise<DeleteNotificationResult> {
  return requireDeleteResult(await tauriInvoke<unknown>('delete_notification', {
    contextAccountId,
    id,
  }))
}

export async function clearAccountNotifications(
  accountId: string,
): Promise<ClearAccountNotificationsResult> {
  return requireClearResult(await tauriInvoke<unknown>('clear_account_notifications', { accountId }))
}

export async function createClientNotification(
  request: CreateClientNotificationRequest,
): Promise<CreateClientNotificationReceipt> {
  return requireClientReceipt(await tauriInvoke<unknown>('create_client_notification', { request }))
}

export async function getNotificationSettings(): Promise<NotificationSettings> {
  const response = await tauriInvoke<unknown>('get_notification_settings')
  if (!isSettings(response)) invalidResponse()
  return response
}

export async function updateNotificationSettings(
  settings: NotificationSettings,
): Promise<NotificationSettings> {
  const response = await tauriInvoke<unknown>('update_notification_settings', { settings })
  if (!isSettings(response)) invalidResponse()
  return response
}

export function listenNotificationChanges(
  handler: (event: NotificationChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<unknown>('notification:changed', (event) => {
    if (isChangedEvent(event.payload)) handler(event.payload)
  })
}

export function decodeCommandError(error: unknown): DecodedCommandError {
  if (typeof error === 'string') return { message: error }
  if (!isObject(error) || typeof error.message !== 'string') return { message: '未知错误' }

  const fallback: DecodedCommandError = { message: error.message }
  const seen = new Set<object>()
  let structured: DecodedCommandError | undefined
  let current: Record<string, unknown> | undefined = error

  for (let depth = 0; current && depth < 8; depth += 1) {
    if (seen.has(current)) break
    seen.add(current)

    if (typeof current.message === 'string') {
      const decoded: DecodedCommandError = { message: current.message }
      if (typeof current.code === 'string') decoded.code = current.code
      if (typeof current.eventId === 'string') decoded.eventId = current.eventId
      if (
        typeof current.notificationId === 'string'
        && current.notificationId.trim().length > 0
      ) {
        decoded.notificationId = current.notificationId
      }
      if (decoded.notificationId) return decoded
      if ((decoded.code || decoded.eventId) && !structured) structured = decoded
    }

    current = isObject(current.cause) ? current.cause : undefined
  }

  return structured ?? fallback
}
