<script setup lang="ts">
import { NTooltip } from 'naive-ui'

withDefaults(
  defineProps<{
    disabled?: boolean
    placement?: 'top' | 'bottom' | 'left' | 'right'
  }>(),
  {
    disabled: false,
    placement: 'top',
  },
)
</script>

<template>
  <NTooltip :disabled="disabled" :placement="placement" trigger="hover">
    <template #trigger>
      <slot name="trigger" />
    </template>
    <slot />
  </NTooltip>
</template>
