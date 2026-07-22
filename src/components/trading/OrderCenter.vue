<script setup lang="ts">
import { ref } from 'vue'
import AppCard from '../ui/AppCard.vue'
import AppTabs from '../ui/AppTabs.vue'
import PositionsTab from './order-center/PositionsTab.vue'
import OpenOrdersTab from './order-center/OpenOrdersTab.vue'
import OrderHistoryTab from './order-center/OrderHistoryTab.vue'
import TradeFillsTab from './order-center/TradeFillsTab.vue'
import ClosedPnlTab from './order-center/ClosedPnlTab.vue'

type OrderCenterTab = 'positions' | 'open' | 'history' | 'fills' | 'closedPnl'

const activeTab = ref<OrderCenterTab>('positions')

const tabs = [
  { key: 'positions' as const, label: '持有仓位' },
  { key: 'open' as const, label: '当前委托' },
  { key: 'history' as const, label: '历史委托' },
  { key: 'fills' as const, label: '成交历史' },
  { key: 'closedPnl' as const, label: '平仓盈亏' },
]
</script>

<template>
  <AppCard as="footer" flush class="order-center">
    <template #header>
      <AppTabs v-model="activeTab" :items="tabs" class="order-center-tabs" />
    </template>

    <div class="tab-body ef-motion-fade">
      <PositionsTab v-show="activeTab === 'positions'" :active="activeTab === 'positions'" />
      <OpenOrdersTab v-show="activeTab === 'open'" :active="activeTab === 'open'" />
      <OrderHistoryTab v-show="activeTab === 'history'" :active="activeTab === 'history'" />
      <TradeFillsTab v-show="activeTab === 'fills'" :active="activeTab === 'fills'" />
      <ClosedPnlTab v-show="activeTab === 'closedPnl'" :active="activeTab === 'closedPnl'" />
    </div>
  </AppCard>
</template>

<style scoped>
.order-center {
  display: flex;
  flex-direction: column;
  height: 260px;
  min-height: 180px;
}

.order-center :deep(.ef-card-header) {
  padding: 0;
  border-bottom: none;
}

.order-center-tabs {
  width: 100%;
  border-bottom: 1px solid var(--border);
}

.tab-body {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  padding: 6px;
}

.tab-body > * {
  height: 100%;
}
</style>
