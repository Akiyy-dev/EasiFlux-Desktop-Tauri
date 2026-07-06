<script setup lang="ts">
import ConnectionStatus from '../common/ConnectionStatus.vue'
import type { NavKey } from './NavigationRail.vue'

const props = defineProps<{
  version: string
  active: NavKey
  title: string
}>()

const emit = defineEmits<{
  openSettings: []
}>()
</script>

<template>
  <header class="top-bar" aria-label="TopBar">
    <div class="left">
      <div class="brand">
        <span class="logo">EasiFlux</span>
        <span class="version">v{{ props.version }}</span>
      </div>
      <div class="divider" aria-hidden="true" />
      <div class="page">
        <span class="page-title">{{ props.title }}</span>
      </div>
    </div>
    <div class="right">
      <ConnectionStatus />
      <button class="icon-btn" type="button" title="网络（占位）" disabled>⌁</button>
      <button class="icon-btn" type="button" title="通知（占位）" disabled>⎔</button>
      <button class="icon-btn" type="button" title="用户（占位）" disabled>☺</button>
      <button
        class="icon-btn"
        type="button"
        title="设置"
        :data-active="props.active === 'settings' ? 'true' : 'false'"
        @click="emit('openSettings')"
      >
        ⚙
      </button>
    </div>
  </header>
</template>

<style scoped>
.top-bar {
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 12px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
}

.left {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.brand {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.logo {
  font-weight: 700;
  font-size: 14px;
  letter-spacing: 0.02em;
}

.version {
  font-size: 11px;
  color: var(--text-secondary);
}

.divider {
  width: 1px;
  height: 18px;
  background: var(--border-color);
}

.page {
  min-width: 0;
}

.page-title {
  font-size: 13px;
  font-weight: 600;
}

.right {
  display: flex;
  align-items: center;
  gap: 10px;
}

.icon-btn {
  background: transparent;
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  border-radius: 10px;
  padding: 4px 8px;
  cursor: pointer;
  height: 28px;
}

.icon-btn:hover:not(:disabled) {
  border-color: var(--accent-blue);
}

.icon-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
</style>
