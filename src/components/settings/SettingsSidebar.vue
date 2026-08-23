<script setup lang="ts">
import type { SettingsSection } from '../../types/navigation'
import { SETTINGS_SECTION_GROUPS } from './settingsSections'

defineProps<{ active: SettingsSection }>()
const emit = defineEmits<{ select: [section: SettingsSection] }>()
</script>

<template>
  <aside class="settings-sidebar" aria-label="设置分类">
    <section
      v-for="group in SETTINGS_SECTION_GROUPS"
      :key="group.key"
      class="settings-sidebar-group"
    >
      <h2 data-testid="settings-group-heading" class="settings-sidebar-heading">
        {{ group.label }}
      </h2>
      <div class="settings-sidebar-items">
        <button
          v-for="item in group.items"
          :key="item.key"
          type="button"
          class="settings-sidebar-item"
          :class="{ active: active === item.key }"
          :data-testid="`settings-nav-${item.key}`"
          :aria-current="active === item.key ? 'page' : undefined"
          @click="emit('select', item.key)"
        >
          {{ item.label }}
        </button>
      </div>
    </section>
  </aside>
</template>
