<script setup lang="ts">
withDefaults(
  defineProps<{
    title?: string
    compact?: boolean
    flush?: boolean
    as?: string
  }>(),
  {
    title: undefined,
    compact: false,
    flush: false,
    as: 'section',
  },
)
</script>

<template>
  <component :is="as" class="ef-card" :class="{ 'ef-card-compact': compact }">
    <header v-if="title || $slots.header" class="ef-card-header">
      <slot name="header">
        <h3 class="ef-card-title">{{ title }}</h3>
      </slot>
    </header>
    <div class="ef-card-body" :class="{ 'ef-card-body-flush': flush }">
      <slot />
    </div>
  </component>
</template>
