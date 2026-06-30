<script setup lang="ts">
import { ref } from 'vue'
import { NButton, NInput, NRadioButton, NRadioGroup, useMessage } from 'naive-ui'
import { storeToRefs } from 'pinia'
import { useMarketStore } from '../../stores/market'
import { useOrderStore } from '../../stores/order'
import { useConnectionStore } from '../../stores/connection'
import { useLogStore } from '../../stores/log'

const marketStore = useMarketStore()
const orderStore = useOrderStore()
const connectionStore = useConnectionStore()
const logStore = useLogStore()
const message = useMessage()

const { activeSymbol } = storeToRefs(marketStore)

const side = ref('Buy')
const orderType = ref('Limit')
const qty = ref('0.001')
const price = ref('')

async function submit(): Promise<void> {
  if (!connectionStore.connected) {
    message.warning('请先连接 API')
    return
  }
  if (!qty.value || parseFloat(qty.value) <= 0) {
    message.warning('请输入有效数量')
    return
  }
  try {
    await orderStore.placeOrder({
      symbol: activeSymbol.value,
      side: side.value,
      orderType: orderType.value,
      qty: qty.value,
      price: orderType.value === 'Limit' ? price.value : undefined,
    })
    message.success('下单成功')
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e)
    logStore.setError(msg)
    message.error(msg)
  }
}
</script>

<template>
  <div class="order-panel">
    <NRadioGroup v-model:value="side" size="small" class="side-group">
      <NRadioButton value="Buy" class="buy">买入</NRadioButton>
      <NRadioButton value="Sell" class="sell">卖出</NRadioButton>
    </NRadioGroup>

    <NRadioGroup v-model:value="orderType" size="small">
      <NRadioButton value="Limit">限价</NRadioButton>
      <NRadioButton value="Market">市价</NRadioButton>
    </NRadioGroup>

    <label v-if="orderType === 'Limit'">
      价格
      <NInput v-model:value="price" size="small" placeholder="限价" />
    </label>

    <label>
      数量
      <NInput v-model:value="qty" size="small" placeholder="数量" />
    </label>

    <NButton type="primary" block :disabled="!connectionStore.connected" @click="submit">
      {{ side === 'Buy' ? '买入' : '卖出' }} {{ activeSymbol }}
    </NButton>
  </div>
</template>

<style scoped>
.order-panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px;
}

label {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--text-secondary);
}

.side-group :deep(.buy.n-radio-button--checked) {
  background: var(--accent-green);
}

.side-group :deep(.sell.n-radio-button--checked) {
  background: var(--accent-red);
}
</style>
