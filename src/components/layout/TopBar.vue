<script setup lang="ts">
import { Bell, Settings, User, Wifi } from 'lucide-vue-next'
import ConnectionStatus from '../common/ConnectionStatus.vue'
import { AppButton, AppIcon, MonoValue } from '../ui'
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
        <span class="ef-text-title">EasiFlux</span>
        <MonoValue class="version ef-text-caption" size="sm">v{{ props.version }}</MonoValue>
      </div>
      <div class="divider" aria-hidden="true" />
      <div class="page">
        <span class="ef-text-body page-title">{{ props.title }}</span>
      </div>
    </div>
    <div class="right">
      <ConnectionStatus />
      <AppButton variant="ghost" size="sm" icon-only title="网络（占位）" disabled>
        <AppIcon :icon="Wifi" :size="16" />
      </AppButton>
      <AppButton variant="ghost" size="sm" icon-only title="通知（占位）" disabled>
        <AppIcon :icon="Bell" :size="16" />
      </AppButton>
      <AppButton variant="ghost" size="sm" icon-only title="用户（占位）" disabled>
        <AppIcon :icon="User" :size="16" />
      </AppButton>
      <AppButton
        variant="ghost"
        size="sm"
        icon-only
        title="设置"
        :class="{ active: props.active === 'settings' }"
        @click="emit('openSettings')"
      >
        <AppIcon :icon="Settings" :size="16" />
      </AppButton>
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

.version {
  color: var(--text-secondary);
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
  font-weight: var(--ef-text-label-weight);
}

.right {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.right .active {
  border-color: var(--ring);
  color: var(--primary);
}
</style>
