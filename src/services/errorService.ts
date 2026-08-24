import type { MessageApi } from 'naive-ui'
import { useLogStore } from '../stores/log'
import type { BackendErrorEvent } from '../types/models'

let messageApi: MessageApi | null = null
const seenBackendEventIds = new Set<string>()
const backendEventOrder: string[] = []
const MAX_SEEN_BACKEND_EVENTS = 2_048

function toText(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return '未知错误'
}

export function installMessageApi(api: MessageApi): void {
  messageApi = api
}

export function notifySuccess(message: string): void {
  messageApi?.success(message)
}

export function notifyInfo(message: string): void {
  messageApi?.info(message)
}

export function notifyWarning(message: string): void {
  messageApi?.warning(message)
}

export function reportFrontendError(error: unknown, context?: string): string {
  const detail = toText(error)
  const message = context ? `${context}: ${detail}` : detail
  useLogStore().setError(message)
  messageApi?.error(message)
  return message
}

export function showBackendError(event: BackendErrorEvent | unknown): string | undefined {
  if (typeof event !== 'object' || event === null) return undefined
  const candidate = event as Partial<BackendErrorEvent>
  if (typeof candidate.eventId !== 'string'
    || candidate.eventId.length === 0
    || typeof candidate.message !== 'string') return undefined
  if (seenBackendEventIds.has(candidate.eventId)) return candidate.message

  seenBackendEventIds.add(candidate.eventId)
  backendEventOrder.push(candidate.eventId)
  if (backendEventOrder.length > MAX_SEEN_BACKEND_EVENTS) {
    const oldest = backendEventOrder.shift()
    if (oldest !== undefined) seenBackendEventIds.delete(oldest)
  }
  messageApi?.error(candidate.message)
  return candidate.message
}

export function reportError(error: unknown, context?: string): string {
  return reportFrontendError(error, context)
}
