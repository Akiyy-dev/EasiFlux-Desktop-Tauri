<script setup lang="ts">
import { ref } from 'vue'
import PositionsTab from './order-center/PositionsTab.vue'
import OpenOrdersTab from './order-center/OpenOrdersTab.vue'
import OrderHistoryTab from './order-center/OrderHistoryTab.vue'
import TradeFillsTab from './order-center/TradeFillsTab.vue'
import ClosedPnlTab from './order-center/ClosedPnlTab.vue'

type OrderCenterTab = 'positions' | 'open' | 'history' | 'fills' | 'closedPnl'

const activeTab = ref<OrderCenterTab>('positions')

const tabs: Array<{ key: OrderCenterTab; label: string }> = [
  { key: 'positions', label: '持有仓位' },
  { key: 'open', label: '当前委托' },
  { key: 'history', label: '历史委托' },
  { key: 'fills', label: '成交历史' },
  { key: 'closedPnl', label: '平仓盈亏' },
]
</script>

<template>
  <footer class="order-center panel">
    <div class="tabs">
      <button
        v-for="tab in tabs"
        :key="tab.key"
        class="tab"
        :class="{ active: activeTab === tab.key }"
        type="button"
        @click="activeTab = tab.key"
      >
        {{ tab.label }}
      </button>
    </div>

    <div class="tab-body">
      <PositionsTab v-show="activeTab === 'positions'" :active="activeTab === 'positions'" />
      <OpenOrdersTab v-show="activeTab === 'open'" :active="activeTab === 'open'" />
      <OrderHistoryTab v-show="activeTab === 'history'" :active="activeTab === 'history'" />
      <TradeFillsTab v-show="activeTab === 'fills'" :active="activeTab === 'fills'" />
      <ClosedPnlTab v-show="activeTab === 'closedPnl'" :active="activeTab === 'closedPnl'" />
    </div>
  </footer>
</template>

<style scoped>
.order-center {
  display: flex;
  flex-direction: column;
  height: 260px;
  min-height: 180px;
}

.tabs {
  display: flex;
  gap: 4px;
  padding: 6px 8px;
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
  overflow-x: auto;
}

.tab {
  background: transparent;
  border: none;
  color: var(--text-secondary);
  padding: 6px 12px;
  border-radius: 8px;
  cursor: pointer;
  font-size: 12px;
  white-space: nowrap;
}

.tab:hover {
  color: var(--text-primary);
  background: var(--bg-tertiary);
}

.tab.active {
  background: var(--bg-tertiary);
  color: var(--text-primary);
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
