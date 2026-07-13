import type { MessageApi } from 'naive-ui'
import { useLogStore } from '../stores/log'

let messageApi: MessageApi | null = null

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

export function reportError(error: unknown, context?: string): string {
  const detail = toText(error)
  const message = context ? `${context}: ${detail}` : detail
  useLogStore().setError(message)
  messageApi?.error(message)
  return message
}
