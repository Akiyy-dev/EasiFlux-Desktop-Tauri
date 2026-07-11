<script setup lang="ts">
import MonoValue from '../ui/MonoValue.vue'

defineProps<{
  label: string
  value: string
  valueClass?: string
  detail?: string
  detailClass?: string
}>()
</script>

<template>
  <div class="metric">
    <span class="label">{{ label }}</span>
    <MonoValue class="value" :class="valueClass" size="sm">{{ value }}</MonoValue>
    <span v-if="detail" class="detail" :class="detailClass">{{ detail }}</span>
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

.detail {
  color: var(--muted-foreground);
  font-size: 10px;
  white-space: nowrap;
}

.detail.error {
  color: var(--down);
}
</style>
