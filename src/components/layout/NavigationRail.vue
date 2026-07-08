<script setup lang="ts">
import {
  ArrowLeftRight,
  BarChart3,
  Home,
  Newspaper,
  Puzzle,
  Settings,
  User,
} from 'lucide-vue-next'
import type { FunctionalComponent } from 'vue'
import { AppButton, AppCard, AppIcon } from '../ui'

export type NavKey =
  | 'home'
  | 'trading'
  | 'charts'
  | 'news'
  | 'account'
  | 'plugins'
  | 'settings'

const props = defineProps<{
  active: NavKey
}>()

const emit = defineEmits<{
  select: [key: NavKey]
  openSettings: []
}>()

const items: Array<{
  key: Exclude<NavKey, 'settings'>
  label: string
  icon: FunctionalComponent
}> = [
  { key: 'home', label: '首页', icon: Home },
  { key: 'trading', label: '交易', icon: ArrowLeftRight },
  { key: 'charts', label: '图表', icon: BarChart3 },
  { key: 'news', label: '新闻', icon: Newspaper },
  { key: 'account', label: '账户', icon: User },
  { key: 'plugins', label: '插件', icon: Puzzle },
]
</script>

<template>
  <AppCard as="nav" class="rail" aria-label="一级导航">
    <div class="rail-top">
      <AppButton
        v-for="item in items"
        :key="item.key"
        variant="ghost"
        size="md"
        icon-only
        class="rail-btn"
        :class="{ active: props.active === item.key }"
        :title="item.label"
        :aria-label="item.label"
        @click="emit('select', item.key)"
      >
        <AppIcon :icon="item.icon" :size="18" />
      </AppButton>
    </div>
    <div class="rail-bottom">
      <AppButton
        variant="ghost"
        size="md"
        icon-only
        class="rail-btn"
        :class="{ active: props.active === 'settings' }"
        title="设置"
        aria-label="设置"
        @click="emit('openSettings')"
      >
        <AppIcon :icon="Settings" :size="18" />
      </AppButton>
    </div>
  </AppCard>
</template>

<style scoped>
.rail {
  width: clamp(56px, 3rem + 1vw, 68px);
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: var(--ef-space-2);
  gap: var(--ef-space-2);
}

.rail :deep(.ef-card-body) {
  padding: 0;
  display: flex;
  flex-direction: column;
  flex: 1;
  gap: var(--ef-space-2);
}

.rail-top {
  display: flex;
  flex-direction: column;
  gap: var(--ef-space-2);
}

.rail-bottom {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: var(--ef-space-2);
}

.rail-btn {
  width: clamp(44px, 2.35rem + 0.9vw, 54px);
  height: clamp(44px, 2.35rem + 0.9vw, 54px);
  color: var(--text-secondary);
}

.rail-btn :deep(.ef-icon) {
  --ef-icon-size: clamp(1.1rem, 0.92rem + 0.48vw, 1.38rem);
}

.rail-btn.active {
  background: var(--accent);
  color: var(--text);
  border-color: var(--border);
}
</style>
