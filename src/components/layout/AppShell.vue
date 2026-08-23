<script setup lang="ts">
import { computed, ref } from 'vue'
import AppCard from '../ui/AppCard.vue'
import TopBar from './TopBar.vue'
import NavigationRail from './NavigationRail.vue'
import Sidebar from './Sidebar.vue'
import TradingLayout from './TradingLayout.vue'
import DashboardPage from '../dashboard/DashboardPage.vue'
import ChartWorkspacePage from '../chart/ChartWorkspacePage.vue'
import SettingsCenterPage from '../settings/SettingsCenterPage.vue'
import { useChartWorkspaceAutosaveHost } from '../../composables/useChartWorkspaceAutosaveHost'
import { flushActiveChartWorkspace } from '../../services/chartWorkspaceFlushRegistry'
import { reportError } from '../../services/errorService'
import type {
  AccountSettingsSection,
  HomeSection,
  NavigationRequest,
  NavigationTarget,
  PluginSection,
  PrimaryPage,
  SettingsSection,
  SidebarSectionKey,
  SidebarTarget,
} from '../../types/navigation'

const activePage = ref<PrimaryPage>('home')
const previousNonSettingsPage = ref<Exclude<PrimaryPage, 'settings'>>('home')
const settingsSessionId = ref(0)
const chartsVisited = ref(false)
const tradingVisited = ref(false)
const activeHomeSection = ref<HomeSection>('welcome')
const activePluginSection = ref<PluginSection>('installed')
const settingsTarget = ref({
  settingsSection: 'general' as SettingsSection,
  accountSection: 'api' as AccountSettingsSection,
})
const sidebarCollapsed = ref(false)

useChartWorkspaceAutosaveHost()
const sidebarTarget = computed<SidebarTarget | null>(() => {
  if (activePage.value === 'home') {
    return { page: 'home', section: activeHomeSection.value }
  }
  if (activePage.value === 'plugins') {
    return { page: 'plugins', section: activePluginSection.value }
  }
  return null
})

const pageTitle = computed(() => {
  const map: Record<PrimaryPage, string> = {
    home: '首页',
    trading: '交易',
    charts: '图表',
    plugins: '插件',
    settings: '设置',
  }
  return map[activePage.value]
})

function isHomeSection(value: SidebarSectionKey): value is HomeSection {
  return value === 'welcome' || value === 'updates'
}

function isPluginSection(value: SidebarSectionKey): value is PluginSection {
  return value === 'installed' || value === 'market' || value === 'manage'
}

function normalizeNavigation(request: NavigationRequest): NavigationTarget {
  if (typeof request !== 'string') return request
  if (request === 'settings') return { page: 'settings' }
  return { page: request }
}

function applyNavigation(target: NavigationTarget): void {
  if (target.page === 'settings') {
    if (activePage.value !== 'settings') {
      previousNonSettingsPage.value = activePage.value
    }
    settingsTarget.value = target.settingsSection === 'account'
      ? {
          settingsSection: 'account',
          accountSection: target.accountSection ?? 'api',
        }
      : {
          settingsSection: target.settingsSection ?? 'general',
          accountSection: 'api',
        }
    settingsSessionId.value += 1
    activePage.value = 'settings'
    return
  }

  activePage.value = target.page
  if (target.page === 'trading') tradingVisited.value = true
  if (target.page === 'charts') chartsVisited.value = true
  if (target.page === 'home') activeHomeSection.value = 'welcome'
  if (target.page === 'plugins') activePluginSection.value = 'installed'
}

function navigateTo(request: NavigationRequest): Promise<void> {
  const normalized = normalizeNavigation(request)
  if (normalized.page === 'settings'
    && activePage.value === 'settings'
    && typeof request === 'string') {
    return Promise.resolve()
  }
  const pending = flushActiveChartWorkspace('page').catch((error: unknown) => {
    reportError(error, '图表页面切换前保存失败')
  })
  applyNavigation(normalized)
  return pending
}

function returnToWorkspace(): Promise<void> {
  return navigateTo(previousNonSettingsPage.value)
}

function selectSection(section: SidebarSectionKey): void {
  if (activePage.value === 'home' && isHomeSection(section)) {
    activeHomeSection.value = section
  }
  if (activePage.value === 'plugins' && isPluginSection(section)) {
    activePluginSection.value = section
  }
}
</script>

<template>
  <div class="app-shell">
    <TopBar :title="pageTitle" />
    <div class="workbench">
      <NavigationRail
        :active="activePage"
        @select="navigateTo"
      />
      <Sidebar
        v-if="sidebarTarget"
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
        <SettingsCenterPage
          v-if="activePage === 'settings'"
          :key="settingsSessionId"
          :initial-section="settingsTarget.settingsSection"
          :initial-account-section="settingsTarget.accountSection"
          @back="returnToWorkspace"
        />
        <AppCard
          v-if="activePage === 'plugins'"
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

<style scoped src="./AppShell.css"></style>
