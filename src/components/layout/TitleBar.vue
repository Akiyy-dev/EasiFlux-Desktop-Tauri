<script setup lang="ts">
import ConnectionStatus from '../common/ConnectionStatus.vue'

defineProps<{
  version: string
}>()

const emit = defineEmits<{
  openSettings: []
}>()
</script>

<template>
  <header class="title-bar">
    <div class="brand">
      <span class="logo">EasiFlux</span>
      <span class="version">v{{ version }}</span>
    </div>
    <div class="actions">
      <ConnectionStatus />
      <button class="icon-btn" title="设置" @click="emit('openSettings')">⚙</button>
    </div>
  </header>
</template>

<style scoped>
.title-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
}

.brand {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.logo {
  font-weight: 700;
  font-size: 15px;
}

.version {
  font-size: 11px;
  color: var(--text-secondary);
}

.actions {
  display: flex;
  align-items: center;
  gap: 12px;
}

.icon-btn {
  background: transparent;
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  border-radius: 4px;
  padding: 4px 8px;
  cursor: pointer;
}

.icon-btn:hover {
  border-color: var(--accent-blue);
}
</style>
