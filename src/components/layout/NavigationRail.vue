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
  width: 56px;
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 6px;
  gap: 6px;
}

.rail :deep(.ef-card-body) {
  padding: 0;
  display: flex;
  flex-direction: column;
  flex: 1;
  gap: 6px;
}

.rail-top {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.rail-bottom {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.rail-btn {
  width: 44px;
  height: 44px;
  color: var(--text-secondary);
}

.rail-btn.active {
  background: var(--accent);
  color: var(--text);
  border-color: var(--border);
}
</style>
