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
  useMessage,
} from 'naive-ui'
import { storeToRefs } from 'pinia'
import { useMarketStore } from '../../stores/market'
import { useOrderStore } from '../../stores/order'
import { useConnectionStore } from '../../stores/connection'
import { useLogStore } from '../../stores/log'
import TradingAssetPanel from './TradingAssetPanel.vue'

const marketStore = useMarketStore()
const orderStore = useOrderStore()
const connectionStore = useConnectionStore()
const logStore = useLogStore()
const message = useMessage()

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

watch(sizePct, (pct) => {
  const base = 0.01
  qty.value = ((base * pct) / 100).toFixed(4)
})

async function submit(selectedSide: 'Buy' | 'Sell'): Promise<void> {
  if (!connectionStore.connected) {
    message.warning('请先连接 API')
    return
  }
  if (orderType.value === 'Plan') {
    message.info('计划委托将在后续版本开放')
    return
  }
  if (!qty.value || Number.parseFloat(qty.value) <= 0) {
    message.warning('请输入有效数量')
    return
  }
  if (orderType.value === 'Limit' && (!price.value || Number.parseFloat(price.value) <= 0)) {
    message.warning('请输入有效价格')
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
    message.success('下单成功')
  } catch (error) {
    const msg = error instanceof Error ? error.message : String(error)
    logStore.setError(msg)
    message.error(msg)
  }
}
</script>

<template>
  <div class="order-panel">
    <div class="top-controls">
      <NRadioGroup v-model:value="marginMode" size="small">
        <NRadioButton value="cross">全仓</NRadioButton>
        <NRadioButton value="isolated">逐仓</NRadioButton>
      </NRadioGroup>
      <div class="leverage">
        <span class="label">杠杆</span>
        <NInput v-model:value="leverage" size="small" style="width: 64px" />
        <span class="suffix">x</span>
      </div>
    </div>

    <NRadioGroup v-model:value="tradeTab" size="small" class="trade-tabs">
      <NRadioButton value="open">开仓</NRadioButton>
      <NRadioButton value="close">平仓</NRadioButton>
    </NRadioGroup>

    <NRadioGroup v-model:value="orderType" size="small" class="order-type">
      <NRadioButton value="Market">市价</NRadioButton>
      <NRadioButton value="Limit">限价</NRadioButton>
      <NRadioButton value="Plan" disabled>计划委托</NRadioButton>
    </NRadioGroup>

    <label v-if="orderType === 'Limit'">
      <span>价格</span>
      <NInput v-model:value="price" size="small" placeholder="输入价格" />
    </label>

    <label>
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

    <label>
      <span>保证金</span>
      <NInput size="small" placeholder="--" disabled />
    </label>

    <NCollapse>
      <NCollapseItem title="止盈止损" name="tpsl">
        <div class="tpsl-grid">
          <label>
            <span>止盈价</span>
            <NInput v-model:value="takeProfit" size="small" placeholder="可选" />
          </label>
          <label>
            <span>止损价</span>
            <NInput v-model:value="stopLoss" size="small" placeholder="可选" />
          </label>
        </div>
      </NCollapseItem>
    </NCollapse>

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

    <TradingAssetPanel />
  </div>
</template>

<style scoped>
.order-panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px;
  min-height: 0;
}

.top-controls {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.leverage {
  display: flex;
  align-items: center;
  gap: 6px;
}

.leverage .label,
.leverage .suffix {
  font-size: 12px;
  color: var(--text-secondary);
}

.trade-tabs,
.order-type {
  width: 100%;
}

label {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--text-secondary);
}

.slider-block {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.slider-head {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  color: var(--text-secondary);
}

.tpsl-grid {
  display: grid;
  gap: 8px;
}

.actions {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}

.buy-btn:deep(.n-button) {
  background: var(--accent-green);
  border-color: var(--accent-green);
}

.sell-btn:deep(.n-button) {
  background: var(--accent-red);
  border-color: var(--accent-red);
}
</style>
