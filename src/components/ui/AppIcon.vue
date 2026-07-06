<script setup lang="ts">
import type { FunctionalComponent } from 'vue'

defineProps<{
  icon: FunctionalComponent
  size?: number
  strokeWidth?: number
}>()
</script>

<template>
  <component
    :is="icon"
    class="ef-icon"
    :size="size ?? 18"
    :stroke-width="strokeWidth ?? 2"
    aria-hidden="true"
  />
</template>
