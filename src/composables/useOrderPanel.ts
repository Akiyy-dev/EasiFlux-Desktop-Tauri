import { computed, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useAccountStore } from '../stores/account'
import { useAccountProfilesStore } from '../stores/accountProfiles'
import { useConnectionStore } from '../stores/connection'
import { useMarketStore } from '../stores/market'
import { useOrderStore } from '../stores/order'
import { usePositionStore } from '../stores/position'
import { refreshSyncTask } from '../services/dataSyncService'
import { notifySuccess, notifyWarning, reportError } from '../services/errorService'
import {
  calculateQuickQty,
  findClosablePosition,
  validateOrderDraft,
  type BasicOrderType,
  type TradeDirection,
  type TradeMode,
} from '../utils/orderForm'
import {
  orderFailureMessage,
  orderSuccessMessage,
  quickQuantityWarning,
} from '../utils/orderPanelMessages'

export function useOrderPanel() {
  const accountStore = useAccountStore()
  const profilesStore = useAccountProfilesStore()
  const connectionStore = useConnectionStore()
  const marketStore = useMarketStore()
  const orderStore = useOrderStore()
  const positionStore = usePositionStore()
  const { summary } = storeToRefs(accountStore)
  const { activeSymbol, ticker } = storeToRefs(marketStore)
  const { positions } = storeToRefs(positionStore)
  const direction = ref<TradeDirection>('Buy')
  const tradeMode = ref<TradeMode>('open')
  const orderType = ref<BasicOrderType>('Limit')
  const qty = ref('')
  const price = ref('')
  const sizePct = ref(0)
  const submitting = ref(false)
  const validationMessage = ref<string | null>(null)
  const usdtBalance = computed(() =>
    summary.value?.balances.find((balance) => balance.asset === 'USDT'),
  )
  const availableBalance = computed(() => usdtBalance.value?.available ?? '--')
  const equityBalance = computed(() => summary.value?.totalEquity ?? '--')
  const closePosition = computed(() =>
    findClosablePosition(positions.value, activeSymbol.value, direction.value),
  )
  const closeableQtyNumber = computed(() =>
    Math.abs(Number.parseFloat(closePosition.value?.size ?? '0')),
  )
  const tradingBlockedMessage = computed(() => profilesStore.tradingBlockedMessage)
  const tradingBlockIsFailure = computed(() =>
    profilesStore.tradingBlocked && !profilesStore.switching,
  )

  function formatQty(value: number): string {
    if (!Number.isFinite(value) || value <= 0) return ''
    return value.toFixed(8).replace(/\.?0+$/, '')
  }

  const closeableQty = computed(() =>
    closePosition.value ? formatQty(closeableQtyNumber.value) : '0',
  )
  const activePosition = computed(() =>
    closePosition.value ?? positions.value.find((item) => item.symbol === activeSymbol.value),
  )
  const leverageNumber = computed(() => {
    const value = Number.parseFloat(activePosition.value?.leverage ?? '1')
    return Number.isFinite(value) && value > 0 ? value : 1
  })
  const leverageLabel = computed(() =>
    activePosition.value?.leverage ? `${activePosition.value.leverage}x` : '--',
  )
  const referencePrice = computed(() => orderType.value === 'Limit'
    ? Number.parseFloat(price.value)
    : Number.parseFloat(ticker.value?.markPrice || ticker.value?.lastPrice || '0'))
  const actionLabel = computed(() => {
    if (tradeMode.value === 'close') {
      return direction.value === 'Buy' ? '\u4e70\u5165\u5e73\u7a7a' : '\u5356\u51fa\u5e73\u591a'
    }
    return direction.value === 'Buy' ? '\u4e70\u5165\u5f00\u591a' : '\u5356\u51fa\u5f00\u7a7a'
  })
  const directionHint = computed(() => {
    if (tradeMode.value === 'close') {
      return direction.value === 'Buy' ? '\u5c06\u51cf\u5c11\u5f53\u524d\u7a7a\u4ed3' : '\u5c06\u51cf\u5c11\u5f53\u524d\u591a\u4ed3'
    }
    return direction.value === 'Buy' ? '\u9884\u671f\u5f00\u7acb\u591a\u4ed3' : '\u9884\u671f\u5f00\u7acb\u7a7a\u4ed3'
  })
  const canSubmit = computed(() => connectionStore.connected
    && !profilesStore.tradingBlocked && !submitting.value && !validationMessage.value)

  function updateValidation(): void {
    validationMessage.value = validateOrderDraft({
      connected: connectionStore.connected,
      mode: tradeMode.value,
      orderType: orderType.value,
      qty: Number.parseFloat(qty.value),
      price: Number.parseFloat(price.value),
      closeableQty: closeableQtyNumber.value,
    })
  }

  function applyQuickPercent(percent: number): void {
    sizePct.value = percent
    const nextQty = calculateQuickQty({
      mode: tradeMode.value, percent, closeableQty: closeableQtyNumber.value,
      availableBalance: Number.parseFloat(usdtBalance.value?.available ?? '0'),
      referencePrice: referencePrice.value, leverage: leverageNumber.value,
    })
    if (nextQty == null) {
      notifyWarning(quickQuantityWarning(tradeMode.value))
      return
    }
    qty.value = formatQty(nextQty)
  }

  watch(
    [qty, price, orderType, tradeMode, direction, closeableQtyNumber,
      () => connectionStore.connected],
    updateValidation,
    { immediate: true },
  )
  watch([tradeMode, direction, activeSymbol], () => {
    sizePct.value = 0
    qty.value = ''
    if (tradeMode.value === 'close' && closeableQtyNumber.value > 0) {
      qty.value = closeableQty.value
      sizePct.value = 100
    }
  })
  watch(orderType, (next) => { if (next === 'Market') price.value = '' })

  async function submit(): Promise<void> {
    if (profilesStore.tradingBlockedMessage) {
      notifyWarning(profilesStore.tradingBlockedMessage)
      return
    }
    updateValidation()
    if (validationMessage.value) {
      notifyWarning(validationMessage.value)
      return
    }
    submitting.value = true
    try {
      await orderStore.placeOrder({
        symbol: activeSymbol.value, side: direction.value, orderType: orderType.value,
        qty: qty.value,
        positionIdx: tradeMode.value === 'close' ? closePosition.value?.positionIdx : 0,
        price: orderType.value === 'Limit' ? price.value : undefined,
        reduceOnly: tradeMode.value === 'close',
      })
      await Promise.all([
        refreshSyncTask('privatePanels', true), refreshSyncTask('account', true),
      ])
      qty.value = ''
      sizePct.value = 0
      notifySuccess(orderSuccessMessage(actionLabel.value))
    } catch (error) {
      reportError(error, orderFailureMessage(actionLabel.value))
    } finally {
      submitting.value = false
    }
  }

  return {
    activeSymbol, direction, tradeMode, orderType, qty, price, sizePct, submitting,
    validationMessage, availableBalance, equityBalance, closeableQty, leverageLabel,
    actionLabel, directionHint, canSubmit, tradingBlockedMessage, tradingBlockIsFailure,
    applyQuickPercent, submit,
  }
}
