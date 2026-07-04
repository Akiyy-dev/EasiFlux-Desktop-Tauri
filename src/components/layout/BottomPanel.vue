<script setup lang="ts">
import { ref } from 'vue'
import OrderTable from '../trading/OrderTable.vue'
import PositionTable from '../trading/PositionTable.vue'
import LogPanel from '../common/LogPanel.vue'
import AnalyticsPanel from '../common/AnalyticsPanel.vue'

const activeTab = ref<'orders' | 'positions' | 'logs' | 'analytics'>('orders')

const tabs = [
  { key: 'orders' as const, label: '订单' },
  { key: 'positions' as const, label: '持仓' },
  { key: 'logs' as const, label: '日志' },
  { key: 'analytics' as const, label: '分析' },
]
</script>

<template>
  <footer class="bottom-panel panel">
    <div class="tabs">
      <button
        v-for="tab in tabs"
        :key="tab.key"
        class="tab"
        :class="{ active: activeTab === tab.key }"
        @click="activeTab = tab.key"
      >
        {{ tab.label }}
      </button>
    </div>
    <div class="tab-body">
      <OrderTable v-if="activeTab === 'orders'" />
      <PositionTable v-else-if="activeTab === 'positions'" />
      <LogPanel v-else-if="activeTab === 'logs'" />
      <AnalyticsPanel v-else :active="activeTab === 'analytics'" />
    </div>
  </footer>
</template>

<style scoped>
.bottom-panel {
  display: flex;
  flex-direction: column;
  height: 220px;
  min-height: 160px;
}

.tabs {
  display: flex;
  gap: 4px;
  padding: 6px 8px;
  border-bottom: 1px solid var(--border-color);
}

.tab {
  background: transparent;
  border: none;
  color: var(--text-secondary);
  padding: 6px 12px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
}

.tab.active {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.tab-body {
  flex: 1;
  overflow: auto;
  padding: 4px;
}
</style>
