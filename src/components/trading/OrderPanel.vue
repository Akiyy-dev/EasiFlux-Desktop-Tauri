<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  NButton,
  NCollapse,
  NCollapseItem,
  NInput,
  NRadioButton,
  NRadioGroup,
  NSlider,
} from 'naive-ui'
import { storeToRefs } from 'pinia'
import { useAccountStore } from '../../stores/account'
import { useMarketStore } from '../../stores/market'
import { useOrderStore } from '../../stores/order'
import { useConnectionStore } from '../../stores/connection'
import TradingAssetPanel from './TradingAssetPanel.vue'
import { notifyInfo, notifySuccess, notifyWarning, reportError } from '../../services/errorService'

const accountStore = useAccountStore()
const marketStore = useMarketStore()
const orderStore = useOrderStore()
const connectionStore = useConnectionStore()

const { summary } = storeToRefs(accountStore)
const { activeSymbol } = storeToRefs(marketStore)

const marginMode = ref<'cross' | 'isolated'>('cross')
const leverage = ref('20')
const tradeTab = ref<'open' | 'close'>('open')
const orderType = ref<'Limit' | 'Market' | 'Plan'>('Limit')
const qty = ref('0.001')
const price = ref('')
const takeProfit = ref('')
const stopLoss = ref('')
const sizePct = ref(25)

const canSubmit = computed(() => connectionStore.connected && orderType.value !== 'Plan')

const buyLabel = computed(() => (tradeTab.value === 'open' ? '买入开多' : '买入平空'))
const sellLabel = computed(() => (tradeTab.value === 'open' ? '卖出开空' : '卖出平多'))
const usdtBalance = computed(() =>
  summary.value?.balances.find((balance) => balance.asset === 'USDT'),
)
const availableBalance = computed(() => usdtBalance.value?.available ?? '--')
const equityBalance = computed(() => summary.value?.totalEquity ?? '--')

watch(sizePct, (pct) => {
  const base = 0.01
  qty.value = ((base * pct) / 100).toFixed(4)
})

async function submit(selectedSide: 'Buy' | 'Sell'): Promise<void> {
  if (!connectionStore.connected) {
    notifyWarning('请先连接 API')
    return
  }
  if (orderType.value === 'Plan') {
    notifyInfo('计划委托将在后续版本开放')
    return
  }
  if (!qty.value || Number.parseFloat(qty.value) <= 0) {
    notifyWarning('请输入有效数量')
    return
  }
  if (orderType.value === 'Limit' && (!price.value || Number.parseFloat(price.value) <= 0)) {
    notifyWarning('请输入有效价格')
    return
  }

  try {
    await orderStore.placeOrder({
      symbol: activeSymbol.value,
      side: selectedSide,
      orderType: orderType.value,
      qty: qty.value,
      positionIdx: 0,
      price: orderType.value === 'Limit' ? price.value : undefined,
    })
    notifySuccess('下单成功')
  } catch (error) {
    reportError(error, '下单失败')
  }
}
</script>

