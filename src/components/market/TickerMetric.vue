<script setup lang="ts">
import MonoValue from '../ui/MonoValue.vue'

defineProps<{
  label: string
  value: string
  valueClass?: string
}>()
</script>

<template>
  <div class="metric">
    <span class="label">{{ label }}</span>
    <MonoValue class="value" :class="valueClass" size="sm">{{ value }}</MonoValue>
  </div>
</template>

<style scoped>
.metric {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 72px;
}

.label {
  font-size: 11px;
  color: var(--muted-foreground);
  white-space: nowrap;
}

.value {
  font-weight: 600;
  white-space: nowrap;
}
</style>
