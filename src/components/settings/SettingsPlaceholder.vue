<script setup lang="ts">
import type { SettingsSectionDefinition } from './settingsSections'

defineProps<{ section: SettingsSectionDefinition }>()
</script>

<template>
  <section data-testid="settings-placeholder" class="settings-placeholder">
    <h1>{{ section.label }}</h1>
    <p>
      {{ section.description }}
    </p>
    <p class="settings-placeholder-status">
      规划中
    </p>
  </section>
</template>
