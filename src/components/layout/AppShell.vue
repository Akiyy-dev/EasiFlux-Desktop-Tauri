<script setup lang="ts">
import { computed, ref } from 'vue'
import AppCard from '../ui/AppCard.vue'
import TopBar from './TopBar.vue'
import NavigationRail from './NavigationRail.vue'
import type { NavKey } from './NavigationRail.vue'
import Sidebar from './Sidebar.vue'
import TradingLayout from './TradingLayout.vue'
import DashboardPage from '../dashboard/DashboardPage.vue'

defineProps<{
  version: string
}>()

const emit = defineEmits<{
  openSettings: []
}>()

const active = ref<NavKey>('home')
const sidebarCollapsed = ref(false)

const pageTitle = computed(() => {
  const map: Record<NavKey, string> = {
    home: '首页',
    trading: '交易',
    charts: '图表',
    news: '新闻',
    account: '账户',
    plugins: '插件',
    settings: '设置',
  }
  return map[active.value]
})

function navigateTo(key: NavKey): void {
  active.value = key
}
</script>

<template>
  <div class="app-shell">
    <TopBar
      :version="version"
      :active="active"
      :title="pageTitle"
      @open-settings="emit('openSettings')"
    />
    <div class="workbench">
      <NavigationRail
        :active="active"
        @select="navigateTo"
        @open-settings="emit('openSettings')"
      />
      <Sidebar
        :active="active"
        :collapsed="sidebarCollapsed"
        @toggle-collapsed="sidebarCollapsed = !sidebarCollapsed"
      />

      <section class="main ef-motion-page">
        <DashboardPage
          v-if="active === 'home'"
          :version="version"
          @navigate="navigateTo"
        />
        <TradingLayout v-else-if="active === 'trading'" />
        <AppCard v-else :title="pageTitle" class="placeholder">
          <div class="placeholder-body">
            <div class="muted">该页面将在后续 PRD 中逐步迁移实现。</div>
            <div class="muted">当前已保留交易功能入口：左侧选择“交易”。</div>
          </div>
        </AppCard>
      </section>
    </div>
  </div>
</template>

<style scoped>
.app-shell {
  display: flex;
  flex-direction: column;
  height: 100vh;
  gap: 8px;
  padding: 0 8px 8px;
  overflow: hidden;
}

.workbench {
  display: flex;
  flex: 1;
  gap: 8px;
  min-height: 0;
  overflow: hidden;
}

.main {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  overflow: hidden;
}

.placeholder {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.placeholder-body {
  font-size: 13px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.muted {
  color: var(--muted-foreground);
}

@media (max-width: 900px) {
  .workbench {
    gap: 6px;
  }
}
</style>
