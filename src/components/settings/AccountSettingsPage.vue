<script setup lang="ts">
import { ref, watch } from 'vue'
import AccountAssetsPanel from '../account/AccountAssetsPanel.vue'
import AccountProfilesPanel from '../account/AccountProfilesPanel.vue'
import RiskControlPanel from '../account/RiskControlPanel.vue'
import AppTabs from '../ui/AppTabs.vue'
import type { AppTabItem } from '../ui/AppTabs.vue'
import type { AccountSettingsSection } from '../../types/navigation'

const props = withDefaults(defineProps<{ initialSection?: AccountSettingsSection }>(), {
  initialSection: 'api',
})
const activeSection = ref<AccountSettingsSection>(props.initialSection)
const tabs: AppTabItem<AccountSettingsSection>[] = [
  { key: 'api', label: 'API 账户' },
  { key: 'assets', label: '资产' },
  { key: 'risk', label: '风险' },
]

watch(() => props.initialSection, (section) => {
  activeSection.value = section
})

function selectSection(section: string): void {
  activeSection.value = section as AccountSettingsSection
}
</script>

<template>
  <section class="account-settings-page">
    <AppTabs
      :items="tabs"
      :model-value="activeSection"
      @update:model-value="selectSection"
    />
    <div class="account-settings-content">
      <AccountProfilesPanel v-if="activeSection === 'api'" />
      <AccountAssetsPanel v-else-if="activeSection === 'assets'" :active="true" />
      <RiskControlPanel v-else :active="true" />
    </div>
  </section>
</template>

<style scoped>
.account-settings-page,
.account-settings-content {
  display: grid;
  gap: var(--ef-space-3);
}
</style>
