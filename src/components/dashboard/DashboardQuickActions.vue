<script setup lang="ts">
import {
  ArrowLeftRight,
  Briefcase,
  Newspaper,
  Puzzle,
  Wallet,
} from 'lucide-vue-next'
import type { FunctionalComponent } from 'vue'
import AppCard from '../ui/AppCard.vue'
import AppIcon from '../ui/AppIcon.vue'
import type { DashboardNavTarget } from './types'

const emit = defineEmits<{
  navigate: [DashboardNavTarget | 'positions' | 'assets']
}>()

type QuickActionItem = {
  key: DashboardNavTarget | 'positions' | 'assets'
  label: string
  description: string
  icon: FunctionalComponent
}

const actions: QuickActionItem[] = [
  { key: 'trading', label: '开始交易', description: '进入交易终端', icon: ArrowLeftRight },
  { key: 'positions', label: '查看持仓', description: '仓位与委托', icon: Briefcase },
  { key: 'assets', label: '查看资产', description: '账户与保证金', icon: Wallet },
  { key: 'news', label: '新闻中心', description: '市场资讯', icon: Newspaper },
  { key: 'plugins', label: '插件市场', description: '扩展能力', icon: Puzzle },
]
</script>

<template>
  <AppCard title="快捷入口">
    <div class="quick-grid">
      <button
        v-for="action in actions"
        :key="action.key"
        type="button"
        class="quick-btn ef-motion-hover ef-motion-press"
        @click="emit('navigate', action.key)"
      >
        <span class="icon-wrap">
          <AppIcon :icon="action.icon" :size="18" />
        </span>
        <span class="text">
          <span class="label">{{ action.label }}</span>
          <span class="desc">{{ action.description }}</span>
        </span>
      </button>
    </div>
  </AppCard>
</template>

<style scoped>
.quick-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(168px, 1fr));
  gap: 8px;
}

.quick-btn {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 12px;
  border: 1px solid var(--border);
  border-radius: var(--ef-radius-lg);
  background: transparent;
  color: var(--foreground);
  cursor: pointer;
  text-align: left;
}

.quick-btn:hover {
  background: var(--accent);
  border-color: color-mix(in srgb, var(--border) 70%, var(--ring));
}

.icon-wrap {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border-radius: 10px;
  background: color-mix(in srgb, var(--accent) 80%, var(--primary) 20%);
  color: var(--primary);
  flex-shrink: 0;
}

.text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.label {
  font-size: 13px;
  font-weight: 600;
}

.desc {
  font-size: 11px;
  color: var(--muted-foreground);
}
</style>
