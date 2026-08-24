import { tauriInvoke } from '../composables/useTauriCommand'
import type {
  CreateClientNotificationReceipt,
  CreateClientNotificationRequest,
} from '../types/notification'

export function createClientNotification(
  request: CreateClientNotificationRequest,
): Promise<CreateClientNotificationReceipt> {
  return tauriInvoke<CreateClientNotificationReceipt>('create_client_notification', { request })
}
