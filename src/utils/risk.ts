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
    return '最大单笔下单数量必须是大于 0 的十进制数。'
  }
  const deviation = decimal(request.maxPriceDeviationPct)
  if (deviation === null || deviation < 0) {
    return '最大价格偏离必须是非负十进制数。'
  }
  if (!Number.isInteger(request.maxDailyOrders) || request.maxDailyOrders <= 0) {
    return '每日最大下单次数必须是正整数。'
  }
  if (!request.tradingDayTimezone.trim()) {
    return '交易日时区不能为空。'
  }
  return null
}
