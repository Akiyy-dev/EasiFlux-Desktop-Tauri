<script setup lang="ts">
export type AppButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger'
export type AppButtonSize = 'sm' | 'md'

withDefaults(
  defineProps<{
    variant?: AppButtonVariant
    size?: AppButtonSize
    type?: 'button' | 'submit' | 'reset'
    disabled?: boolean
    loading?: boolean
    iconOnly?: boolean
  }>(),
  {
    variant: 'secondary',
    size: 'md',
    type: 'button',
    disabled: false,
    loading: false,
    iconOnly: false,
  },
)
</script>

<template>
  <button
    class="ef-btn ef-motion-hover ef-motion-press ef-focus-ring"
    :class="[
      `ef-btn-${variant}`,
      `ef-btn-${size}`,
      { 'ef-btn-icon': iconOnly, 'is-loading': loading },
    ]"
    :type="type"
    :disabled="disabled || loading"
    :aria-busy="loading || undefined"
  >
    <slot />
  </button>
</template>

<style scoped>
.is-loading {
  opacity: 0.7;
  cursor: wait;
}
</style>
