<script setup lang="ts">
import { uiMotion } from '../../composables/useUiMotion'

export type AppTabItem<T extends string = string> = {
  key: T
  label: string
}

defineProps<{
  items: AppTabItem[]
  modelValue: string
}>()

const emit = defineEmits<{
  'update:modelValue': [string]
}>()
</script>

<template>
  <div class="ef-tabs" role="tablist">
    <button
      v-for="tab in items"
      :key="tab.key"
      role="tab"
      type="button"
      class="ef-tab"
      :class="[uiMotion.tab, { active: modelValue === tab.key }]"
      :aria-selected="modelValue === tab.key"
      @click="emit('update:modelValue', tab.key)"
    >
      {{ tab.label }}
    </button>
  </div>
</template>
