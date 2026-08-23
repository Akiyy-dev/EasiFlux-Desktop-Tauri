<script setup lang="ts">
import { ref } from 'vue'
import GeneralSettingsPanel from './GeneralSettingsPanel.vue'
import AccountSettingsPage from './AccountSettingsPage.vue'
import AboutSettingsPanel from './AboutSettingsPanel.vue'
import SettingsPlaceholder from './SettingsPlaceholder.vue'
import SettingsSidebar from './SettingsSidebar.vue'
import { SETTINGS_SECTION_BY_KEY } from './settingsSections'
import type { AccountSettingsSection, SettingsSection } from '../../types/navigation'

const props = withDefaults(defineProps<{
  initialSection?: SettingsSection
  initialAccountSection?: AccountSettingsSection
}>(), {
  initialSection: 'general',
  initialAccountSection: 'api',
})
const emit = defineEmits<{ back: [] }>()
const activeSection = ref<SettingsSection>(props.initialSection)

function selectSection(section: SettingsSection): void {
  activeSection.value = section
}
</script>

<template>
  <section class="settings-center-page">
    <SettingsSidebar :active="activeSection" @select="selectSection" />
    <main class="settings-center-main">
      <header class="settings-center-header">
        <button data-testid="settings-back" type="button" @click="emit('back')">
          返回
        </button>
        <h1>设置</h1>
      </header>
      <div class="settings-center-content">
        <GeneralSettingsPanel v-if="activeSection === 'general'" />
        <AccountSettingsPage
          v-else-if="activeSection === 'account'"
          :initial-section="props.initialAccountSection"
        />
        <AboutSettingsPanel v-else-if="activeSection === 'about'" />
        <SettingsPlaceholder v-else :section="SETTINGS_SECTION_BY_KEY[activeSection]" />
      </div>
    </main>
  </section>
</template>

<style src="./SettingsCenterPage.css"></style>
