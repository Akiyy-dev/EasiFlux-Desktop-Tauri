export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

export function extractListItems(payload: unknown): unknown[] {
  if (!isRecord(payload)) {
    return Array.isArray(payload) ? payload : []
  }

  const data = payload.data ?? payload.result ?? payload
  if (Array.isArray(data)) {
    return data
  }
  if (!isRecord(data)) {
    return []
  }

  const listKeys = [
    'list',
    'items',
    'records',
    'symbols',
    'instruments',
    'fills',
    'rows',
    'orders',
    'positions',
  ] as const

  for (const key of listKeys) {
    const list = data[key]
    if (Array.isArray(list)) {
      return list
    }
  }

  return []
}

function readString(record: Record<string, unknown>, ...keys: string[]): string {
  for (const key of keys) {
    const value = record[key]
    if (value != null && value !== '') {
      return String(value)
    }
  }
  return ''
}

function readNumber(record: Record<string, unknown>, ...keys: string[]): number {
  for (const key of keys) {
    const value = record[key]
    if (typeof value === 'number' && Number.isFinite(value)) {
      return value
    }
    if (typeof value === 'string' && value.trim().length > 0) {
      const parsed = Number.parseFloat(value)
      if (Number.isFinite(parsed)) {
        return parsed
      }
    }
  }
  return 0
}

export function readRecordString(record: unknown, ...keys: string[]): string {
  if (!isRecord(record)) {
    return ''
  }
  return readString(record, ...keys)
}

export function readRecordNumber(record: unknown, ...keys: string[]): number {
  if (!isRecord(record)) {
    return 0
  }
  return readNumber(record, ...keys)
}
