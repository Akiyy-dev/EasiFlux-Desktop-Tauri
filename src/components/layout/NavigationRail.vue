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
import AppCard from '../ui/AppCard.vue'
import AppIcon from '../ui/AppIcon.vue'

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
      <button
        v-for="item in items"
        :key="item.key"
        class="rail-btn ef-motion-hover ef-motion-press"
        :class="{ active: props.active === item.key }"
        type="button"
        :title="item.label"
        @click="emit('select', item.key)"
      >
        <AppIcon :icon="item.icon" :size="18" />
      </button>
    </div>
    <div class="rail-bottom">
      <button
        class="rail-btn ef-motion-hover ef-motion-press"
        :class="{ active: props.active === 'settings' }"
        type="button"
        title="设置"
        @click="emit('openSettings')"
      >
        <AppIcon :icon="Settings" :size="18" />
      </button>
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
  border-radius: 10px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--muted-foreground);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.rail-btn:hover {
  background: var(--accent);
  color: var(--foreground);
}

.rail-btn.active {
  background: var(--accent);
  color: var(--foreground);
  border-color: var(--border);
}
</style>
