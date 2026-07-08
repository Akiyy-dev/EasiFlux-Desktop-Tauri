<script setup lang="ts">
import { Bell, User, Wifi } from 'lucide-vue-next'
import { storeToRefs } from 'pinia'
import ConnectionStatus from '../common/ConnectionStatus.vue'
import { AppButton, AppIcon, MonoValue } from '../ui'
import { useAppStore } from '../../stores/app'

const props = defineProps<{
  title: string
}>()

const appStore = useAppStore()
const { version } = storeToRefs(appStore)
</script>

<template>
  <header class="top-bar" aria-label="TopBar">
    <div class="left">
      <div class="brand">
        <span class="ef-text-title">EasiFlux</span>
        <MonoValue class="version ef-text-caption" size="sm">v{{ version }}</MonoValue>
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
    </div>
  </header>
</template>

<style scoped>
.top-bar {
  min-height: clamp(46px, 2.4rem + 0.75vw, 58px);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 var(--ef-space-3);
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: var(--ef-radius-lg);
  flex-shrink: 0;
}

.left {
  display: flex;
  align-items: center;
  gap: var(--ef-space-3);
  min-width: 0;
}

.brand {
  display: flex;
  align-items: baseline;
  gap: var(--ef-space-2);
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
  gap: var(--ef-space-2);
  flex-shrink: 0;
}

</style>
