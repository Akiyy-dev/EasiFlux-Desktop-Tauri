import type { Position } from '../types/models'
import { normalizePositions } from './position'

export function sumUnrealisedPnl(positions: Position[]): string {
  const total = normalizePositions(positions)
    .map((position) => Number.parseFloat(position.unrealisedPnl))
    .filter((value) => Number.isFinite(value))
    .reduce((sum, value) => sum + value, 0)

  return total.toFixed(4)
}

export function pnlToneClass(value: string): string {
  const parsed = Number.parseFloat(value)
  if (!Number.isFinite(parsed) || value === '--') {
    return ''
  }
  if (parsed > 0) return 'text-up'
  if (parsed < 0) return 'text-down'
  return ''
}

export function formatMetric(value: string | undefined | null, suffix = ''): string {
  if (!value || value === '--') {
    return '--'
  }
  return suffix ? `${value} ${suffix}` : value
}
