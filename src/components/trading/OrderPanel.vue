<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { NButton, NInput, NRadioButton, NRadioGroup, NSlider } from 'naive-ui'
import { storeToRefs } from 'pinia'
import { useAccountStore } from '../../stores/account'
import { useConnectionStore } from '../../stores/connection'
import { useMarketStore } from '../../stores/market'
import { useOrderStore } from '../../stores/order'
import { usePositionStore } from '../../stores/position'
import { notifySuccess, notifyWarning, reportError } from '../../services/errorService'
import { refreshSyncTask } from '../../services/dataSyncService'
import {
  calculateQuickQty,
  findClosablePosition,
  validateOrderDraft,
  type BasicOrderType,
  type TradeDirection,
  type TradeMode,
} from '../../utils/orderForm'
import TradingAssetPanel from './TradingAssetPanel.vue'

const accountStore = useAccountStore()
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
const closeableQty = computed(() =>
  closePosition.value ? formatQty(closeableQtyNumber.value) : '0',
)
const activePosition = computed(
  () => closePosition.value ?? positions.value.find((item) => item.symbol === activeSymbol.value),
)
const leverageNumber = computed(() => {
  const value = Number.parseFloat(activePosition.value?.leverage ?? '1')
  return Number.isFinite(value) && value > 0 ? value : 1
})
const leverageLabel = computed(() =>
  activePosition.value?.leverage ? `${activePosition.value.leverage}x` : '--',
)
const referencePrice = computed(() => {
  if (orderType.value === 'Limit') return Number.parseFloat(price.value)
  return Number.parseFloat(ticker.value?.markPrice || ticker.value?.lastPrice || '0')
})
const actionLabel = computed(() => {
  if (tradeMode.value === 'close') {
    return direction.value === 'Buy' ? '买入平空' : '卖出平多'
  }
  return direction.value === 'Buy' ? '买入开多' : '卖出开空'
})
const directionHint = computed(() => {
  if (tradeMode.value === 'close') {
    return direction.value === 'Buy' ? '将减少当前空仓' : '将减少当前多仓'
  }
  return direction.value === 'Buy' ? '预期开立多仓' : '预期开立空仓'
})
const canSubmit = computed(
  () => connectionStore.connected && !submitting.value && !validationMessage.value,
)

function formatQty(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return ''
  return value.toFixed(8).replace(/\.?0+$/, '')
}

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
    mode: tradeMode.value,
    percent,
    closeableQty: closeableQtyNumber.value,
    availableBalance: Number.parseFloat(usdtBalance.value?.available ?? '0'),
    referencePrice: referencePrice.value,
    leverage: leverageNumber.value,
  })
  if (nextQty == null) {
    notifyWarning(
      tradeMode.value === 'close' ? '当前方向没有可平仓位' : '需要有效余额和价格才能计算数量',
    )
    return
  }
  qty.value = formatQty(nextQty)
}

