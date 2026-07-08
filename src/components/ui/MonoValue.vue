<script setup lang="ts">
withDefaults(
  defineProps<{
    tag?: string
    size?: 'sm' | 'md' | 'lg'
  }>(),
  {
    tag: 'span',
    size: 'md',
  },
)
</script>

<template>
  <component :is="tag" class="ef-mono" :class="`ef-mono-${size}`">
    <slot />
  </component>
</template>
