<script setup lang="ts">
import type { AccountSection } from '../../types/navigation'
import AccountAssetsPanel from './AccountAssetsPanel.vue'
import AccountProfilesPanel from './AccountProfilesPanel.vue'
import RiskControlPanel from './RiskControlPanel.vue'

const props = defineProps<{ activeSection: AccountSection }>()
</script>

<template>
  <div class="account-center">
    <AccountProfilesPanel v-if="props.activeSection === 'api'" />
    <AccountAssetsPanel
      v-else-if="props.activeSection === 'assets'"
      :active="props.activeSection === 'assets'"
    />
    <RiskControlPanel v-else :active="props.activeSection === 'risk'" />
  </div>
</template>

<style scoped>
.account-center {
  flex: 1;
  min-height: 0;
  overflow: auto;
}
</style>
