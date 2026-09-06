<script setup lang="ts">
import { NButton, NInput, NRadioButton, NRadioGroup, NSlider } from 'naive-ui'
import { useOrderPanel } from '../../composables/useOrderPanel'
import TradingAssetPanel from './TradingAssetPanel.vue'

const {
  activeSymbol, direction, tradeMode, orderType, qty, price, sizePct, submitting,
  validationMessage, availableBalance, equityBalance, closeableQty, leverageLabel,
  actionLabel, directionHint, canSubmit, tradingBlockedMessage, tradingBlockIsFailure,
  pendingSubmissions, pendingLoading, pendingError, pendingStatusMessage, queryingOrderLinkId,
  canQuerySubmission, reconcileSubmission, refreshPendingSubmissions,
  applyQuickPercent, submit,
} = useOrderPanel()
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

    <section
      v-if="pendingSubmissions.length || pendingLoading || pendingError"
      class="trade-card pending-submissions"
      data-testid="pending-order-submissions"
      aria-label="待确认订单"
    >
      <strong>待确认订单</strong>
      <p class="pending-guidance">
        请查询原订单结果；结果未确认前请勿重复提交。
      </p>
      <div v-for="pending in pendingSubmissions" :key="pending.orderLinkId" class="pending-submission">
        <strong>{{ pending.symbol }} · {{ pending.side === 'Buy' ? '买入' : '卖出' }} · {{ pending.reduceOnly ? '只减仓' : '开仓' }}</strong>
        <span>{{ pending.orderType === 'Market' ? '市价' : '限价' }} · 数量 {{ pending.qty }}<template v-if="pending.price"> · 价格 {{ pending.price }}</template></span>
        <span class="pending-order-id">订单标识：{{ pending.orderLinkId }}</span>
        <NButton
          size="small"
          :disabled="!canQuerySubmission"
          :loading="queryingOrderLinkId === pending.orderLinkId"
          @click="reconcileSubmission(pending.orderLinkId)"
        >
          查询订单结果
        </NButton>
      </div>
      <span v-if="pendingLoading" role="status">正在读取待确认订单…</span>
      <span v-if="pendingStatusMessage" role="status">{{ pendingStatusMessage }}</span>
      <div v-if="pendingError" role="alert">
        {{ pendingError }}
        <NButton size="small" :disabled="!canQuerySubmission" @click="refreshPendingSubmissions">
          重新读取待确认订单
        </NButton>
      </div>
    </section>

    <section class="trade-card">
      <div class="section-title">
        <div><strong>交易方向</strong><span>{{ directionHint }}</span></div>
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
        <div><span>保证金模式</span><strong>全仓</strong></div>
        <div><span>当前杠杆</span><strong>{{ leverageLabel }}</strong></div>
        <div v-if="tradeMode === 'close'">
          <span>可平数量</span><strong>{{ closeableQty }}</strong>
        </div>
      </div>

      <label v-if="orderType === 'Limit'" class="field-row">
        <span>价格</span>
        <NInput v-model:value="price" size="small" placeholder="输入委托价格" inputmode="decimal">
          <template #suffix>USDT</template>
        </NInput>
      </label>
      <div v-else class="market-price-hint">
        <span>成交价格</span><strong>以当前市场最优价格成交</strong>
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
      <div
        v-if="tradingBlockedMessage"
        class="validation-message"
        data-testid="trading-blocked-message"
        :role="tradingBlockIsFailure ? 'alert' : 'status'"
      >
        {{ tradingBlockedMessage }}
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

<style scoped src="./orderPanelLayout.css"></style>
<style scoped src="./orderPanelControls.css"></style>

<style scoped>
.pending-submissions {
  border-color: var(--warning, #c98c31);
}

.pending-guidance {
  margin: 0;
  color: var(--text-secondary);
}

.pending-submission {
  display: flex;
  flex-direction: column;
  gap: var(--ef-space-2);
}

.pending-order-id {
  color: var(--text-secondary);
  overflow-wrap: anywhere;
  font-size: var(--ef-text-sm);
}
</style>
