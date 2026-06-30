<script setup lang="ts">
import OrderPanel from '../trading/OrderPanel.vue'
import { useAccountStore } from '../../stores/account'
import { storeToRefs } from 'pinia'

const accountStore = useAccountStore()
const { summary } = storeToRefs(accountStore)
</script>

<template>
  <aside class="right-panel">
    <div class="panel order-panel-wrap">
      <div class="panel-title">交易</div>
      <OrderPanel />
    </div>
    <div class="panel summary-panel">
      <div class="panel-title">账户</div>
      <div class="summary-body">
        <div class="equity">
          <span class="label">权益</span>
          <span class="value">{{ summary?.totalEquity ?? '--' }} USDT</span>
        </div>
        <div
          v-for="balance in summary?.balances ?? []"
          :key="balance.asset"
          class="balance-row"
        >
          <span>{{ balance.asset }}</span>
          <span>{{ balance.available }}</span>
        </div>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.right-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
  height: 100%;
}

.order-panel-wrap {
  flex: 1;
  min-height: 0;
  overflow: auto;
}

.summary-body {
  padding: 12px;
  font-size: 13px;
}

.equity {
  display: flex;
  justify-content: space-between;
  margin-bottom: 12px;
}

.label {
  color: var(--text-secondary);
}

.value {
  font-weight: 600;
}

.balance-row {
  display: flex;
  justify-content: space-between;
  padding: 4px 0;
  color: var(--text-secondary);
}
</style>