<template>
  <div class="order-panel">
    <section class="account-strip" aria-label="账户摘要">
      <div class="account-item">
        <span>可用余额</span>
        <strong>{{ availableBalance }} USDT</strong>
      </div>
      <div class="account-item align-right">
        <span>账户权益</span>
        <strong>{{ equityBalance }} USDT</strong>
      </div>
    </section>

    <section class="mode-card">
      <div class="top-controls">
        <NRadioGroup v-model:value="marginMode" size="small" class="segmented">
          <NRadioButton value="cross">全仓</NRadioButton>
          <NRadioButton value="isolated">逐仓</NRadioButton>
        </NRadioGroup>
        <div class="leverage">
          <span class="label">杠杆</span>
          <NInput v-model:value="leverage" size="small" class="leverage-input" />
          <span class="suffix">x</span>
        </div>
      </div>

      <NRadioGroup v-model:value="tradeTab" size="small" class="trade-tabs">
        <NRadioButton value="open">开仓</NRadioButton>
        <NRadioButton value="close">平仓</NRadioButton>
      </NRadioGroup>
    </section>

    <section class="order-card">
      <NRadioGroup v-model:value="orderType" size="small" class="order-type">
        <NRadioButton value="Limit">限价</NRadioButton>
        <NRadioButton value="Market">市价</NRadioButton>
        <NRadioButton value="Plan" disabled>计划</NRadioButton>
      </NRadioGroup>

      <label v-if="orderType === 'Limit'" class="field-row">
        <span>价格</span>
        <NInput v-model:value="price" size="small" placeholder="输入价格" />
      </label>

      <label class="field-row">
        <span>数量</span>
        <NInput v-model:value="qty" size="small" placeholder="输入数量" />
      </label>

      <div class="slider-block">
        <div class="slider-head">
          <span>仓位比例</span>
          <span>{{ sizePct }}%</span>
        </div>
        <NSlider v-model:value="sizePct" :step="1" :min="0" :max="100" />
      </div>

      <div class="estimate-grid">
        <div>
          <span>保证金</span>
          <strong>--</strong>
        </div>
        <div>
          <span>手续费</span>
          <strong>--</strong>
        </div>
      </div>
    </section>

    <NCollapse class="risk-collapse">
      <NCollapseItem title="止盈止损" name="tpsl">
        <div class="tpsl-grid">
          <label class="field-row">
            <span>止盈价</span>
            <NInput v-model:value="takeProfit" size="small" placeholder="可选" />
          </label>
          <label class="field-row">
            <span>止损价</span>
            <NInput v-model:value="stopLoss" size="small" placeholder="可选" />
          </label>
        </div>
      </NCollapseItem>
    </NCollapse>

    <section class="actions-card">
      <div class="action-hint">
        <span>{{ activeSymbol }}</span>
        <span>{{ orderType === 'Limit' ? '限价委托' : '市价委托' }}</span>
      </div>
      <div class="actions">
        <NButton
          class="buy-btn"
          type="primary"
          block
          :disabled="!canSubmit"
          @click="submit('Buy')"
        >
          {{ buyLabel }}
        </NButton>
        <NButton
          class="sell-btn"
          type="error"
          block
          :disabled="!canSubmit"
          @click="submit('Sell')"
        >
          {{ sellLabel }}
        </NButton>
      </div>
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
.mode-card,
.order-card,
.actions-card {
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
.estimate-grid span,
.action-hint,
.field-row span,
.slider-head,
.leverage .label,
.leverage .suffix {
  color: var(--text-secondary);
  font-size: var(--ef-text-sm);
}

.account-item strong,
.estimate-grid strong {
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

.mode-card,
.order-card,
.actions-card {
  display: flex;
  flex-direction: column;
  gap: var(--ef-space-3);
  padding: var(--ef-space-3);
}

.top-controls {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--ef-space-2);
}

.leverage {
  display: flex;
  align-items: center;
  gap: var(--ef-space-1);
  flex-shrink: 0;
}

.leverage-input {
  width: clamp(64px, 4rem + 1vw, 84px);
}

.segmented,
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

.estimate-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--ef-space-2);
  padding-top: var(--ef-space-2);
  border-top: 1px solid var(--border);
}

.estimate-grid > div {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--ef-space-2);
}

.tpsl-grid {
  display: grid;
  gap: var(--ef-space-2);
}

.risk-collapse {
  border: 1px solid var(--border);
  border-radius: var(--ef-radius-lg);
  padding: 0 var(--ef-space-2);
  background: color-mix(in srgb, var(--card) 92%, var(--accent) 8%);
}

.action-hint {
  display: flex;
  justify-content: space-between;
  gap: var(--ef-space-2);
}

.actions {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--ef-space-2);
}

.buy-btn {
  background: var(--accent-green);
  border-color: var(--accent-green);
  min-height: clamp(38px, 2rem + 0.7vw, 48px);
  font-weight: 700;
}

.sell-btn {
  background: var(--accent-red);
  border-color: var(--accent-red);
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
  .account-strip,
  .estimate-grid,
  .actions {
    grid-template-columns: 1fr;
  }

  .align-right {
    align-items: flex-start;
    text-align: left;
  }
}
</style>