watch(
  [qty, price, orderType, tradeMode, direction, closeableQtyNumber, () => connectionStore.connected],
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
watch(orderType, (next) => {
  if (next === 'Market') price.value = ''
})

async function submit(): Promise<void> {
  updateValidation()
  if (validationMessage.value) {
    notifyWarning(validationMessage.value)
    return
  }
  submitting.value = true
  try {
    await orderStore.placeOrder({
      symbol: activeSymbol.value,
      side: direction.value,
      orderType: orderType.value,
      qty: qty.value,
      positionIdx: tradeMode.value === 'close' ? closePosition.value?.positionIdx : 0,
      price: orderType.value === 'Limit' ? price.value : undefined,
      reduceOnly: tradeMode.value === 'close',
    })
    await Promise.all([
      refreshSyncTask('privatePanels', true),
      refreshSyncTask('account', true),
    ])
    qty.value = ''
    sizePct.value = 0
    notifySuccess(`${actionLabel.value}委托已提交`)
  } catch (error) {
    reportError(error, `${actionLabel.value}失败`)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="order-panel">
    <section class="account-strip" aria-label="账户摘要">
      <div class="account-item">
        <span>可用保证金</span>
        <strong>{{ availableBalance }} USDT</strong>
      </div>
      <div class="account-item align-right">
        <span>账户权益</span>
        <strong>{{ equityBalance }} USDT</strong>
      </div>
    </section>

    <section class="trade-card">
      <div class="section-title">
        <div>
          <strong>交易方向</strong>
          <span>{{ directionHint }}</span>
        </div>
        <span class="symbol-badge">{{ activeSymbol }}</span>
      </div>
      <div class="direction-tabs" role="group" aria-label="交易方向">
        <NButton
          class="direction-button buy-direction"
          :class="{ active: direction === 'Buy' }"
          :secondary="direction !== 'Buy'"
          @click="direction = 'Buy'"
        >
          买入 / 做多
        </NButton>
        <NButton
          class="direction-button sell-direction"
          :class="{ active: direction === 'Sell' }"
          :secondary="direction !== 'Sell'"
          @click="direction = 'Sell'"
        >
          卖出 / 做空
        </NButton>
      </div>

      <NRadioGroup v-model:value="tradeMode" size="small" class="trade-tabs">
        <NRadioButton value="open">
          开仓
        </NRadioButton>
        <NRadioButton value="close">
          平仓
        </NRadioButton>
      </NRadioGroup>

      <NRadioGroup v-model:value="orderType" size="small" class="order-type">
        <NRadioButton value="Limit">
          限价
        </NRadioButton>
        <NRadioButton value="Market">
          市价
        </NRadioButton>
      </NRadioGroup>

      <div class="contract-summary">
        <div>
          <span>保证金模式</span>
          <strong>全仓</strong>
        </div>
        <div>
          <span>当前杠杆</span>
          <strong>{{ leverageLabel }}</strong>
        </div>
        <div v-if="tradeMode === 'close'">
          <span>可平数量</span>
          <strong>{{ closeableQty }}</strong>
        </div>
      </div>

      <label v-if="orderType === 'Limit'" class="field-row">
        <span>价格</span>
        <NInput v-model:value="price" size="small" placeholder="输入委托价格" inputmode="decimal">
          <template #suffix>USDT</template>
        </NInput>
      </label>
      <div v-else class="market-price-hint">
        <span>成交价格</span>
        <strong>以当前市场最优价格成交</strong>
      </div>

      <label class="field-row">
        <span>数量</span>
        <NInput v-model:value="qty" size="small" placeholder="输入委托数量" inputmode="decimal">
          <template #suffix>{{ activeSymbol.replace(/USDT$/i, '') }}</template>
        </NInput>
      </label>

      <div class="quick-percent" aria-label="快捷仓位比例">
        <NButton
          v-for="percent in [25, 50, 75, 100]"
          :key="percent"
          size="tiny"
          :type="sizePct === percent ? 'primary' : 'default'"
          @click="applyQuickPercent(percent)"
        >
          {{ percent }}%
        </NButton>
      </div>
      <div class="slider-block">
        <div class="slider-head">
          <span>仓位比例</span>
          <span>{{ sizePct }}%</span>
        </div>
        <NSlider
          v-model:value="sizePct"
          :step="25"
          :min="0"
          :max="100"
          @update:value="applyQuickPercent"
        />
      </div>

      <div v-if="validationMessage" class="validation-message" role="status">
        {{ validationMessage }}
      </div>

      <div class="action-hint">
        <span>{{ tradeMode === 'open' ? '开仓委托' : '只减仓委托' }}</span>
        <span>{{ orderType === 'Limit' ? '限价委托' : '市价委托' }}</span>
      </div>
      <NButton
        class="submit-button"
        :class="direction === 'Buy' ? 'buy-btn' : 'sell-btn'"
        :type="direction === 'Buy' ? 'primary' : 'error'"
        block
        :disabled="!canSubmit"
        :loading="submitting"
        @click="submit"
      >
        {{ actionLabel }}
      </NButton>
    </section>

    <TradingAssetPanel />
  </div>
</template>

<style scoped>
.order-panel {
  display: flex;
  flex-direction: column;
  gap: var(--ef-space-2);
  padding: var(--ef-space-3);
  min-height: 0;
  font-size: var(--ef-text-base);
}

.account-strip,
.trade-card {
  border: 1px solid var(--border);
  border-radius: var(--ef-radius-lg);
  background: color-mix(in srgb, var(--card) 88%, var(--accent) 12%);
}

.account-strip {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--ef-space-2);
  padding: var(--ef-space-3);
}

.account-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.account-item span,
.action-hint,
.field-row span,
.slider-head {
  color: var(--text-secondary);
  font-size: var(--ef-text-sm);
}

.account-item strong {
  color: var(--text);
  font-family: var(--font-mono);
  font-size: var(--ef-text-base);
  font-variant-numeric: tabular-nums;
  overflow: hidden;
  text-overflow: ellipsis;
}

.align-right {
  text-align: right;
  align-items: flex-end;
}

.trade-card {
  display: flex;
  flex-direction: column;
  gap: var(--ef-space-3);
  padding: var(--ef-space-3);
}

.section-title,
.section-title > div {
  display: flex;
  align-items: center;
}

.section-title {
  justify-content: space-between;
  gap: var(--ef-space-2);
}

.section-title > div {
  min-width: 0;
  gap: var(--ef-space-2);
}

.section-title strong {
  color: var(--text);
  font-size: var(--ef-text-base);
}

.section-title span,
.contract-summary span,
.market-price-hint span {
  color: var(--text-secondary);
  font-size: var(--ef-text-sm);
}

.symbol-badge {
  flex-shrink: 0;
  padding: 2px 7px;
  border: 1px solid var(--border);
  border-radius: 999px;
  font-family: var(--font-mono);
}

.direction-tabs {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--ef-space-2);
}

.direction-button {
  min-height: 40px;
  border-width: 1px;
  font-weight: 700;
}

.buy-direction.active {
  color: white;
  border-color: var(--accent-green);
  background: var(--accent-green);
}

.sell-direction.active {
  color: white;
  border-color: var(--accent-red);
  background: var(--accent-red);
}

.contract-summary {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: var(--ef-space-2);
  padding: var(--ef-space-2);
  border: 1px solid var(--border);
  border-radius: var(--ef-radius-md);
  background: color-mix(in srgb, var(--card) 94%, var(--accent) 6%);
}

.contract-summary > div {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.contract-summary strong,
.market-price-hint strong {
  overflow: hidden;
  color: var(--text);
  font-family: var(--font-mono);
  font-size: var(--ef-text-sm);
  text-overflow: ellipsis;
}

.market-price-hint {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--ef-space-2);
  min-height: 34px;
  padding: 0 var(--ef-space-2);
  border: 1px dashed var(--border);
  border-radius: var(--ef-radius-md);
}

.quick-percent {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--ef-space-1);
}

