import type { UpdateRiskConfigRequest } from '../types/models'

const DECIMAL = /^[+-]?(?:\d+(?:\.\d*)?|\.\d+)$/

function decimal(value: string): number | null {
  const normalized = value.trim()
  if (!DECIMAL.test(normalized)) return null
  const parsed = Number(normalized)
  return Number.isFinite(parsed) ? parsed : null
}

export function validateRiskConfig(request: UpdateRiskConfigRequest): string | null {
  const quantity = decimal(request.maxOrderQty)
  if (quantity === null || quantity <= 0) {
    return 'Maximum order quantity must be a decimal greater than 0.'
  }
  const deviation = decimal(request.maxPriceDeviationPct)
  if (deviation === null || deviation < 0) {
    return 'Maximum price deviation must be a non-negative decimal.'
  }
  if (!Number.isInteger(request.maxDailyOrders) || request.maxDailyOrders <= 0) {
    return 'Maximum daily orders must be a positive whole number.'
  }
  if (!request.tradingDayTimezone.trim()) {
    return 'Trading day timezone is required.'
  }
  return null
}
