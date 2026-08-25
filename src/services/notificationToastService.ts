import type { MessageApi } from 'naive-ui'
import type {
  NotificationSettings,
  NotificationToastCandidate,
} from '../types/notification'

let messageApi: MessageApi | null = null

export function shouldToast(
  candidate: NotificationToastCandidate,
  settings: NotificationSettings,
  activeAccountId: string | null,
): boolean {
  const visible = candidate.scope.type === 'global'
    || (activeAccountId !== null && candidate.scope.accountId === activeAccountId)
  if (!visible) return false

  switch (candidate.category) {
    case 'trading':
      return settings.tradingToast
    case 'riskAccount':
      return settings.riskAccountToast
    case 'connectionSystem':
      return settings.connectionSystemToast
  }
}

export function installNotificationToastApi(message: MessageApi): void {
  messageApi = message
}

export function showNotificationToast(candidate: NotificationToastCandidate): void {
  if (!messageApi) return
  const message = `${candidate.content.fallbackTitle}：${candidate.content.fallbackBody}`
  switch (candidate.severity) {
    case 'success':
      messageApi.success(message)
      break
    case 'info':
      messageApi.info(message)
      break
    case 'warning':
      messageApi.warning(message)
      break
    case 'error':
    case 'critical':
      messageApi.error(message)
      break
  }
}