.validation-message {
  padding: var(--ef-space-2);
  border: 1px solid color-mix(in srgb, var(--accent-red) 55%, var(--border));
  border-radius: var(--ef-radius-md);
  color: var(--accent-red);
  background: color-mix(in srgb, var(--accent-red) 8%, transparent);
  font-size: var(--ef-text-sm);
}

.trade-tabs,
.order-type {
  width: 100%;
}

.field-row {
  display: flex;
  flex-direction: column;
  gap: var(--ef-space-1);
}

.slider-block {
  display: flex;
  flex-direction: column;
  gap: var(--ef-space-1);
}

.slider-head {
  display: flex;
  justify-content: space-between;
}

.action-hint {
  display: flex;
  justify-content: space-between;
  gap: var(--ef-space-2);
}

.buy-btn {
  background: var(--accent-green);
  border-color: var(--accent-green);
}

.sell-btn {
  background: var(--accent-red);
  border-color: var(--accent-red);
}

.submit-button {
  min-height: clamp(38px, 2rem + 0.7vw, 48px);
  font-weight: 700;
}

.order-panel :deep(.n-radio-button),
.order-panel :deep(.n-input) {
  font-size: var(--ef-text-sm);
}

.order-panel :deep(.n-radio-button__state-border) {
  border-radius: var(--ef-radius-md);
}

@media (max-width: 1180px) {
  .account-strip {
    grid-template-columns: 1fr;
  }

  .align-right {
    align-items: flex-start;
    text-align: left;
  }

  .contract-summary {
    grid-template-columns: 1fr 1fr;
  }
}
</style>
