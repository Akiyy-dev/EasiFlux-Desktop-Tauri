<script setup lang="ts">
import MonoValue from '../ui/MonoValue.vue'

defineProps<{
  label: string
  value: string
  valueClass?: string
}>()
</script>

<template>
  <div class="metric-tile">
    <span class="label">{{ label }}</span>
    <MonoValue class="value" :class="valueClass" size="md">{{ value }}</MonoValue>
  </div>
</template>

<style scoped>
.metric-tile {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: var(--ef-radius-lg);
  background: color-mix(in srgb, var(--card) 88%, var(--accent) 12%);
  min-width: 0;
}

.label {
  font-size: 11px;
  color: var(--muted-foreground);
  white-space: nowrap;
}

.value {
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
