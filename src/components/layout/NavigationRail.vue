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
import { computed } from 'vue'
import type { NavKey } from '../../types/navigation'
import { AppButton, AppCard, AppIcon } from '../ui'

const props = withDefaults(defineProps<{
  active: NavKey
  newsUnreadCount?: number
}>(), { newsUnreadCount: 0 })

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

const normalizedUnread = computed(() => Math.max(0, Math.floor(props.newsUnreadCount)))
const newsBadge = computed(() => normalizedUnread.value > 99 ? '99+' : String(normalizedUnread.value))

function accessibleLabel(key: Exclude<NavKey, 'settings'>, label: string): string {
  if (key !== 'news' || normalizedUnread.value === 0) return label
  return `${label}，${normalizedUnread.value} 条未读`
}
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
        :aria-label="accessibleLabel(item.key, item.label)"
        @click="emit('select', item.key)"
      >
        <AppIcon :icon="item.icon" :size="18" />
        <span
          v-if="item.key === 'news' && normalizedUnread > 0"
          class="news-unread-badge"
          aria-hidden="true"
        >{{ newsBadge }}</span>
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
  position: relative;
  width: clamp(44px, 2.35rem + 0.9vw, 54px);
  height: clamp(44px, 2.35rem + 0.9vw, 54px);
  color: var(--text-secondary);
}

.news-unread-badge {
  position: absolute;
  top: 3px;
  right: 2px;
  display: inline-flex;
  min-width: 18px;
  height: 18px;
  align-items: center;
  justify-content: center;
  padding: 0 4px;
  border: 1px solid var(--card);
  border-radius: 999px;
  color: var(--primary-foreground, #fff);
  background: var(--danger);
  font-size: 10px;
  font-variant-numeric: tabular-nums;
  font-weight: 700;
  line-height: 1;
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
