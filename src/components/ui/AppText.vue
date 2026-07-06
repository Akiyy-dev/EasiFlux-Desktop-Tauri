<script setup lang="ts">
export type AppTextVariant = 'heading' | 'title' | 'body' | 'caption' | 'code'

withDefaults(
  defineProps<{
    variant?: AppTextVariant
    as?: string
  }>(),
  {
    variant: 'body',
    as: 'span',
  },
)
</script>

<template>
  <component :is="as" :class="`ef-text-${variant}`">
    <slot />
  </component>
</template>
