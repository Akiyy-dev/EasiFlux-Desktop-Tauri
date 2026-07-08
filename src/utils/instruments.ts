const SYMBOL_KEYS = ['symbol', 's', 'instId', 'instrumentId'] as const

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function extractSymbol(item: unknown): string | null {
  if (!isRecord(item)) {
    return null
  }
  for (const key of SYMBOL_KEYS) {
    const raw = item[key]
    if (typeof raw === 'string' && raw.trim().length > 0) {
      return raw.trim()
    }
  }
  return null
}

function collectListItems(payload: unknown): unknown[] {
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

  const listKeys = ['list', 'items', 'records', 'symbols', 'instruments'] as const
  for (const key of listKeys) {
    const list = data[key]
    if (Array.isArray(list)) {
      return list
    }
  }

  return []
}

export function parseInstrumentSymbols(payload: unknown): string[] {
  const symbols = new Set<string>()
  for (const item of collectListItems(payload)) {
    const symbol = extractSymbol(item)
    if (symbol) {
      symbols.add(symbol)
    }
  }
  return [...symbols].sort((a, b) => a.localeCompare(b))
}
