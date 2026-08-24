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
  notification: { id: string }
  unreadCount: number
  revision: string
}
