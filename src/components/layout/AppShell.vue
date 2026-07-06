<script setup lang="ts">
import { computed, ref } from 'vue'
import TopBar from './TopBar.vue'
import NavigationRail from './NavigationRail.vue'
import type { NavKey } from './NavigationRail.vue'
import Sidebar from './Sidebar.vue'
import LeftSidebar from './LeftSidebar.vue'
import CenterPanel from './CenterPanel.vue'
import RightPanel from './RightPanel.vue'
import BottomPanel from './BottomPanel.vue'

defineProps<{
  version: string
}>()

const emit = defineEmits<{
  openSettings: []
}>()

const active = ref<NavKey>('trading')
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
        @select="(key) => (active = key)"
        @open-settings="emit('openSettings')"
      />
      <Sidebar
        :active="active"
        :collapsed="sidebarCollapsed"
        @toggle-collapsed="sidebarCollapsed = !sidebarCollapsed"
      />

      <section class="main">
        <template v-if="active === 'trading'">
          <div class="main-grid">
            <LeftSidebar class="left" />
            <CenterPanel class="center" />
            <RightPanel class="right" />
          </div>
          <BottomPanel />
        </template>
        <template v-else>
          <div class="placeholder panel">
            <div class="panel-title">{{ pageTitle }}</div>
            <div class="placeholder-body">
              <div class="muted">该页面将在后续 PRD 中逐步迁移实现。</div>
              <div class="muted">当前已保留交易功能入口：左侧选择“交易”。</div>
            </div>
          </div>
        </template>
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
}

.workbench {
  display: flex;
  flex: 1;
  gap: 8px;
  min-height: 0;
}

.main {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.main-grid {
  display: grid;
  grid-template-columns: 200px 1fr 280px;
  gap: 8px;
  flex: 1;
  min-height: 0;
}

.left,
.center,
.right {
  min-height: 0;
}

.placeholder {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.placeholder-body {
  padding: 12px;
  font-size: 13px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.muted {
  color: var(--text-secondary);
}
</style>
