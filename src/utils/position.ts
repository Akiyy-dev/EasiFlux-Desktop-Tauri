import type { Position } from '../types/models'

function readString(raw: Record<string, unknown>, ...keys: string[]): string {
  for (const key of keys) {
    const value = raw[key]
    if (value != null && value !== '') {
      return String(value)
    }
  }
  return ''
}

function readPositionIdx(raw: Record<string, unknown>): number | undefined {
  const value = raw.positionIdx ?? raw.position_idx
  if (value == null || value === '') {
    return undefined
  }
  const parsed = Number(value)
  return Number.isFinite(parsed) ? parsed : undefined
}

export function normalizePosition(raw: Position | Record<string, unknown>): Position {
  const record = raw as Record<string, unknown>
  const positionIdx = readPositionIdx(record)
  return {
    symbol: readString(record, 'symbol', 's'),
    side: readString(record, 'side', 'positionSide', 'position_side', 'direction'),
    size:
      readString(
        record,
        'size',
        'qty',
        'quantity',
        'positionAmt',
        'position_amt',
        'positionQty',
        'position_qty',
        'holdQty',
        'hold_qty',
        'openQty',
        'open_qty',
        'currentPiece',
        'current_piece',
        'totalPiece',
        'total_piece',
      ) || '0',
    entryPrice:
      readString(record, 'entryPrice', 'entry_price', 'avgPrice', 'avg_price', 'openPrice', 'open_price') ||
      '0',
    leverage: readString(record, 'leverage') || '1',
    unrealisedPnl: readString(
      record,
      'unrealisedPnl',
      'unrealised_pnl',
      'unrealizedPnl',
      'unrealized_pnl',
      'profitUnreal',
      'profit_unreal',
      'pnl',
    ) || '0',
    positionIdx,
  }
}

export function normalizePositions(raw: unknown): Position[] {
  if (!Array.isArray(raw)) {
    return []
  }
  return raw
    .map((item) => normalizePosition(item as Record<string, unknown>))
    .filter((position) => position.symbol.length > 0 && parseFloat(position.size) !== 0)
}

export function positionRoiPct(position: Position): string {
  const pnl = Number.parseFloat(position.unrealisedPnl)
  const size = Math.abs(Number.parseFloat(position.size))
  const entryPrice = Number.parseFloat(position.entryPrice)
  const notional = size * entryPrice
  if (!Number.isFinite(pnl) || !Number.isFinite(notional) || notional <= 0) {
    return '--'
  }
  return `${((pnl / notional) * 100).toFixed(2)}%`
}
