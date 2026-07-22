import { describe, expect, it } from 'vitest'
import {
  orderFailureMessage,
  orderSuccessMessage,
  quickQuantityWarning,
} from '../../src/utils/orderPanelMessages'

describe('order panel messages', () => {
  it('preserves the original open and close quick-quantity guidance', () => {
    expect(quickQuantityWarning('open')).toBe('需要有效余额和价格才能计算数量')
    expect(quickQuantityWarning('close')).toBe('当前方向没有可平仓位')
  })

  it('preserves the original submission result wording', () => {
    expect(orderSuccessMessage('买入开多')).toBe('买入开多委托已提交')
    expect(orderFailureMessage('买入开多')).toBe('买入开多失败')
  })
})
