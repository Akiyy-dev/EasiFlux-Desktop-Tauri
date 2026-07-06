<script setup lang="ts">
import { Bell, Settings, User, Wifi } from 'lucide-vue-next'
import ConnectionStatus from '../common/ConnectionStatus.vue'
import AppIcon from '../ui/AppIcon.vue'
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
        <span class="version ef-mono ef-mono-sm">v{{ props.version }}</span>
      </div>
      <div class="divider" aria-hidden="true" />
      <div class="page">
        <span class="page-title">{{ props.title }}</span>
      </div>
    </div>
    <div class="right">
      <ConnectionStatus />
      <button
        class="icon-btn ef-motion-hover ef-motion-press"
        type="button"
        title="网络（占位）"
        disabled
      >
        <AppIcon :icon="Wifi" :size="16" />
      </button>
      <button
        class="icon-btn ef-motion-hover ef-motion-press"
        type="button"
        title="通知（占位）"
        disabled
      >
        <AppIcon :icon="Bell" :size="16" />
      </button>
      <button
        class="icon-btn ef-motion-hover ef-motion-press"
        type="button"
        title="用户（占位）"
        disabled
      >
        <AppIcon :icon="User" :size="16" />
      </button>
      <button
        class="icon-btn ef-motion-hover ef-motion-press"
        type="button"
        title="设置"
        :data-active="props.active === 'settings' ? 'true' : 'false'"
        @click="emit('openSettings')"
      >
        <AppIcon :icon="Settings" :size="16" />
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
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: var(--ef-radius-lg);
  flex-shrink: 0;
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
  color: var(--muted-foreground);
}

.divider {
  width: 1px;
  height: 18px;
  background: var(--border);
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
  gap: 8px;
  flex-shrink: 0;
}

.icon-btn {
  background: transparent;
  border: 1px solid var(--border);
  color: var(--foreground);
  border-radius: 10px;
  padding: 4px 8px;
  cursor: pointer;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.icon-btn:hover:not(:disabled) {
  border-color: var(--ring);
  color: var(--primary);
}

.icon-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.icon-btn[data-active='true'] {
  border-color: var(--ring);
  color: var(--primary);
}
</style>
