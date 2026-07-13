import type { Position } from '../types/models'

export type TradeDirection = 'Buy' | 'Sell'
export type TradeMode = 'open' | 'close'
export type BasicOrderType = 'Limit' | 'Market'

function normalizedSide(side: string): 'long' | 'short' | 'unknown' {
  const value = side.trim().toLowerCase()
  if (value === 'buy' || value === 'long') return 'long'
  if (value === 'sell' || value === 'short') return 'short'
  return 'unknown'
}

export function findClosablePosition(
  positions: Position[],
  symbol: string,
  direction: TradeDirection,
): Position | null {
  const targetSide = direction === 'Buy' ? 'short' : 'long'
  return (
    positions
      .filter(
        (position) =>
          position.symbol === symbol &&
          normalizedSide(position.side) === targetSide &&
          Number.parseFloat(position.size) !== 0,
      )
      .sort(
        (left, right) =>
          Math.abs(Number.parseFloat(right.size)) - Math.abs(Number.parseFloat(left.size)),
      )[0] ?? null
  )
}

export function calculateQuickQty(input: {
  mode: TradeMode
  percent: number
  closeableQty: number
  availableBalance: number
  referencePrice: number
  leverage: number
}): number | null {
  const ratio = Math.min(100, Math.max(0, input.percent)) / 100
  if (input.mode === 'close') {
    return Number.isFinite(input.closeableQty) && input.closeableQty > 0
      ? input.closeableQty * ratio
      : null
  }
  if (
    !Number.isFinite(input.availableBalance) ||
    input.availableBalance <= 0 ||
    !Number.isFinite(input.referencePrice) ||
    input.referencePrice <= 0 ||
    !Number.isFinite(input.leverage) ||
    input.leverage <= 0
  ) {
    return null
  }
  return ((input.availableBalance * input.leverage) / input.referencePrice) * ratio
}

export function validateOrderDraft(input: {
  connected: boolean
  mode: TradeMode
  orderType: BasicOrderType
  qty: number
  price: number
  closeableQty: number
}): string | null {
  if (!input.connected) return '请先连接 API'
  if (!Number.isFinite(input.qty) || input.qty <= 0) return '请输入有效数量'
  if (input.orderType === 'Limit' && (!Number.isFinite(input.price) || input.price <= 0)) {
    return '请输入有效价格'
  }
  if (input.mode === 'close') {
    if (!Number.isFinite(input.closeableQty) || input.closeableQty <= 0) {
      return '当前方向没有可平仓位'
    }
    if (input.qty > input.closeableQty) {
      return '平仓数量不能超过当前可平数量'
    }
  }
  return null
}
