<script setup lang="ts">
import { computed, ref } from 'vue'
import AppCard from '../ui/AppCard.vue'
import TopBar from './TopBar.vue'
import NavigationRail from './NavigationRail.vue'
import Sidebar from './Sidebar.vue'
import TradingLayout from './TradingLayout.vue'
import AccountCenterPage from '../account/AccountCenterPage.vue'
import DashboardPage from '../dashboard/DashboardPage.vue'
import ChartWorkspacePage from '../chart/ChartWorkspacePage.vue'
import { useChartWorkspaceAutosaveHost } from '../../composables/useChartWorkspaceAutosaveHost'
import { flushActiveChartWorkspace } from '../../services/chartWorkspaceFlushRegistry'
import { reportError } from '../../services/errorService'
import type {
  AccountSection,
  NavigationTarget,
  NavKey,
  NonAccountSection,
  SidebarSectionKey,
  SidebarTarget,
} from '../../types/navigation'

const emit = defineEmits<{
  openSettings: []
}>()

const activePage = ref<NavKey>('home')
const chartsVisited = ref(false)
const tradingVisited = ref(false)
const activeAccountSection = ref<AccountSection>('api')
const activeSecondary = ref<NonAccountSection>('welcome')
const sidebarCollapsed = ref(false)

useChartWorkspaceAutosaveHost()
const sidebarTarget = computed<SidebarTarget>(() => activePage.value === 'account'
  ? { page: 'account', section: activeAccountSection.value }
  : { page: activePage.value, section: activeSecondary.value })

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
  return map[activePage.value]
})

function isAccountSection(value: string): value is AccountSection {
  return value === 'api' || value === 'assets' || value === 'risk'
}

function isNonAccountSection(value: string): value is NonAccountSection {
  return value === 'welcome' || value === 'updates' || value === 'installed'
    || value === 'market' || value === 'manage'
}

function applyNavigation(target: NavigationTarget): void {
  activePage.value = target.page
  if (target.page === 'charts') chartsVisited.value = true
  if (target.page === 'trading') tradingVisited.value = true
  if (target.page === 'account' && target.section) {
    activeAccountSection.value = target.section
  } else if (target.page === 'home') {
    activeSecondary.value = 'welcome'
  } else if (target.page === 'plugins') {
    activeSecondary.value = 'installed'
  }
}

function navigateTo(target: NavKey | NavigationTarget): Promise<void> {
  const normalized = typeof target === 'string' ? { page: target } : target
  const flush = flushActiveChartWorkspace('page').catch((error: unknown) => {
    reportError(error, '图表页面切换前保存失败')
  })
  applyNavigation(normalized)
  return flush
}

function selectSection(section: SidebarSectionKey): void {
  if (activePage.value === 'account' && isAccountSection(section)) {
    activeAccountSection.value = section
    return
  }
  if (isNonAccountSection(section)) activeSecondary.value = section
}
</script>

<template>
  <div class="app-shell">
    <TopBar :title="pageTitle" />
    <div class="workbench">
      <NavigationRail
        :active="activePage"
        @select="navigateTo"
        @open-settings="emit('openSettings')"
      />
      <Sidebar
        v-if="activePage !== 'charts'"
        :target="sidebarTarget"
        :collapsed="sidebarCollapsed"
        @select-section="selectSection"
        @toggle-collapsed="sidebarCollapsed = !sidebarCollapsed"
      />

      <section class="main ef-motion-page">
        <DashboardPage
          v-if="activePage === 'home'"
          @navigate="navigateTo"
        />
        <TradingLayout
          v-if="tradingVisited"
          v-show="activePage === 'trading'"
          :active="activePage === 'trading'"
        />
        <ChartWorkspacePage
          v-if="chartsVisited"
          v-show="activePage === 'charts'"
          :active="activePage === 'charts'"
        />
        <AccountCenterPage
          v-if="activePage === 'account'"
          :active-section="activeAccountSection"
        />
        <AppCard
          v-if="activePage === 'news' || activePage === 'plugins' || activePage === 'settings'"
          :title="pageTitle"
          class="placeholder"
        >
          <div class="placeholder-body">
            <div class="muted">
              该页面将在后续 PRD 中逐步迁移实现。
            </div>
            <div class="muted">
              当前已保留交易功能入口：左侧选择“交易”。
            </div>
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
  gap: var(--ef-space-2);
  padding: 0 var(--ef-space-2) var(--ef-space-2);
  overflow: hidden;
}

.workbench {
  display: flex;
  flex: 1;
  gap: var(--ef-space-2);
  min-height: 0;
  overflow: hidden;
}

.main {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--ef-space-2);
  overflow: hidden;
}

.placeholder {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.placeholder-body {
  font-size: var(--ef-text-base);
  display: flex;
  flex-direction: column;
  gap: var(--ef-space-2);
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
