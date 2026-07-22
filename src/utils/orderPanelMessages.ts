import type { TradeMode } from './orderForm'

export function quickQuantityWarning(mode: TradeMode): string {
  return mode === 'close'
    ? '当前方向没有可平仓位'
    : '需要有效余额和价格才能计算数量'
}

export function orderSuccessMessage(actionLabel: string): string {
  return `${actionLabel}委托已提交`
}

export function orderFailureMessage(actionLabel: string): string {
  return `${actionLabel}失败`
}
