<script setup lang="ts">
import type { NavKey } from '../layout/NavigationRail.vue'
import DashboardHero from './DashboardHero.vue'
import DashboardAssetOverview from './DashboardAssetOverview.vue'
import DashboardQuickActions from './DashboardQuickActions.vue'
import DashboardMarketOverview from './DashboardMarketOverview.vue'
import DashboardRecentActivity from './DashboardRecentActivity.vue'
import DashboardStatusBar from './DashboardStatusBar.vue'
import type { DashboardNavTarget } from './types'

defineProps<{
  version: string
}>()

const emit = defineEmits<{
  navigate: [NavKey]
}>()

function handleQuickNavigate(target: DashboardNavTarget | 'positions' | 'assets'): void {
  if (target === 'positions') {
    emit('navigate', 'trading')
    return
  }
  if (target === 'assets') {
    emit('navigate', 'account')
    return
  }
  emit('navigate', target)
}
</script>

<template>
  <div class="dashboard">
    <div class="dashboard-scroll">
      <DashboardHero :version="version" />

      <div class="dashboard-grid">
        <section class="primary-column">
          <DashboardAssetOverview />
          <DashboardQuickActions @navigate="handleQuickNavigate" />
        </section>

        <section class="secondary-column">
          <DashboardMarketOverview />
          <DashboardRecentActivity />
        </section>
      </div>
    </div>

    <DashboardStatusBar :version="version" />
  </div>
</template>

<style scoped>
.dashboard {
  display: flex;
  flex-direction: column;
  gap: 8px;
  height: 100%;
  min-height: 0;
  overflow: hidden;
}

.dashboard-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding-right: 2px;
}

.dashboard-grid {
  display: grid;
  grid-template-columns: minmax(0, 1.15fr) minmax(280px, 0.85fr);
  gap: 8px;
  align-items: start;
}

.primary-column,
.secondary-column {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

@media (max-width: 1100px) {
  .dashboard-grid {
    grid-template-columns: 1fr;
  }
}
</style>
