<script setup lang="ts">
defineProps<{
  label: string
  value: string
  valueClass?: string
}>()
</script>

<template>
  <div class="metric">
    <span class="label">{{ label }}</span>
    <span class="value" :class="valueClass">{{ value }}</span>
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
  color: var(--text-secondary);
  white-space: nowrap;
}

.value {
  font-size: 12px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
</style>
